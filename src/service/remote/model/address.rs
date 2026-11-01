use super::super::Error;
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    Sftp,
    Smb,
}

/// A validated, password-free URI; never a local filesystem path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Address(Url);

impl Address {
    /// # Errors
    /// Rejects unsupported schemes, credentials in the URI, and invalid path encodings.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let mut url = Url::parse(input).map_err(|e| Error::Address(e.to_string()))?;
        if !matches!(url.scheme(), "sftp" | "smb") || url.host_str().is_none() {
            return Err(Error::Address(
                "use sftp://host/path or smb://host/share".into(),
            ));
        }
        if url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
            return Err(Error::Address(
                "passwords, queries and fragments are not accepted".into(),
            ));
        }
        for value in [url.username(), url.path()] {
            let bytes = value.as_bytes();
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'%'
                    && !bytes
                        .get(index + 1..index + 3)
                        .is_some_and(|v| v.iter().all(u8::is_ascii_hexdigit))
                {
                    return Err(Error::Address("invalid percent encoding".into()));
                }
            }
            let decoded = percent_decode_str(value)
                .decode_utf8()
                .map_err(|_| Error::Address("remote names must be UTF-8".into()))?;
            if decoded.contains('\0')
                || (value == url.path() && url.scheme() == "smb" && decoded.contains('\\'))
            {
                return Err(Error::Address("invalid character in remote address".into()));
            }
        }
        if url.path().is_empty() {
            url.set_path("/");
        }
        Ok(Self(url))
    }

    pub fn protocol(&self) -> Protocol {
        if self.0.scheme() == "sftp" {
            Protocol::Sftp
        } else {
            Protocol::Smb
        }
    }
    pub fn host(&self) -> &str {
        let host = self.0.host_str().expect("validated remote host");
        host.strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .unwrap_or(host)
    }
    pub fn port(&self) -> u16 {
        self.0.port().unwrap_or(match self.protocol() {
            Protocol::Sftp => 22,
            Protocol::Smb => 445,
        })
    }
    pub fn username(&self) -> String {
        percent_decode_str(self.0.username())
            .decode_utf8()
            .expect("validated username")
            .into_owned()
    }
    pub fn path(&self) -> String {
        percent_decode_str(self.0.path())
            .decode_utf8()
            .expect("validated path")
            .into_owned()
    }
    pub fn uri(&self) -> &str {
        self.0.as_str()
    }
    /// # Errors
    /// Rejects invalid characters in the username.
    pub fn with_username(&self, username: &str) -> Result<Self, Error> {
        let mut url = self.0.clone();
        url.set_username(username)
            .expect("validated hierarchical address");
        Self::parse(url.as_str())
    }
    pub fn name(&self) -> String {
        self.path()
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|v| !v.is_empty())
            .unwrap_or(self.host())
            .to_owned()
    }
    pub fn connection_key(&self) -> String {
        let mut url = self.0.clone();
        url.set_path("/");
        url.to_string()
    }

    /// # Errors
    /// Rejects invalid names or names containing path separators.
    pub fn child(&self, name: &str) -> Result<Self, Error> {
        Self::validate_name(name, self.protocol())?;
        let mut url = self.0.clone();
        // URL's segment setter strips literal tabs/newlines; encode filesystem names first.
        let name = utf8_percent_encode(name, NON_ALPHANUMERIC);
        url.set_path(&format!("{}/{name}", self.0.path().trim_end_matches('/')));
        Self::parse(url.as_str())
    }

    pub fn parent(&self) -> Option<Self> {
        if self.path() == "/" {
            return None;
        }
        let mut url = self.0.clone();
        url.path_segments_mut().ok()?.pop_if_empty().pop();
        if url.path().is_empty() {
            url.set_path("/");
        }
        Some(Self(url))
    }

    /// # Errors
    /// Rejects empty, reserved, or separator-containing names.
    pub fn validate_name(name: &str, protocol: Protocol) -> Result<(), Error> {
        if name.is_empty()
            || matches!(name, "." | "..")
            || name.contains(['/', '\0'])
            || (protocol == Protocol::Smb && name.contains('\\'))
        {
            return Err(Error::Address("invalid file name".into()));
        }
        Ok(())
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.uri())
    }
}
impl Serialize for Address {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.uri())
    }
}
impl<'de> Deserialize<'de> for Address {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
