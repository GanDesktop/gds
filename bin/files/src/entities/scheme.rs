//! URI schemes own address and navigation semantics, independently of query backends.
use crate::entities::location::{AddressError, Location};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

pub trait Scheme: Sync {
    fn name(&self) -> &'static str;
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
struct RemoteScheme(&'static str);
static LOCATION: LocationScheme = LocationScheme;
static TAGS: TagsScheme = TagsScheme;
static FILE: FileScheme = FileScheme;
static SFTP: RemoteScheme = RemoteScheme("sftp");
static SMB: RemoteScheme = RemoteScheme("smb");

pub fn registry() -> [&'static dyn Scheme; 5] {
    [&LOCATION, &TAGS, &FILE, &SFTP, &SMB]
}

pub fn recognizes(input: &str) -> bool {
    input.split_once(':').is_some_and(|(name, _)| {
        registry()
            .iter()
            .any(|s| s.name().eq_ignore_ascii_case(name))
    })
}

pub fn parse(input: &str) -> Result<Location, AddressError> {
    let name = input.split_once(':').map(|(s, _)| s).unwrap_or_default();
    registry()
        .into_iter()
        .find(|s| s.name().eq_ignore_ascii_case(name))
        .ok_or_else(|| AddressError::Invalid(input.into()))?
        .parse(input)
}

fn provider(address: &Location) -> &'static dyn Scheme {
    match address {
        Location::Tags(_) => &TAGS,
        Location::Directory(_) => &FILE,
        Location::Remote(a) if a.uri().starts_with("sftp:") => &SFTP,
        Location::Remote(_) => &SMB,
        _ => &LOCATION,
    }
}

pub fn parent(address: &Location) -> Option<Location> {
    provider(address).parent(address)
}

pub fn browse(address: &Location) -> Browse<'_> {
    provider(address).browse(address)
}

impl Scheme for LocationScheme {
    fn name(&self) -> &'static str {
        "location"
    }
    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        Location::from_builtin_uri(input)
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
        Location::from_builtin_uri(input)
    }
    fn parent(&self, a: &Location) -> Option<Location> {
        a.local_path()?.parent().map(Location::from)
    }
}
impl Scheme for RemoteScheme {
    fn browse<'a>(&self, address: &'a Location) -> Browse<'a> {
        match address {
            Location::Remote(a) => Browse::Remote(a),
            _ => Browse::Location,
        }
    }
    fn name(&self) -> &'static str {
        self.0
    }
    fn parse(&self, input: &str) -> Result<Location, AddressError> {
        Location::from_builtin_uri(input)
    }
    fn parent(&self, a: &Location) -> Option<Location> {
        if let Location::Remote(a) = a {
            a.parent().map(Location::Remote)
        } else {
            None
        }
    }
}
impl Scheme for TagsScheme {
    fn browse<'a>(&self, address: &'a Location) -> Browse<'a> {
        match address {
            Location::Tags(a) => Browse::Tags(a),
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
    fn parent(&self, a: &Location) -> Option<Location> {
        if let Location::Tags(tag) = a {
            tag.parent().map(Location::Tags)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TagAddress(String);
impl TagAddress {
    pub fn new(tag: &str) -> Result<Self, String> {
        if tag.contains(['\0', '\n', '\r'])
            || (!tag.is_empty() && tag.split('/').any(|s| matches!(s, "" | "." | "..")))
        {
            return Err("Invalid tag path".into());
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
