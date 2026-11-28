//! Standard file-manager and desktop-application requests, independent of the graphical toolkit.
use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;
use zbus::{
    Connection,
    fdo::{RequestNameFlags, RequestNameReply},
    names::WellKnownName,
    zvariant::{ObjectPath, OwnedValue, Value},
};

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";
pub const LAUNCHER_INTERFACE: &str = "io.github.GanDesktop.Files";
pub const APPLICATION_INTERFACE: &str = "org.freedesktop.Application";

/// Convert a D-Bus well-known application name into its FreeDesktop `org.freedesktop.Application`
/// object path (Desktop Entry Specification 1.5, D-Bus Activation).
///
/// # Errors
/// Reports `Error::InvalidBusName` if `app_bus_name` is empty or not a valid D-Bus well-known name.
pub fn application_object_path(app_bus_name: &str) -> Result<String, Error> {
    WellKnownName::try_from(app_bus_name)
        .map_err(|_| Error::InvalidBusName(app_bus_name.to_owned()))?;
    // Per Desktop Entry Spec 1.5: dots become slashes with a leading slash, and hyphens become
    // underscores so every path element remains valid in D-Bus object paths.
    let path = format!("/{}", app_bus_name.replace('.', "/").replace('-', "_"));
    ObjectPath::try_from(path.as_str())
        .map_err(|_| Error::InvalidBusName(app_bus_name.to_owned()))?;
    Ok(path)
}

/// Desktop requests carry the paths appropriate to each operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Activate the application, optionally forcing a new window.
    Activate { new_window: bool },
    /// Open one directory in a new window.
    NewWindow { directory: PathBuf },
    /// Open one or more target URIs (`file://`, `trash:///`, `sftp://`, etc.), optionally forcing a new window.
    OpenLocations { uris: Vec<String>, new_window: bool },
    /// Open one or more folder URIs (`file://`, `trash:///`, `smb://`, `sftp://`, etc.),
    /// reusing a window when possible.
    ShowFolders { uris: Vec<String> },
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
    #[error("invalid D-Bus application name: {0}")]
    InvalidBusName(String),
    #[error("unsupported or invalid file-manager URI: {0}")]
    InvalidUri(String),
}

fn invalid_uri(uri: &str) -> zbus::fdo::Error {
    zbus::fdo::Error::InvalidArgs(Error::InvalidUri(uri.into()).to_string())
}

fn local_path(uri: &str) -> zbus::fdo::Result<PathBuf> {
    local_path_with_host(uri, super::recent_files::local_hostname())
}

fn local_path_with_host(uri: &str, local_hostname: Option<&str>) -> zbus::fdo::Result<PathBuf> {
    super::recent_files::parse_strict_local_file_url_with_host(uri, local_hostname)
        .ok_or_else(|| invalid_uri(uri))
}

fn local_paths(uris: &[String]) -> zbus::fdo::Result<Vec<PathBuf>> {
    uris.iter().map(|uri| local_path(uri)).collect()
}

fn folder_uri_with_host(uri: &str, local_hostname: Option<&str>) -> zbus::fdo::Result<String> {
    if uri.is_empty() || uri.as_bytes().contains(&0) {
        return Err(invalid_uri(uri));
    }
    let parsed = url::Url::parse(uri).map_err(|_| invalid_uri(uri))?;
    match parsed.scheme() {
        "file" => {
            let _ = local_path_with_host(uri, local_hostname)?;
            Ok(uri.to_owned())
        }
        "smb" | "sftp" => {
            if parsed.host_str().is_some_and(|host| !host.is_empty()) {
                Ok(uri.to_owned())
            } else {
                Err(invalid_uri(uri))
            }
        }
        "trash" | "recent" | "recentlyused" | "network" | "remote" | "computer" | "starred"
        | "tags" | "tag" | "location" => {
            if parsed.host_str().is_none_or(str::is_empty) {
                Ok(uri.to_owned())
            } else {
                Err(invalid_uri(uri))
            }
        }
        _ => Err(invalid_uri(uri)),
    }
}

fn folder_uris_with_host(
    uris: &[String],
    local_hostname: Option<&str>,
) -> zbus::fdo::Result<Vec<String>> {
    uris.iter()
        .map(|uri| folder_uri_with_host(uri, local_hostname))
        .collect()
}

fn folder_uris(uris: &[String]) -> zbus::fdo::Result<Vec<String>> {
    folder_uris_with_host(uris, super::recent_files::local_hostname())
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
                uris: folder_uris(&uris)?,
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

    async fn open_locations(
        &self,
        uris: Vec<String>,
        new_window: bool,
        startup_id: String,
    ) -> zbus::fdo::Result<()> {
        if uris.is_empty() {
            return Err(zbus::fdo::Error::InvalidArgs(
                "at least one URI is required".into(),
            ));
        }
        dispatch(
            &self.sender,
            Request::OpenLocations { uris, new_window },
            startup_id,
        )
        .await
    }
}

fn platform_string(platform_data: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    match platform_data.get(key).map(Deref::deref) {
        Some(Value::Str(token)) if !token.is_empty() => Some(token.as_str().to_owned()),
        _ => None,
    }
}

fn platform_activation_token(platform_data: &HashMap<String, OwnedValue>) -> String {
    platform_string(platform_data, "activation-token")
        .or_else(|| platform_string(platform_data, "desktop-startup-id"))
        .unwrap_or_default()
}

struct Application {
    sender: mpsc::Sender<Command>,
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl Application {
    async fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> zbus::fdo::Result<()> {
        let startup_id = platform_activation_token(&platform_data);
        dispatch(
            &self.sender,
            Request::Activate { new_window: false },
            startup_id,
        )
        .await
    }

    async fn open(
        &self,
        uris: Vec<String>,
        platform_data: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        let startup_id = platform_activation_token(&platform_data);
        let request = if uris.is_empty() {
            Request::Activate { new_window: false }
        } else {
            Request::OpenLocations {
                uris: folder_uris(&uris)?,
                new_window: false,
            }
        };
        dispatch(&self.sender, request, startup_id).await
    }

    async fn activate_action(
        &self,
        action_name: String,
        _parameter: Vec<OwnedValue>,
        platform_data: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        let startup_id = platform_activation_token(&platform_data);
        match action_name.as_str() {
            "new-window" => {
                dispatch(
                    &self.sender,
                    Request::Activate { new_window: true },
                    startup_id,
                )
                .await
            }
            _ => Err(zbus::fdo::Error::InvalidArgs(format!(
                "unknown application action: {action_name}"
            ))),
        }
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
    /// Reports invalid application bus names, another application instance, or an unavailable
    /// session bus.
    pub async fn claim_app(
        app_bus_name: &str,
        sender: mpsc::Sender<Command>,
    ) -> Result<Self, Error> {
        let app_path = application_object_path(app_bus_name)?;
        let connection = zbus::connection::Builder::session()?
            .serve_at(
                app_path,
                Application {
                    sender: sender.clone(),
                },
            )?
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

    /// Forward one or more target URIs to the running Files instance, optionally opening a new window.
    ///
    /// # Errors
    /// Reports empty URI lists or failures contacting the running Files process.
    pub async fn open_existing_locations(
        app_bus_name: &str,
        uris: &[String],
        new_window: bool,
        startup_id: &str,
    ) -> Result<(), Error> {
        if uris.is_empty() {
            return Err(Error::InvalidUri(String::new()));
        }
        let connection = Connection::session().await?;
        connection
            .call_method(
                Some(app_bus_name),
                OBJECT_PATH,
                Some(LAUNCHER_INTERFACE),
                "OpenLocations",
                &(uris, new_window, startup_id),
            )
            .await?;
        Ok(())
    }
}

fn encode_show_item_uris(paths: &[impl AsRef<Path>]) -> Result<Vec<String>, Error> {
    let mut uris = Vec::with_capacity(paths.len());
    for path in paths {
        let raw = path.as_ref();
        let abs =
            std::path::absolute(raw).map_err(|_| Error::InvalidUri(raw.display().to_string()))?;
        let uri = url::Url::from_file_path(&abs)
            .map_err(|_| Error::InvalidUri(raw.display().to_string()))?;
        uris.push(uri.to_string());
    }
    Ok(uris)
}

/// Reveal and select local items in whichever desktop file manager owns `org.freedesktop.FileManager1`.
///
/// # Errors
/// Reports invalid local paths or D-Bus session call failures.
pub async fn show_items(paths: &[impl AsRef<Path>], startup_id: &str) -> Result<(), Error> {
    if paths.is_empty() {
        return Ok(());
    }
    let uris = encode_show_item_uris(paths)?;
    let connection = Connection::session().await?;
    connection
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(BUS_NAME),
            "ShowItems",
            &(uris, startup_id),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
