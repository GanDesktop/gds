//! Standard file-manager requests, independent of the graphical toolkit.
use std::path::PathBuf;
use tokio::sync::mpsc;
use zbus::{
    Connection,
    fdo::{RequestNameFlags, RequestNameReply},
};

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";
pub const LAUNCHER_INTERFACE: &str = "io.github.GanDesktop.Files";

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

async fn dispatch(
    sender: &mpsc::Sender<Command>,
    request: Request,
    startup_id: String,
) -> zbus::fdo::Result<()> {
    sender
        .send(Command {
            request,
            startup_id,
        })
        .await
        .map_err(|_| zbus::fdo::Error::Failed("file manager is closing".into()))
}

struct Interface {
    sender: mpsc::Sender<Command>,
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl Interface {
    async fn show_folders(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        dispatch(
            &self.sender,
            Request::ShowFolders {
                directories: local_paths(&uris)?,
            },
            startup_id,
        )
        .await
    }
    async fn show_items(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        dispatch(
            &self.sender,
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
        dispatch(
            &self.sender,
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
        dispatch(
            &self.sender,
            Request::NewWindow {
                directory: local_path(&uri)?,
            },
            startup_id,
        )
        .await
    }
}

async fn resolve_provider_conflict<Req, ReqFut, Own, OwnFut>(
    mut request_name: Req,
    mut get_owner: Own,
) -> Result<Option<String>, Error>
where
    Req: FnMut() -> ReqFut,
    ReqFut: Future<Output = zbus::Result<RequestNameReply>>,
    Own: FnMut() -> OwnFut,
    OwnFut: Future<Output = zbus::fdo::Result<String>>,
{
    let mut retried = false;
    loop {
        match request_name().await {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                return Ok(None);
            }
            Ok(_) | Err(zbus::Error::NameTaken) => match get_owner().await {
                Ok(owner) => return Ok(Some(owner)),
                Err(zbus::fdo::Error::NameHasNoOwner(_)) if !retried => {
                    retried = true;
                }
                Err(error) => return Err(zbus::Error::from(error).into()),
            },
            Err(error) => return Err(error.into()),
        }
    }
}

pub struct Server {
    connection: Connection,
    pub provider_conflict: Option<String>,
}

impl Server {
    /// Claim the application's own single-instance bus name without yet registering `FileManager1`.
    ///
    /// # Errors
    /// Reports another application instance or an unavailable session bus.
    pub async fn claim_app(
        app_bus_name: &str,
        sender: mpsc::Sender<Command>,
    ) -> Result<Self, Error> {
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
        Ok(Self {
            connection,
            provider_conflict: None,
        })
    }

    /// Claim `org.freedesktop.FileManager1` on the established session bus connection.
    ///
    /// Returns `Ok(Some(owner))` when another provider already owns the well-known name.
    ///
    /// # Errors
    /// Reports session-bus query failures while negotiating ownership.
    pub async fn claim_file_manager(&mut self) -> Result<Option<String>, Error> {
        let conn = &self.connection;
        let conflict = resolve_provider_conflict(
            || conn.request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into()),
            || async move {
                let dbus = zbus::fdo::DBusProxy::new(conn).await?;
                let owner = dbus
                    .get_name_owner(BUS_NAME.try_into().expect("standard bus name"))
                    .await?;
                Ok(owner.to_string())
            },
        )
        .await?;
        self.provider_conflict.clone_from(&conflict);
        Ok(conflict)
    }

    /// # Errors
    /// Reports another application instance or an unavailable session bus.
    pub async fn connect(app_bus_name: &str, sender: mpsc::Sender<Command>) -> Result<Self, Error> {
        let mut server = Self::claim_app(app_bus_name, sender).await?;
        server.claim_file_manager().await?;
        Ok(server)
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
                Some(LAUNCHER_INTERFACE),
                "OpenWindow",
                &(uri.as_str(), startup_id),
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
