use super::*;
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, percent_encode};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid location URI: {0}")]
    Invalid(String),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Remote(#[from] gds::service::remote::Error),
    #[error("cannot resolve a location URI: {0}")]
    Io(#[from] std::io::Error),
}

impl Location {
    /// # Errors
    /// Rejects relative filesystem paths and reports failures resolving parent traversal.
    pub(crate) fn uri(&self) -> Result<String, Error> {
        if let Some(uri) = self.virtual_uri() {
            return Ok(uri.to_owned());
        }
        match self {
            Self::Tags(tag) => Ok(tag.uri()),
            Self::Directory(path) => Self::file_uri(Path::new(path)),
            Self::Remote(address) => Ok(address.uri().to_owned()),
            Self::Volume { identity, relative } => {
                let (kind, id) = match identity {
                    VolumeIdentity::Stable(id) => ("stable", id),
                    VolumeIdentity::Attachment(id) => ("attachment", id),
                };
                Ok(format!(
                    "location:/volume/{kind}/{}?path={}",
                    percent_encode(id.as_bytes(), NON_ALPHANUMERIC),
                    percent_encode(relative.as_encoded_bytes(), NON_ALPHANUMERIC),
                ))
            }
            _ => unreachable!("virtual locations have a fixed URI"),
        }
    }

    fn file_uri(path: &Path) -> Result<String, Error> {
        let resolved;
        // URL parsing removes '..' lexically; resolve its parent first so a
        // symlink/../child keeps filesystem meaning without following the leaf.
        let path = if path
            .components()
            .any(|part| part == std::path::Component::ParentDir)
        {
            resolved = match path.file_name() {
                Some(name) => path
                    .parent()
                    .expect("path with parent traversal")
                    .canonicalize()?
                    .join(name),
                None => path.canonicalize()?,
            };
            &resolved
        } else {
            path
        };
        Url::from_file_path(path)
            .map(String::from)
            .map_err(|_| Error::Invalid(path.display().to_string()))
    }

    /// # Errors
    /// Rejects unsupported schemes, malformed addresses and invalid volume-relative paths.
    pub(crate) fn from_uri(input: &str) -> Result<Self, Error> {
        crate::entities::scheme::parse(input)
    }

    pub(crate) fn from_builtin_uri(input: &str) -> Result<Self, Error> {
        if let Some(location) = Self::from_virtual_uri(input) {
            return Ok(location);
        }
        let url = Url::parse(input)?;
        match url.scheme() {
            "file" if url.query().is_none() && url.fragment().is_none() => url
                .to_file_path()
                .map(|path| Self::from(&path))
                .map_err(|_| Error::Invalid(input.to_owned())),
            "sftp" | "smb" => Ok(Self::Remote(gds::service::remote::Address::parse(input)?)),
            "location"
                if url.host_str().is_none()
                    && input
                        .split_once(':')
                        .is_some_and(|(_, path)| path.starts_with("/volume/")) =>
            {
                Self::volume_uri(&url)
            }
            _ => Err(Error::Invalid(input.to_owned())),
        }
    }

    fn volume_uri(url: &Url) -> Result<Self, Error> {
        let invalid = || Error::Invalid(url.to_string());
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
        Ok(Self::Volume { identity, relative })
    }
}

#[cfg(test)]
mod tests;
