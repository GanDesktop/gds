use std::path::{Component, PathBuf};
use url::Url;

use super::{Error, Identity, Mode, Snapshot};

pub(super) const URI_LIST: &str = "text/uri-list";
pub(super) const GNOME_FILES: &str = "x-special/gnome-copied-files";
pub(super) const KDE_CUT: &str = "application/x-kde-cutselection";
pub(super) const CLIPBOARD_ID: &str = "application/x-gds-clipboard-id";
pub(super) const TEXT_PLAIN_UTF8: &str = "text/plain;charset=utf-8";
/// Plain-text MIME aliases offered alongside file URIs so native Wayland, XWayland, and X11 text
/// fields all recognize the copied path string (matching `zgui-platform-wayland`'s text offer set).
pub(super) const TEXT_PLAIN_MIMES: [&str; 5] = [
    TEXT_PLAIN_UTF8,
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];
pub(super) const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct Formats {
    pub uri_list: Vec<u8>,
    pub gnome: Vec<u8>,
    pub kde_cut: Vec<u8>,
    pub plain_text: Vec<u8>,
}

impl Formats {
    /// # Errors
    /// Reports unrepresentable paths or oversized payloads.
    pub fn encode(snapshot: &Snapshot) -> Result<Self, Error> {
        let uri_list = Self::encode_uris(&snapshot.paths)?;
        let mut gnome = match snapshot.mode {
            Mode::Copy => b"copy".to_vec(),
            Mode::Cut => b"cut".to_vec(),
        };
        // Nautilus requires LF-separated URIs without a trailing empty line.
        // https://github.com/GNOME/nautilus/blob/main/src/nautilus-clipboard.c
        for uri in std::str::from_utf8(&uri_list)
            .expect("URL serialization produces UTF-8")
            .lines()
        {
            gnome.push(b'\n');
            gnome.extend_from_slice(uri.as_bytes());
        }
        let kde_cut = match snapshot.mode {
            Mode::Copy => b"0".to_vec(),
            Mode::Cut => b"1".to_vec(),
        };
        let plain_text = Self::encode_plain_paths(&snapshot.paths, &uri_list);
        if [&uri_list, &gnome, &plain_text]
            .iter()
            .any(|bytes| bytes.len() > MAX_PAYLOAD_BYTES)
        {
            return Err(Error::TooLarge);
        }
        Ok(Self {
            uri_list,
            gnome,
            kde_cut,
            plain_text,
        })
    }

    /// # Errors
    /// Reports oversized remote URI lists.
    pub fn from_remote_uris(checked: &[String]) -> Result<Self, Error> {
        let uri_list = (checked.join("\r\n") + "\r\n").into_bytes();
        let gnome = ("copy\n".to_owned() + &checked.join("\n")).into_bytes();
        let kde_cut = b"0".to_vec();
        let plain_text = checked.join("\n").into_bytes();
        if [&uri_list, &gnome, &plain_text]
            .iter()
            .any(|bytes| bytes.len() > MAX_PAYLOAD_BYTES)
        {
            return Err(Error::TooLarge);
        }
        Ok(Self {
            uri_list,
            gnome,
            kde_cut,
            plain_text,
        })
    }

    pub(super) fn encode_plain_paths(paths: &[PathBuf], uri_list: &[u8]) -> Vec<u8> {
        if let Some(utf8_paths) = paths
            .iter()
            .map(|path| path.to_str())
            .collect::<Option<Vec<_>>>()
        {
            return utf8_paths.join("\n").into_bytes();
        }
        std::str::from_utf8(uri_list)
            .expect("URL serialization produces UTF-8")
            .lines()
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes()
    }

    /// # Errors
    /// Reports paths which cannot become local file URLs or resolve parent traversal.
    pub fn encode_uris(paths: &[PathBuf]) -> Result<Vec<u8>, Error> {
        let mut output = String::new();
        for path in paths {
            // URL parsers normalize '..'; resolve only its parent when necessary
            // so an alias/../child retains filesystem meaning, never following the leaf.
            let resolved;
            let wire_path = if path.components().any(|part| part == Component::ParentDir) {
                let parent = path
                    .parent()
                    .ok_or_else(|| Error::InvalidPath(path.clone()))?;
                let name = path
                    .file_name()
                    .ok_or_else(|| Error::InvalidPath(path.clone()))?;
                resolved = parent.canonicalize()?.join(name);
                &resolved
            } else {
                path
            };
            let uri =
                Url::from_file_path(wire_path).map_err(|_| Error::InvalidPath(path.clone()))?;
            output.push_str(uri.as_str());
            output.push_str("\r\n");
            if output.len() > MAX_PAYLOAD_BYTES {
                return Err(Error::TooLarge);
            }
        }
        Ok(output.into_bytes())
    }

    /// # Errors
    /// Rejects invalid, remote, non-file or empty URI lists.
    pub fn decode_uris(bytes: &[u8], mode: Mode) -> Result<Snapshot, Error> {
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::TooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| {
            Error::InvalidPayload("URI list must be UTF-8 with percent-encoded path bytes".into())
        })?;
        let paths = text
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(Self::local_file_uri)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Snapshot {
            mode,
            paths: Snapshot::validate_paths(paths)?,
            identity: Identity(None),
        })
    }

    /// # Errors
    /// Requires a copy/cut intent and a valid local URI list.
    pub fn decode_gnome(bytes: &[u8]) -> Result<Snapshot, Error> {
        let mut lines = bytes.splitn(2, |byte| *byte == b'\n');
        let intent = lines.next().expect("splitn yields at least one slice");
        let Some(uris) = lines.next() else {
            return Err(Error::InvalidPayload(
                "GNOME file clipboard has no intent line".into(),
            ));
        };
        let mode = match intent.strip_suffix(b"\r").unwrap_or(intent) {
            b"copy" => Mode::Copy,
            b"cut" => Mode::Cut,
            _ => {
                return Err(Error::InvalidPayload(
                    "GNOME file clipboard intent must be copy or cut".into(),
                ));
            }
        };
        Self::decode_uris(uris, mode)
    }

    /// # Errors
    /// Requires a recognized KDE copy/cut marker.
    pub fn kde_mode(bytes: &[u8]) -> Result<Mode, Error> {
        let marker = bytes
            .trim_ascii()
            .strip_suffix(b"\0")
            .unwrap_or(bytes.trim_ascii());
        match marker {
            b"0" => Ok(Mode::Copy),
            b"1" => Ok(Mode::Cut),
            _ => Err(Error::InvalidPayload(
                "KDE cut marker must be 0 or 1".into(),
            )),
        }
    }

    /// # Errors
    /// Rejects oversized, empty, malformed or password-bearing URI lists.
    pub fn parse_uri_list(text: &str) -> Result<Vec<String>, Error> {
        if text.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::TooLarge);
        }
        let uris = text
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let uri =
                    Url::parse(line).map_err(|error| Error::InvalidPayload(error.to_string()))?;
                if uri.password().is_some() {
                    return Err(Error::InvalidPayload(
                        "passwords are not allowed in file URIs".into(),
                    ));
                }
                Ok(uri.to_string())
            })
            .collect::<Result<Vec<_>, Error>>()?;
        if uris.is_empty() {
            return Err(Error::InvalidPayload("empty URI list".into()));
        }
        Ok(uris)
    }

    pub fn copyable_uris(uris: &[String]) -> bool {
        Self::copyable_uris_with_host(uris, gds::service::desktop::recent_files::local_hostname())
    }

    fn copyable_uris_with_host(uris: &[String], local_hostname: Option<&str>) -> bool {
        !uris.is_empty()
            && uris.iter().all(|uri| {
                gds::service::remote::Address::parse(uri).is_ok()
                    || Self::local_file_uri_with_host(uri, local_hostname).is_ok()
            })
    }

    /// # Errors
    /// Rejects non-local, relative or parameterized file URLs.
    fn local_file_uri(text: &str) -> Result<PathBuf, Error> {
        Self::local_file_uri_with_host(text, gds::service::desktop::recent_files::local_hostname())
    }

    fn local_file_uri_with_host(
        text: &str,
        local_hostname: Option<&str>,
    ) -> Result<PathBuf, Error> {
        gds::service::desktop::recent_files::parse_strict_local_file_url_with_host(
            text,
            local_hostname,
        )
        .ok_or_else(|| Error::InvalidUri(text.into()))
    }
}

#[cfg(test)]
mod tests;
