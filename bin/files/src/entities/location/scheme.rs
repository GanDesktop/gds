//! URI schemes own address and navigation semantics, independently of query backends.
use crate::entities::location::{AddressError, Location, VolumeIdentity};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use std::{ffi::OsString, path::Path};
use url::Url;

pub trait Scheme: Sync {
    fn name(&self) -> &'static str;
    /// # Errors
    /// Rejects malformed addresses or URIs that do not belong to this scheme.
    fn parse(&self, input: &str) -> Result<Location, AddressError>;
    fn parent(&self, address: &Location) -> Option<Location>;
    fn browse<'a>(&self, _: &'a Location) -> Browse<'a> {
        Browse::Location
    }
}

/// Navigation chooses a content source; providers implement loading separately.
/// A virtual tag address cannot accidentally enter a filesystem directory read.
pub enum Browse<'a> {
    Location,
    Tags(&'a TagAddress),
    Remote(&'a gds::service::remote::Address),
}

struct LocationScheme;
struct TagsScheme;
struct FileScheme;
struct RemoteScheme {
    name: &'static str,
    protocol: gds::service::remote::Protocol,
}

static LOCATION: LocationScheme = LocationScheme;
static TAGS: TagsScheme = TagsScheme;
static FILE: FileScheme = FileScheme;
static SFTP: RemoteScheme = RemoteScheme {
    name: "sftp",
    protocol: gds::service::remote::Protocol::Sftp,
};
static SMB: RemoteScheme = RemoteScheme {
    name: "smb",
    protocol: gds::service::remote::Protocol::Smb,
};

pub fn registry() -> [&'static dyn Scheme; 5] {
    [&LOCATION, &TAGS, &FILE, &SFTP, &SMB]
}

pub fn recognizes(input: &str) -> bool {
    input.split_once(':').is_some_and(|(name, _)| {
        registry()
            .iter()
            .any(|scheme| scheme.name().eq_ignore_ascii_case(name))
    })
}

/// Parse a URI using the registered scheme provider for its prefix.
///
/// # Errors
/// Rejects unrecognized schemes and invalid scheme-specific URI syntax.
pub fn parse(input: &str) -> Result<Location, AddressError> {
    let name = input.split_once(':').map(|(s, _)| s).unwrap_or_default();
    registry()
        .into_iter()
        .find(|scheme| scheme.name().eq_ignore_ascii_case(name))
        .ok_or_else(|| AddressError::Invalid(input.into()))?
        .parse(input)
}

fn provider(address: &Location) -> &'static dyn Scheme {
    match address {
        Location::Tags(_) => &TAGS,
        Location::Directory(_) => &FILE,
        Location::Remote(remote) => match remote.protocol() {
            gds::service::remote::Protocol::Sftp => &SFTP,
            gds::service::remote::Protocol::Smb => &SMB,
        },
        _ => &LOCATION,
    }
}

pub fn parent(address: &Location) -> Option<Location> {
    provider(address).parent(address)
}

pub fn browse(address: &Location) -> Browse<'_> {
    provider(address).browse(address)
}

impl LocationScheme {
    fn parse_volume(url: &Url) -> Result<Location, AddressError> {
        let invalid = || AddressError::Invalid(url.to_string());
        if !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let mut segments = url.path_segments().ok_or_else(invalid)?;
        if segments.next() != Some("volume") {
            return Err(invalid());
        }
        let kind = segments.next().ok_or_else(invalid)?;
        let id = percent_decode_str(segments.next().ok_or_else(invalid)?)
            .decode_utf8()
            .map_err(|_| invalid())?
            .into_owned();
        if id.is_empty() || segments.next().is_some() {
            return Err(invalid());
        }
        let identity = match kind {
            "stable" => VolumeIdentity::Stable(id),
            "attachment" => VolumeIdentity::Attachment(id),
            _ => return Err(invalid()),
        };
        let path = url
            .query()
            .and_then(|query| query.strip_prefix("path="))
            .ok_or_else(invalid)?;
        #[cfg(unix)]
        let relative = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(percent_decode_str(path).collect())
        };
        #[cfg(not(unix))]
        let relative = OsString::from(
            percent_decode_str(path)
                .decode_utf8()
                .map_err(|_| invalid())?
                .into_owned(),
        );
        if Path::new(&relative).components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        }) {
            return Err(invalid());
        }
        Ok(Location::Volume { identity, relative })
    }
}

impl Scheme for LocationScheme {
    fn name(&self) -> &'static str {
        "location"
    }

    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        if let Some(location) = Location::from_virtual_uri(input) {
            return Ok(location);
        }
        let url = Url::parse(input)?;
        if url.scheme() == "location"
            && url.host_str().is_none()
            && input
                .split_once(':')
                .is_some_and(|(_, path)| path.starts_with("/volume/"))
        {
            Self::parse_volume(&url)
        } else {
            Err(AddressError::Invalid(input.to_owned()))
        }
    }

    fn parent(&self, _: &Location) -> Option<Location> {
        None
    }
}

impl Scheme for FileScheme {
    fn name(&self) -> &'static str {
        "file"
    }

    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        let url = Url::parse(input)?;
        if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
            return Err(AddressError::Invalid(input.to_owned()));
        }
        url.to_file_path()
            .map(|path| Location::from(&path))
            .map_err(|()| AddressError::Invalid(input.to_owned()))
    }

    fn parent(&self, address: &Location) -> Option<Location> {
        address.local_path()?.parent().map(Location::from)
    }
}

impl Scheme for RemoteScheme {
    fn browse<'a>(&self, address: &'a Location) -> Browse<'a> {
        match address {
            Location::Remote(remote) => Browse::Remote(remote),
            _ => Browse::Location,
        }
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        let address = gds::service::remote::Address::parse(input)?;
        if address.protocol() != self.protocol {
            return Err(AddressError::Invalid(input.to_owned()));
        }
        Ok(Location::Remote(address))
    }

    fn parent(&self, address: &Location) -> Option<Location> {
        if let Location::Remote(remote) = address {
            remote.parent().map(Location::Remote)
        } else {
            None
        }
    }
}

impl Scheme for TagsScheme {
    fn browse<'a>(&self, address: &'a Location) -> Browse<'a> {
        match address {
            Location::Tags(tag) => Browse::Tags(tag),
            _ => Browse::Location,
        }
    }

    fn name(&self) -> &'static str {
        "tags"
    }

    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        let (_, path) = input
            .split_once(':')
            .ok_or_else(|| AddressError::Invalid(input.into()))?;
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#']) {
            return Err(AddressError::Invalid(input.into()));
        }
        for (index, byte) in path.bytes().enumerate() {
            if byte == b'%'
                && !path
                    .as_bytes()
                    .get(index + 1..index + 3)
                    .is_some_and(|s| s.iter().all(u8::is_ascii_hexdigit))
            {
                return Err(AddressError::Invalid(input.into()));
            }
        }
        // Decode exactly once; '#' and '?' in names must be percent encoded.
        let decoded = percent_decode_str(&path[1..])
            .decode_utf8()
            .map_err(|_| AddressError::Invalid(input.into()))?;
        let tag = TagAddress::new(&decoded).map_err(|_| AddressError::Invalid(input.into()))?;
        Ok(Location::Tags(tag))
    }

    fn parent(&self, address: &Location) -> Option<Location> {
        if let Location::Tags(tag) = address {
            tag.parent().map(Location::Tags)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TagAddressError {
    #[error("Invalid tag path")]
    InvalidPath,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TagAddress(String);

impl TagAddress {
    /// Validate a hierarchical tag path.
    ///
    /// # Errors
    /// Rejects control characters, empty segments, and `.` / `..` traversal segments.
    pub fn new(tag: &str) -> Result<Self, TagAddressError> {
        if tag.contains(['\0', '\n', '\r'])
            || (!tag.is_empty() && tag.split('/').any(|s| matches!(s, "" | "." | "..")))
        {
            return Err(TagAddressError::InvalidPath);
        }
        Ok(Self(tag.to_owned()))
    }

    pub fn tag(&self) -> &str {
        &self.0
    }

    pub fn uri(&self) -> String {
        format!(
            "tags:/{}",
            self.0
                .split('/')
                .map(|s| utf8_percent_encode(s, NON_ALPHANUMERIC).to_string())
                .collect::<Vec<_>>()
                .join("/")
        )
    }

    /// Human-readable address; preserve escapes that affect parsing or display.
    pub fn display_uri(&self) -> String {
        let mut output = String::from("tags:/");
        for c in self.0.chars() {
            if c.is_ascii() && (!c.is_ascii_graphic() || "%?#\"<>`{}|\\^".contains(c)) {
                output.push_str(&format!("%{:02X}", c as u32));
            } else {
                output.push(c);
            }
        }
        output
    }

    pub fn parent(&self) -> Option<Self> {
        (!self.0.is_empty()).then(|| {
            Self(
                self.0
                    .rsplit_once('/')
                    .map(|(p, _)| p)
                    .unwrap_or_default()
                    .into(),
            )
        })
    }

    pub fn label(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
