//! Single-instance registration and desktop command delivery.
use super::{APP_ID, Config};
use gds::service::desktop::file_manager::{Command, Error, Server};
use tokio::sync::mpsc;

#[derive(Default)]
pub(super) struct Desktop {
    pub server: Option<Server>,
    pub receiver: Option<mpsc::Receiver<Command>>,
}

impl Desktop {
    /// None means the request was handled by an existing Files process.
    pub fn start(
        config: &mut Config,
        runtime: &tokio::runtime::Runtime,
    ) -> anyhow::Result<Option<Self>> {
        let mut desktop = Self::default();
        if config.picker.is_some() {
            return Ok(Some(desktop));
        }
        let (sender, receiver) = mpsc::channel(32);
        match runtime.block_on(Server::connect(APP_ID, sender)) {
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
                if !config.dbus_service {
                    let startup_id = std::env::var("XDG_ACTIVATION_TOKEN")
                        .or_else(|_| std::env::var("DESKTOP_STARTUP_ID"))
                        .unwrap_or_default();
                    runtime.block_on(Server::open_existing(APP_ID, &config.start, &startup_id))?;
                }
                return Ok(None);
            }
            Err(error) => {
                tracing::warn!(%error, "file-manager integration unavailable");
                config.report(error.to_string());
            }
        }
        Ok(Some(desktop))
    }
}
