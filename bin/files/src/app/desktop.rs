//! Single-instance D-Bus negotiation and `org.freedesktop.FileManager1` command dispatch.

use super::{APP_ID, config::Mode};
use crate::shared::app::{Files, FilesExt as _};
use gds::app::AppExt as _;
use gds::service::desktop::file_manager::{Command, Error, Server};
use tokio::sync::mpsc;

pub(super) enum Acquire {
    /// Another Files instance handled the launch request (or already owns the service bus name);
    /// this process should exit cleanly without touching local state.
    Delegated,
    /// This process continues startup, optionally holding the session-bus server.
    Continue(Option<DesktopIntegration>),
}

pub(super) struct DesktopIntegration {
    server: Server,
    receiver: mpsc::Receiver<Command>,
}

impl DesktopIntegration {
    /// Negotiates single-instance ownership on the session bus according to `mode`.
    ///
    /// # Errors
    /// Returns an error if forwarding a browser target to an already-running instance fails, or
    /// if `--dbus-service` cannot acquire the session bus.
    pub async fn acquire(mode: &Mode, notices: &mut Vec<String>) -> anyhow::Result<Acquire> {
        if matches!(mode, Mode::Picker { .. }) {
            // Picker windows are transient modal helpers spawned by portal or CLI callers and
            // must not claim or delegate to the primary browser instance.
            return Ok(Acquire::Continue(None));
        }

        let (sender, receiver) = mpsc::channel(32);
        match Server::connect(APP_ID, sender).await {
            Ok(server) => {
                if let Some(provider) = &server.provider_conflict {
                    let message = format!(
                        "org.freedesktop.FileManager1 is already provided by {provider}; Files did not take ownership of this interface."
                    );
                    tracing::warn!(%message, "file-manager integration unavailable");
                    notices.push(message);
                }
                Ok(Acquire::Continue(Some(Self { server, receiver })))
            }
            Err(Error::AlreadyRunning) => {
                if let Mode::Browser { start } = mode {
                    let startup_id = std::env::var("XDG_ACTIVATION_TOKEN")
                        .or_else(|_| std::env::var("DESKTOP_STARTUP_ID"))
                        .unwrap_or_default();
                    Server::open_existing(APP_ID, start, &startup_id).await?;
                }
                Ok(Acquire::Delegated)
            }
            // D-Bus activation (`Mode::Service`) has no UI without the bus and must fail fast,
            // whereas interactive browser launches degrade to standalone windows with a notice.
            Err(error) if matches!(mode, Mode::Service) => Err(error.into()),
            Err(error) => {
                tracing::warn!(%error, "file-manager integration unavailable");
                notices.push(error.to_string());
                Ok(Acquire::Continue(None))
            }
        }
    }
}

impl Files<&mut gpui_kit::App> {
    /// Spawns the command forwarding loop once UI services and the window host are installed.
    ///
    /// # Errors
    /// Returns an error if the task supervisor is already shutting down.
    pub(in crate::app) fn listen_desktop(
        &mut self,
        desktop: DesktopIntegration,
    ) -> anyhow::Result<()> {
        let DesktopIntegration {
            server,
            mut receiver,
        } = desktop;
        let cx = self.app();
        let tasks = cx.gds_ro().tasks().scope();
        let cancel = tasks.token();
        let completion = tasks.register_named(
            "Desktop requests",
            gds::service::task_manager::Execution::Supervisor,
        )?;
        cx.spawn(async move |cx| {
            loop {
                let command = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => None,
                    command = receiver.recv() => command,
                };
                let Some(command) = command else {
                    break;
                };
                cx.update(|cx| cx.gds().files().windows().dispatch_desktop(command));
            }
            drop(server);
            drop(completion);
        })
        .detach();
        Ok(())
    }
}
