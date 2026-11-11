//! Standard file-manager requests, independent of the graphical toolkit.
use std::path::PathBuf;
use tokio::sync::mpsc;
use zbus::{
    Connection,
    fdo::{RequestNameFlags, RequestNameReply},
};

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

/// Desktop requests carry the paths appropriate to each operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Open one directory in a new window.
    NewWindow { directory: PathBuf },
    /// Open the directories themselves, reusing a window when possible.
    ShowFolders { directories: Vec<PathBuf> },
    /// Reveal and select these files or folders in their parent directories.
    ShowItems { paths: Vec<PathBuf> },
    /// Show properties for these files or folders.
    ShowProperties { paths: Vec<PathBuf> },
}

#[derive(Clone, Debug)]
pub struct Command {
    pub request: Request,
    pub startup_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the file-manager application is already running")]
    AlreadyRunning,
    #[error("file-manager bus operation failed: {0}")]
    Bus(#[from] zbus::Error),
    #[error("unsupported or invalid local file URI: {0}")]
    InvalidUri(String),
}

fn local_path(uri: &str) -> zbus::fdo::Result<PathBuf> {
    url::Url::parse(uri)
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
        .filter(|path| path.is_absolute())
        .ok_or_else(|| zbus::fdo::Error::InvalidArgs(Error::InvalidUri(uri.into()).to_string()))
}

fn local_paths(uris: &[String]) -> zbus::fdo::Result<Vec<PathBuf>> {
    uris.iter().map(|uri| local_path(uri)).collect()
}

struct Interface {
    sender: mpsc::Sender<Command>,
}

impl Interface {
    async fn dispatch(&self, request: Request, startup_id: String) -> zbus::fdo::Result<()> {
        self.sender
            .send(Command {
                request,
                startup_id,
            })
            .await
            .map_err(|_| zbus::fdo::Error::Failed("Files is closing".into()))
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl Interface {
    async fn show_folders(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        self.dispatch(
            Request::ShowFolders {
                directories: local_paths(&uris)?,
            },
            startup_id,
        )
        .await
    }
    async fn show_items(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        self.dispatch(
            Request::ShowItems {
                paths: local_paths(&uris)?,
            },
            startup_id,
        )
        .await
    }
    async fn show_item_properties(
        &self,
        uris: Vec<String>,
        startup_id: String,
    ) -> zbus::fdo::Result<()> {
        self.dispatch(
            Request::ShowProperties {
                paths: local_paths(&uris)?,
            },
            startup_id,
        )
        .await
    }
}

struct Launcher {
    sender: mpsc::Sender<Command>,
}

#[zbus::interface(name = "io.github.GanDesktop.Files")]
impl Launcher {
    async fn open_window(&self, uri: String, startup_id: String) -> zbus::fdo::Result<()> {
        let command = Command {
            request: Request::NewWindow {
                directory: local_path(&uri)?,
            },
            startup_id,
        };
        self.sender
            .send(command)
            .await
            .map_err(|_| zbus::fdo::Error::Failed("Files is closing".into()))
    }
}

pub struct Server {
    _connection: Connection,
    pub provider_conflict: Option<String>,
}

impl Server {
    /// # Errors
    /// Reports another application instance or an unavailable session bus.
    pub async fn connect(app_bus_name: &str, sender: mpsc::Sender<Command>) -> Result<Self, Error> {
        let connection = zbus::connection::Builder::session()?
            .serve_at(
                OBJECT_PATH,
                Launcher {
                    sender: sender.clone(),
                },
            )?
            .serve_at(OBJECT_PATH, Interface { sender })?
            .build()
            .await?;
        match connection
            .request_name_with_flags(app_bus_name, RequestNameFlags::DoNotQueue.into())
            .await
        {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => (),
            Ok(_) | Err(zbus::Error::NameTaken) => return Err(Error::AlreadyRunning),
            Err(error) => return Err(error.into()),
        }
        let provider_conflict = match connection
            .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
            .await
        {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => None,
            Ok(_) | Err(zbus::Error::NameTaken) => {
                let dbus = zbus::fdo::DBusProxy::new(&connection).await?;
                Some(
                    dbus.get_name_owner(BUS_NAME.try_into().expect("standard bus name"))
                        .await
                        .map_err(zbus::Error::from)?
                        .to_string(),
                )
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            _connection: connection,
            provider_conflict,
        })
    }

    /// Forward to the application's own bus name without taking over another provider.
    ///
    /// # Errors
    /// Reports invalid paths or failures contacting the running Files process.
    pub async fn open_existing(
        app_bus_name: &str,
        path: &std::path::Path,
        startup_id: &str,
    ) -> Result<(), Error> {
        let uri = url::Url::from_file_path(path)
            .map_err(|_| Error::InvalidUri(path.display().to_string()))?;
        let connection = Connection::session().await?;
        connection
            .call_method(
                Some(app_bus_name),
                OBJECT_PATH,
                Some("io.github.GanDesktop.Files"),
                "OpenWindow",
                &(uri.as_str(), startup_id),
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
