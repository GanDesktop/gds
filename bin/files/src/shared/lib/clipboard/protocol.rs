use std::path::{Component, PathBuf};
use url::Url;

use super::{Error, Identity, Mode, Snapshot};

pub(super) const URI_LIST: &str = "text/uri-list";
pub(super) const GNOME_FILES: &str = "x-special/gnome-copied-files";
pub(super) const KDE_CUT: &str = "application/x-kde-cutselection";
pub(super) const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct Formats {
    pub uri_list: Vec<u8>,
    pub gnome: Vec<u8>,
    pub kde_cut: Vec<u8>,
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
        if [&uri_list, &gnome]
            .iter()
            .any(|bytes| bytes.len() > MAX_PAYLOAD_BYTES)
        {
            return Err(Error::TooLarge);
        }
        Ok(Self {
            uri_list,
            gnome,
            kde_cut,
        })
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
    /// Rejects non-local, relative or parameterized file URLs.
    fn local_file_uri(text: &str) -> Result<PathBuf, Error> {
        let uri = Url::parse(text).map_err(|_| Error::InvalidUri(text.into()))?;
        let absolute = text
            .split_once(':')
            .is_some_and(|(_, rest)| rest.starts_with('/'));
        if text.as_bytes().contains(&0)
            || !absolute
            || uri.scheme() != "file"
            || uri.host_str().is_some_and(|host| host != "localhost")
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.port().is_some()
            || uri.query().is_some()
            || uri.fragment().is_some()
        {
            return Err(Error::InvalidUri(text.into()));
        }
        uri.to_file_path()
            .map_err(|_| Error::InvalidUri(text.into()))
    }
}
