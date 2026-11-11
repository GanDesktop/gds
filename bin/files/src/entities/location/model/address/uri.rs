use super::*;
use percent_encoding::{NON_ALPHANUMERIC, percent_encode};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid location URI: {0}")]
    Invalid(String),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Remote(#[from] gds::service::remote::Error),
}

impl Location {
    /// Format a lexical URI for this location without touching the filesystem.
    ///
    /// # Errors
    /// Rejects relative filesystem paths that cannot be represented as a `file://` URL.
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
        Url::from_file_path(path)
            .map(String::from)
            .map_err(|()| Error::Invalid(path.display().to_string()))
    }

    /// # Errors
    /// Rejects unsupported schemes, malformed addresses and invalid volume-relative paths.
    pub(crate) fn from_uri(input: &str) -> Result<Self, Error> {
        crate::entities::location::scheme::parse(input)
    }
}

#[cfg(test)]
mod tests;
