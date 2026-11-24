//! Persistent remote credential storage backed by the FreeDesktop Secret Service
//! API (`org.freedesktop.secrets`, and `org.freedesktop.portal.Secret` when
//! sandboxed) via pure-Rust `oo7`.
//!
//! Call from a Tokio runtime. Credentials are indexed by `(protocol, server,
//! port, user)` to match [`Address::connection_key`].

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use super::{Address, Credentials, Protocol};

/// Reverse-DNS application identifier attached to every Secret Service item.
pub const APPLICATION_ID: &str = "io.github.GanDesktop.GdsFiles";

/// `xdg:schema` value identifying remote filesystem credentials stored by GDS Files.
pub const SCHEMA_NAME: &str = "io.github.GanDesktop.GdsFiles.RemoteCredentials";

const ATTR_SCHEMA: &str = "xdg:schema";
const ATTR_APPLICATION: &str = "application";
const ATTR_PROTOCOL: &str = "protocol";
const ATTR_SERVER: &str = "server";
const ATTR_PORT: &str = "port";
const ATTR_USER: &str = "user";
const ATTR_SECRET_KIND: &str = "secret_kind";
const ATTR_PRIVATE_KEY_PATH: &str = "private_key_path";

const SECRET_KIND_PASSWORD: &str = "password";
const SECRET_KIND_PRIVATE_KEY: &str = "private_key";

/// Errors produced when communicating with the desktop Secret Service.
///
/// Error messages never include secret values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No Secret Service daemon (`org.freedesktop.secrets`) or Secret portal is reachable.
    #[error("system keyring service (org.freedesktop.secrets) is unavailable")]
    Unavailable,
    /// The user dismissed a keyring unlock or creation prompt.
    #[error("system keyring prompt was dismissed")]
    Dismissed,
    /// Another backend or validation failure occurred.
    #[error("system keyring error: {0}")]
    Backend(String),
}

struct EncodedItem {
    label: String,
    attributes: BTreeMap<String, String>,
    secret: String,
}

fn protocol_str(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Sftp => "sftp",
        Protocol::Smb => "smb",
    }
}

fn default_port(protocol: Protocol) -> u16 {
    match protocol {
        Protocol::Sftp => 22,
        Protocol::Smb => 445,
    }
}

/// Builds the lookup attributes identifying all credential items belonging to `address`.
#[must_use]
pub(crate) fn lookup_attributes(address: &Address) -> BTreeMap<String, String> {
    BTreeMap::from([
        (ATTR_SCHEMA.to_owned(), SCHEMA_NAME.to_owned()),
        (ATTR_APPLICATION.to_owned(), APPLICATION_ID.to_owned()),
        (
            ATTR_PROTOCOL.to_owned(),
            protocol_str(address.protocol()).to_owned(),
        ),
        (ATTR_SERVER.to_owned(), address.host().to_owned()),
        (ATTR_PORT.to_owned(), address.port().to_string()),
        (ATTR_USER.to_owned(), address.username()),
    ])
}

/// Formats a human-readable Secret Service item label without embedding any secret.
#[must_use]
pub(crate) fn item_label(address: &Address) -> String {
    let protocol = match address.protocol() {
        Protocol::Sftp => "SFTP",
        Protocol::Smb => "SMB",
    };
    let host = if address.host().contains(':') {
        format!("[{}]", address.host())
    } else {
        address.host().to_owned()
    };
    let endpoint = if address.port() == default_port(address.protocol()) {
        host
    } else {
        format!("{host}:{}", address.port())
    };
    let user = address.username();
    if user.is_empty() {
        format!("GDS Files ({protocol}): {endpoint}")
    } else {
        format!("GDS Files ({protocol}): {user}@{endpoint}")
    }
}

fn encode_item(address: &Address, credentials: &Credentials) -> Result<Option<EncodedItem>, Error> {
    let mut attributes = lookup_attributes(address);
    if let Some(key_path) = credentials.private_key.as_ref() {
        if !key_path.is_absolute() {
            return Err(Error::Backend(
                "private key path must be absolute".to_owned(),
            ));
        }
        let Some(key_path) = key_path.to_str() else {
            return Err(Error::Backend(
                "private key path must be valid UTF-8".to_owned(),
            ));
        };
        attributes.insert(
            ATTR_SECRET_KIND.to_owned(),
            SECRET_KIND_PRIVATE_KEY.to_owned(),
        );
        attributes.insert(ATTR_PRIVATE_KEY_PATH.to_owned(), key_path.to_owned());
        return Ok(Some(EncodedItem {
            label: item_label(address),
            attributes,
            secret: credentials.key_passphrase.clone().unwrap_or_default(),
        }));
    }
    if !credentials.password.is_empty() {
        attributes.insert(ATTR_SECRET_KIND.to_owned(), SECRET_KIND_PASSWORD.to_owned());
        return Ok(Some(EncodedItem {
            label: item_label(address),
            attributes,
            secret: credentials.password.clone(),
        }));
    }
    Ok(None)
}

pub(crate) fn decode_credentials(
    attributes: &HashMap<String, String>,
    secret_bytes: &[u8],
) -> Option<Credentials> {
    let secret = std::str::from_utf8(secret_bytes).ok()?;
    match attributes.get(ATTR_SECRET_KIND).map(String::as_str) {
        Some(SECRET_KIND_PASSWORD) => (!secret.is_empty()).then(|| Credentials {
            password: secret.to_owned(),
            private_key: None,
            key_passphrase: None,
        }),
        Some(SECRET_KIND_PRIVATE_KEY) => {
            let key_path = attributes
                .get(ATTR_PRIVATE_KEY_PATH)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())?;
            Some(Credentials {
                password: String::new(),
                private_key: Some(key_path),
                key_passphrase: (!secret.is_empty()).then(|| secret.to_owned()),
            })
        }
        _ => None,
    }
}

/// Checks whether the desktop Secret Service (or sandbox Secret portal) is
/// reachable without triggering any user prompt or collection creation.
pub async fn is_available() -> bool {
    platform::is_available().await
}

/// Checks passively whether at least one saved credential entry exists for
/// `address` without creating a collection or prompting to unlock.
///
/// # Errors
/// Returns an error if the Secret Service exists but fails while searching.
pub async fn has_saved(address: &Address) -> Result<bool, Error> {
    platform::has_saved(address).await
}

/// Looks up stored credentials for `address` in the desktop Secret Service.
///
/// Unlocks a matched locked item via the desktop keyring prompt if necessary.
/// Returns `Ok(None)` when the Secret Service is unavailable, no default
/// collection exists yet, or no matching item is stored.
///
/// # Errors
/// Returns [`Error::Dismissed`] if the user cancels a keyring unlock prompt,
/// or [`Error::Backend`] on other Secret Service failures.
pub async fn lookup(address: &Address) -> Result<Option<Credentials>, Error> {
    platform::lookup(address).await
}

/// Stores or replaces credentials for `address` in the desktop Secret Service.
///
/// Empty credentials (SSH agent or SMB guest) are a no-op returning `Ok(())`.
/// When switching authentication methods (for example, from password to private
/// key), any stale items for the same `(protocol, server, port, user)` are
/// removed only after the new item is stored.
///
/// # Errors
/// Returns [`Error::Unavailable`] when no Secret Service is available,
/// [`Error::Dismissed`] when the user dismisses an unlock or creation prompt,
/// or [`Error::Backend`] on validation or backend failures.
pub async fn store(address: &Address, credentials: &Credentials) -> Result<(), Error> {
    let Some(encoded) = encode_item(address, credentials)? else {
        return Ok(());
    };
    platform::store(address, encoded).await
}

/// Deletes all stored credentials matching `address`.
///
/// Returns `Ok(true)` if at least one matching item was removed, or `Ok(false)`
/// if the Secret Service is unavailable, no default collection exists, or no
/// matching item was found.
///
/// # Errors
/// Returns [`Error::Dismissed`] if the user cancels an unlock prompt, or
/// [`Error::Backend`] on other Secret Service failures.
pub async fn delete(address: &Address) -> Result<bool, Error> {
    platform::delete(address).await
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod platform {
    use std::time::Duration;

    use oo7::{Keyring, Secret, dbus};
    use zbus::names::BusName;

    use super::{EncodedItem, Error, decode_credentials, lookup_attributes};
    use crate::service::remote::{Address, Credentials};

    const SECRET_SERVICE_BUS_NAME: &str = "org.freedesktop.secrets";
    const PORTAL_BUS_NAME: &str = "org.freedesktop.portal.Desktop";

    async fn bus_name_available(name: &'static str) -> bool {
        let Ok(connection) = zbus::Connection::session().await else {
            return false;
        };
        let Ok(dbus_proxy) = zbus::fdo::DBusProxy::new(&connection).await else {
            return false;
        };
        let Ok(bus_name) = BusName::try_from(name) else {
            return false;
        };
        if dbus_proxy.name_has_owner(bus_name).await.unwrap_or(false) {
            return true;
        }
        dbus_proxy
            .list_activatable_names()
            .await
            .is_ok_and(|names| names.iter().any(|entry| entry.as_str() == name))
    }

    fn is_unavailable_zbus(error: &zbus::Error) -> bool {
        match error {
            zbus::Error::InputOutput(_) | zbus::Error::Address(_) => true,
            zbus::Error::FDO(fdo) => matches!(
                fdo.as_ref(),
                zbus::fdo::Error::ServiceUnknown(_)
                    | zbus::fdo::Error::NameHasNoOwner(_)
                    | zbus::fdo::Error::SpawnChildExited(_)
                    | zbus::fdo::Error::SpawnFailed(_)
                    | zbus::fdo::Error::SpawnExecFailed(_)
            ),
            _ => false,
        }
    }

    fn map_dbus_error(error: dbus::Error) -> Error {
        match error {
            dbus::Error::Dismissed => Error::Dismissed,
            dbus::Error::ZBus(err) if is_unavailable_zbus(&err) => Error::Unavailable,
            dbus::Error::Service(dbus::ServiceError::ZBus(err)) if is_unavailable_zbus(&err) => {
                Error::Unavailable
            }
            other => Error::Backend(other.to_string()),
        }
    }

    fn map_keyring_error(error: oo7::Error) -> Error {
        match error {
            oo7::Error::DBus(err) => map_dbus_error(err),
            oo7::Error::File(oo7::file::Error::Portal(oo7::ashpd::Error::PortalNotFound(_))) => {
                Error::Unavailable
            }
            oo7::Error::File(oo7::file::Error::Portal(oo7::ashpd::Error::Response(
                oo7::ashpd::desktop::ResponseError::Cancelled,
            ))) => Error::Dismissed,
            other => Error::Backend(other.to_string()),
        }
    }

    pub(super) async fn is_available() -> bool {
        if oo7::ashpd::is_sandboxed() && bus_name_available(PORTAL_BUS_NAME).await {
            return true;
        }
        bus_name_available(SECRET_SERVICE_BUS_NAME).await
    }

    pub(super) async fn has_saved(address: &Address) -> Result<bool, Error> {
        let attrs = lookup_attributes(address);
        if oo7::ashpd::is_sandboxed() {
            if !is_available().await {
                return Ok(false);
            }
            let keyring = match Keyring::new().await {
                Ok(keyring) => keyring,
                Err(err) => match map_keyring_error(err) {
                    Error::Unavailable | Error::Dismissed => return Ok(false),
                    other => return Err(other),
                },
            };
            if keyring.is_locked().await.map_err(map_keyring_error)? {
                return Ok(false);
            }
            let items = keyring
                .search_items(&attrs)
                .await
                .map_err(map_keyring_error)?;
            return Ok(!items.is_empty());
        }

        if !bus_name_available(SECRET_SERVICE_BUS_NAME).await {
            return Ok(false);
        }
        let service = match dbus::Service::new().await {
            Ok(service) => service,
            Err(err) => match map_dbus_error(err) {
                Error::Unavailable => return Ok(false),
                other => return Err(other),
            },
        };
        let Some(collection) = service
            .with_alias(dbus::Service::DEFAULT_COLLECTION)
            .await
            .map_err(map_dbus_error)?
        else {
            return Ok(false);
        };
        let items = collection
            .search_items(&attrs)
            .await
            .map_err(map_dbus_error)?;
        Ok(!items.is_empty())
    }

    pub(super) async fn lookup(address: &Address) -> Result<Option<Credentials>, Error> {
        let attrs = lookup_attributes(address);
        if oo7::ashpd::is_sandboxed() {
            if !is_available().await {
                return Ok(None);
            }
            let keyring = match Keyring::new().await {
                Ok(keyring) => keyring,
                Err(err) => match map_keyring_error(err) {
                    Error::Unavailable => return Ok(None),
                    other => return Err(other),
                },
            };
            if keyring.is_locked().await.map_err(map_keyring_error)? {
                keyring.unlock().await.map_err(map_keyring_error)?;
            }
            let items = keyring
                .search_items(&attrs)
                .await
                .map_err(map_keyring_error)?;
            for item in items {
                if item.is_locked().await.map_err(map_keyring_error)? {
                    item.unlock().await.map_err(map_keyring_error)?;
                }
                let item_attrs = item.attributes().await.map_err(map_keyring_error)?;
                let secret = item.secret().await.map_err(map_keyring_error)?;
                if let Some(credentials) = decode_credentials(&item_attrs, secret.as_bytes()) {
                    return Ok(Some(credentials));
                }
            }
            return Ok(None);
        }

        if !bus_name_available(SECRET_SERVICE_BUS_NAME).await {
            return Ok(None);
        }
        let service = match dbus::Service::new().await {
            Ok(service) => service,
            Err(err) => match map_dbus_error(err) {
                Error::Unavailable => return Ok(None),
                other => return Err(other),
            },
        };
        let Some(collection) = service
            .with_alias(dbus::Service::DEFAULT_COLLECTION)
            .await
            .map_err(map_dbus_error)?
        else {
            return Ok(None);
        };
        let items = collection
            .search_items(&attrs)
            .await
            .map_err(map_dbus_error)?;
        if items.is_empty() {
            return Ok(None);
        }

        let mut ranked = Vec::with_capacity(items.len());
        for item in items {
            let modified = item.modified().await.unwrap_or(Duration::ZERO);
            ranked.push((modified, item));
        }
        ranked.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));

        for (_, item) in ranked {
            if item.is_locked().await.map_err(map_dbus_error)? {
                item.unlock(None).await.map_err(map_dbus_error)?;
                if item.is_locked().await.map_err(map_dbus_error)? {
                    return Err(Error::Dismissed);
                }
            }
            let item_attrs = item.attributes().await.map_err(map_dbus_error)?;
            let secret = item.secret().await.map_err(map_dbus_error)?;
            if let Some(credentials) = decode_credentials(&item_attrs, secret.as_bytes()) {
                return Ok(Some(credentials));
            }
        }
        Ok(None)
    }

    pub(super) async fn store(address: &Address, encoded: EncodedItem) -> Result<(), Error> {
        let base_attrs = lookup_attributes(address);
        if oo7::ashpd::is_sandboxed() {
            if !is_available().await {
                return Err(Error::Unavailable);
            }
            let keyring = Keyring::new().await.map_err(map_keyring_error)?;
            if keyring.is_locked().await.map_err(map_keyring_error)? {
                keyring.unlock().await.map_err(map_keyring_error)?;
            }
            let existing = keyring
                .search_items(&base_attrs)
                .await
                .map_err(map_keyring_error)?;
            keyring
                .create_item(
                    &encoded.label,
                    &encoded.attributes,
                    Secret::text(&encoded.secret),
                    true,
                )
                .await
                .map_err(map_keyring_error)?;
            for item in existing {
                if let Ok(item_attrs) = item.attributes().await
                    && !attributes_match(&item_attrs, &encoded.attributes)
                {
                    let _ = item.delete().await;
                }
            }
            return Ok(());
        }

        if !bus_name_available(SECRET_SERVICE_BUS_NAME).await {
            return Err(Error::Unavailable);
        }
        let service = dbus::Service::new().await.map_err(map_dbus_error)?;
        let collection = service.default_collection().await.map_err(map_dbus_error)?;
        if collection.is_locked().await.map_err(map_dbus_error)? {
            collection.unlock(None).await.map_err(map_dbus_error)?;
            if collection.is_locked().await.map_err(map_dbus_error)? {
                return Err(Error::Dismissed);
            }
        }
        let created = collection
            .create_item(
                &encoded.label,
                &encoded.attributes,
                Secret::text(&encoded.secret),
                true,
                None,
            )
            .await
            .map_err(map_dbus_error)?;

        if let Ok(existing) = collection.search_items(&base_attrs).await {
            for item in existing {
                if item.path() != created.path()
                    && let Err(err) = item.delete(None).await
                {
                    tracing::debug!("failed to clean up superseded keyring item: {err}");
                }
            }
        }
        Ok(())
    }

    pub(super) async fn delete(address: &Address) -> Result<bool, Error> {
        let base_attrs = lookup_attributes(address);
        if oo7::ashpd::is_sandboxed() {
            if !is_available().await {
                return Ok(false);
            }
            let keyring = match Keyring::new().await {
                Ok(keyring) => keyring,
                Err(err) => match map_keyring_error(err) {
                    Error::Unavailable => return Ok(false),
                    other => return Err(other),
                },
            };
            if keyring.is_locked().await.map_err(map_keyring_error)? {
                keyring.unlock().await.map_err(map_keyring_error)?;
            }
            let items = keyring
                .search_items(&base_attrs)
                .await
                .map_err(map_keyring_error)?;
            if items.is_empty() {
                return Ok(false);
            }
            for item in items {
                item.delete().await.map_err(map_keyring_error)?;
            }
            return Ok(true);
        }

        if !bus_name_available(SECRET_SERVICE_BUS_NAME).await {
            return Ok(false);
        }
        let service = match dbus::Service::new().await {
            Ok(service) => service,
            Err(err) => match map_dbus_error(err) {
                Error::Unavailable => return Ok(false),
                other => return Err(other),
            },
        };
        let Some(collection) = service
            .with_alias(dbus::Service::DEFAULT_COLLECTION)
            .await
            .map_err(map_dbus_error)?
        else {
            return Ok(false);
        };
        if collection.is_locked().await.map_err(map_dbus_error)? {
            collection.unlock(None).await.map_err(map_dbus_error)?;
            if collection.is_locked().await.map_err(map_dbus_error)? {
                return Err(Error::Dismissed);
            }
        }
        let items = collection
            .search_items(&base_attrs)
            .await
            .map_err(map_dbus_error)?;
        if items.is_empty() {
            return Ok(false);
        }
        for item in items {
            item.delete(None).await.map_err(map_dbus_error)?;
        }
        Ok(true)
    }

    fn attributes_match(
        existing: &std::collections::HashMap<String, String>,
        expected: &std::collections::BTreeMap<String, String>,
    ) -> bool {
        expected
            .iter()
            .all(|(key, value)| existing.get(key) == Some(value))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod platform {
    use super::{EncodedItem, Error};
    use crate::service::remote::{Address, Credentials};

    pub(super) async fn is_available() -> bool {
        false
    }

    pub(super) async fn has_saved(_address: &Address) -> Result<bool, Error> {
        Ok(false)
    }

    pub(super) async fn lookup(_address: &Address) -> Result<Option<Credentials>, Error> {
        Ok(None)
    }

    pub(super) async fn store(_address: &Address, _encoded: EncodedItem) -> Result<(), Error> {
        Err(Error::Unavailable)
    }

    pub(super) async fn delete(_address: &Address) -> Result<bool, Error> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
