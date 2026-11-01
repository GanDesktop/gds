use gds::service::remote::{Address, Credentials, Protocol};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    Open,
    Add,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Authentication {
    Agent,
    Password,
    Key,
    Guest,
}

pub(super) struct Request {
    pub address: Address,
    pub credentials: Credentials,
}

pub(super) struct Details {
    pub protocol: Protocol,
    pub host: String,
    pub port: String,
    pub user: String,
    pub path: String,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("a server hostname or IP address is required")]
    Host,
    #[error("port must be between 1 and 65535")]
    Port,
    #[error("an absolute folder path is required")]
    Path,
    #[error("an SMB share name is required")]
    Share,
    #[error(transparent)]
    Address(#[from] gds::service::remote::Error),
}

impl Error {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Host => "remote-invalid-host",
            Self::Port => "remote-invalid-port",
            Self::Path => "remote-invalid-path",
            Self::Share => "remote-missing-share",
            Self::Address(_) => "remote-invalid-address",
        }
    }
}

impl Details {
    /// # Errors
    /// Rejects invalid hosts, ports and paths without interpreting a password as URI data.
    pub fn address(&self) -> Result<Address, Error> {
        let host = self.host.trim();
        if host.is_empty()
            || host.contains(['/', '@', '?', '#'])
            || host.chars().any(char::is_whitespace)
        {
            return Err(Error::Host);
        }
        let port = self
            .port
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or(Error::Port)?;
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let scheme = match self.protocol {
            Protocol::Sftp => "sftp",
            Protocol::Smb => "smb",
        };
        let mut url = url::Url::parse(&format!("{scheme}://{host}")).map_err(|_| Error::Host)?;
        url.set_port(
            (port
                != match self.protocol {
                    Protocol::Sftp => 22,
                    Protocol::Smb => 445,
                })
            .then_some(port),
        )
        .map_err(|_| Error::Port)?;
        let path = match self.protocol {
            Protocol::Sftp if !self.path.starts_with('/') => return Err(Error::Path),
            Protocol::Smb if self.path.trim_matches('/').is_empty() => return Err(Error::Share),
            _ => format!("/{}", self.path.trim_start_matches('/')),
        };
        let encoded = path
            .split('/')
            .map(|part| utf8_percent_encode(part, PATH_SEGMENT).to_string())
            .collect::<Vec<_>>()
            .join("/");
        url.set_path(&encoded);
        Ok(Address::parse(url.as_str())?.with_username(&self.user)?)
    }
}

#[cfg(test)]
mod tests;
