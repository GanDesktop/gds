//! Single-instance D-Bus negotiation and `org.freedesktop.FileManager1` command dispatch.

use gds::service::desktop::file_manager::{Command, Error, Server};
use gds::service::task_manager::Scope;
use tokio::sync::mpsc;

use super::{APP_ID, config::Mode, windows};

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
        match Server::claim_app(APP_ID, sender).await {
            Ok(server) => Ok(Acquire::Continue(Some(Self { server, receiver }))),
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

    /// Serve D-Bus on Tokio; UI delivery remains owned by the application root.
    pub fn listen(self, tasks: &Scope, windows: windows::Host) {
        enum Delivery {
            Command(Command),
            Notice(String),
        }

        let (ui_tx, ui_rx) = mpsc::channel(32);
        zgui::tokio::spawn_receiver(ui_rx, move |message| match message {
            Delivery::Command(command) => windows.dispatch_desktop(command),
            Delivery::Notice(message) => windows.report_notice(message),
        });

        let Self {
            mut server,
            mut receiver,
        } = self;
        let cancel = tasks.token();
        let watcher = tasks.spawn_named("Desktop requests", async move {
            let claim = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                claim = server.claim_file_manager() => claim,
            };
            match claim {
                Ok(Some(provider)) => {
                    let _ = ui_tx
                        .send(Delivery::Notice(format!(
                            "org.freedesktop.FileManager1 is already provided by {provider}; Files did not take ownership of this interface."
                        )))
                        .await;
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = ui_tx.send(Delivery::Notice(error.to_string())).await;
                }
            }
            loop {
                enum Event {
                    Cancelled,
                    Command(Option<Command>),
                }
                let event = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Event::Cancelled,
                    command = receiver.recv() => Event::Command(command),
                };
                match event {
                    Event::Cancelled | Event::Command(None) => break,
                    Event::Command(Some(command)) => {
                        if ui_tx.send(Delivery::Command(command)).await.is_err() {
                            break;
                        }
                    }
                }
            }
            drop(server);
        });
        zgui::task::spawn_local(async move {
            if let Err(error) = watcher.await {
                tracing::error!(%error, "desktop request listener stopped");
            }
        });
    }
}
