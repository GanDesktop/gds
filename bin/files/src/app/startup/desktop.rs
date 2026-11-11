//! Single-instance registration and desktop command delivery.
use super::{APP_ID, Config, Mode};
use crate::app::windows::Windows;
use gds::app::AppExt as _;
use gds::service::desktop::file_manager::{Command, Error, Server};
use tokio::sync::mpsc;

#[derive(Default)]
pub(super) struct DesktopIntegration {
    server: Option<Server>,
    receiver: Option<mpsc::Receiver<Command>>,
}

impl DesktopIntegration {
    /// Process requests only after all application services and windows are ready.
    pub fn listen(
        mut self,
        windows: gpui_kit::Entity<Windows>,
        cx: &mut gpui_kit::App,
    ) -> anyhow::Result<()> {
        let Some(mut receiver) = self.receiver.take() else {
            return Ok(());
        };
        let tasks = cx.gds().tasks().scope();
        let cancel = tasks.token();
        let completion = tasks.register_named(
            "Desktop requests",
            gds::service::task_manager::Execution::Supervisor,
        )?;
        cx.spawn(async move |cx| {
            loop {
                let command = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    command = receiver.recv() => match command { Some(command) => command, None => break },
                };
                windows.update(cx, |windows, cx| windows.desktop_command(command, cx));
            }
            drop(self);
            drop(completion);
        }).detach();
        Ok(())
    }

    /// None means the request was handled by an existing Files process.
    pub async fn start(config: &mut Config) -> anyhow::Result<Option<Self>> {
        let mut desktop = Self::default();
        if config.picker.is_some() {
            return Ok(Some(desktop));
        }
        let (sender, receiver) = mpsc::channel(32);
        match Server::connect(APP_ID, sender).await {
            Ok(server) => {
                if let Some(provider) = &server.provider_conflict {
                    let message = format!(
                        "org.freedesktop.FileManager1 is already provided by {provider}; Files did not take ownership of this interface."
                    );
                    tracing::warn!(%message, "file-manager integration unavailable");
                    config.report(message);
                }
                desktop.receiver = Some(receiver);
                desktop.server = Some(server);
            }
            Err(Error::AlreadyRunning) => {
                if config.mode != Mode::Service {
                    let startup_id = std::env::var("XDG_ACTIVATION_TOKEN")
                        .or_else(|_| std::env::var("DESKTOP_STARTUP_ID"))
                        .unwrap_or_default();
                    Server::open_existing(APP_ID, &config.start, &startup_id).await?;
                }
                return Ok(None);
            }
            Err(error) if config.mode == Mode::Service => return Err(error.into()),
            Err(error) => {
                tracing::warn!(%error, "file-manager integration unavailable");
                config.report(error.to_string());
            }
        }
        Ok(Some(desktop))
    }
}
