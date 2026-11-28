//! URI schemes own address and navigation semantics, independently of query backends.
pub(crate) use crate::entities::location::model::address::{TagAddress, TagAddressError};
use crate::entities::location::{AddressError, Location, VolumeIdentity};
use percent_encoding::percent_decode_str;
use std::{ffi::OsString, path::Path};
use url::Url;

pub trait Scheme: Sync {
    fn name(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }
    fn matches(&self, name: &str) -> bool {
        self.name().eq_ignore_ascii_case(name)
            || self
                .aliases()
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    }
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
    Tags,
    Remote(&'a gds::service::remote::Address),
}

struct LocationScheme;
struct RootVirtualScheme {
    name: &'static str,
    aliases: &'static [&'static str],
    make: fn() -> Location,
}
struct TagsScheme;
struct FileScheme;
struct RemoteScheme {
    name: &'static str,
    protocol: gds::service::remote::Protocol,
}

static LOCATION: LocationScheme = LocationScheme;
static TRASH: RootVirtualScheme = RootVirtualScheme {
    name: "trash",
    aliases: &[],
    make: || Location::Trash,
};
static RECENT: RootVirtualScheme = RootVirtualScheme {
    name: "recent",
    aliases: &["recentlyused"],
    make: || Location::Recent,
};
static NETWORK: RootVirtualScheme = RootVirtualScheme {
    name: "network",
    aliases: &["remote"],
    make: || Location::Network,
};
static COMPUTER: RootVirtualScheme = RootVirtualScheme {
    name: "computer",
    aliases: &[],
    make: || Location::Volumes,
};
static STARRED: RootVirtualScheme = RootVirtualScheme {
    name: "starred",
    aliases: &[],
    make: || Location::Favorites,
};
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

pub fn registry() -> [&'static dyn Scheme; 10] {
    [
        &LOCATION, &TRASH, &RECENT, &NETWORK, &COMPUTER, &STARRED, &TAGS, &FILE, &SFTP, &SMB,
    ]
}

pub fn recognizes(input: &str) -> bool {
    input
        .split_once(':')
        .is_some_and(|(name, _)| registry().iter().any(|scheme| scheme.matches(name)))
}

/// Parse a URI using the registered scheme provider for its prefix.
///
/// # Errors
/// Rejects unrecognized schemes and invalid scheme-specific URI syntax.
pub fn parse(input: &str) -> Result<Location, AddressError> {
    let name = input.split_once(':').map(|(s, _)| s).unwrap_or_default();
    registry()
        .into_iter()
        .find(|scheme| scheme.matches(name))
        .ok_or_else(|| AddressError::Invalid(input.into()))?
        .parse(input)
}

fn provider(address: &Location) -> &'static dyn Scheme {
    match address {
        Location::Trash => &TRASH,
        Location::Recent => &RECENT,
        Location::Network => &NETWORK,
        Location::Tags(_) => &TAGS,
        Location::Directory(_) => &FILE,
        Location::Remote(remote) => match remote.protocol() {
            gds::service::remote::Protocol::Sftp => &SFTP,
            gds::service::remote::Protocol::Smb => &SMB,
        },
        Location::Favorites | Location::Volumes | Location::Volume { .. } => &LOCATION,
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

impl Scheme for RootVirtualScheme {
    fn name(&self) -> &'static str {
        self.name
    }

    fn aliases(&self) -> &'static [&'static str] {
        self.aliases
    }

    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        let (scheme, rest) = input
            .split_once(':')
            .ok_or_else(|| AddressError::Invalid(input.into()))?;
        if !self.matches(scheme) {
            return Err(AddressError::Invalid(input.into()));
        }
        // Accept both RFC 3986 path-absolute (`scheme:/`) and empty-authority root (`scheme:///`),
        // while rejecting non-empty authorities (`scheme://host`) and subpaths/queries/fragments.
        match rest {
            "/" | "///" => Ok((self.make)()),
            _ => Err(AddressError::Invalid(input.into())),
        }
    }

    fn parent(&self, _: &Location) -> Option<Location> {
        None
    }
}

impl FileScheme {
    fn parse_with_host(
        input: &str,
        local_hostname: Option<&str>,
    ) -> Result<Location, AddressError> {
        gds::service::desktop::recent_files::parse_strict_local_file_url_with_host(
            input,
            local_hostname,
        )
        .map(|path| Location::from(&path))
        .ok_or_else(|| AddressError::Invalid(input.to_owned()))
    }
}

impl Scheme for FileScheme {
    fn name(&self) -> &'static str {
        "file"
    }

    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        Self::parse_with_host(input, gds::service::desktop::recent_files::local_hostname())
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
            Location::Tags(_) => Browse::Tags,
            _ => Browse::Location,
        }
    }

    fn name(&self) -> &'static str {
        "tags"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["tag"]
    }

    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        let (_, raw_path) = input
            .split_once(':')
            .ok_or_else(|| AddressError::Invalid(input.into()))?;
        // Accept empty-authority `tags:///...` alongside canonical `tags:/...`, while still
        // rejecting non-empty authorities like `tags://host/path`.
        let path = raw_path
            .strip_prefix("//")
            .filter(|rest| rest.starts_with('/'))
            .unwrap_or(raw_path);
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

#[cfg(test)]
mod tests;
