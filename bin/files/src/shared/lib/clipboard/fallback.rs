use std::path::PathBuf;

use gpui_kit::{App, ClipboardEntry, ClipboardItem, ExternalPaths, Task};
use serde::{Deserialize, Serialize};

use super::{
    Error, Identity, Mode, Snapshot,
    protocol::{Formats, MAX_PAYLOAD_BYTES},
};

const METADATA_PREFIX: &str = "gds-files-v1:";

#[derive(Serialize, Deserialize)]
struct Metadata {
    mode: Mode,
    identity: String,
}

pub(super) struct Clipboard;

impl Clipboard {
    /// # Errors
    /// Rejects malformed file offers; ordinary text never becomes a file selection.
    pub fn decode_uris(item: Option<&ClipboardItem>) -> Result<Option<(Mode, Vec<String>)>, Error> {
        if let Some(snapshot) = Self::decode(item)? {
            let uris = snapshot
                .paths
                .iter()
                .map(|path| {
                    url::Url::from_file_path(path)
                        .map(|uri| uri.to_string())
                        .map_err(|_| Error::InvalidPath(path.clone()))
                })
                .collect::<Result<_, _>>()?;
            return Ok(Some((snapshot.mode, uris)));
        }
        let Some(item) = item else {
            return Ok(None);
        };
        let remote = item.entries().iter().any(|entry| matches!(entry, gpui_kit::ClipboardEntry::String(text) if text.metadata.as_deref() == Some(super::REMOTE_URI_METADATA)));
        if !remote {
            return Ok(None);
        }
        item.text()
            .map(|text| Formats::parse_uri_list(&text).map(|uris| (Mode::Copy, uris)))
            .transpose()
    }

    /// # Errors
    /// Rejects invalid selections and metadata encoding failures.
    pub fn write(mode: Mode, paths: Vec<PathBuf>, cx: &mut App) -> Result<Snapshot, Error> {
        let snapshot = Snapshot::new(mode, paths)?;
        cx.write_to_clipboard(Self::encode(&snapshot)?);
        Ok(snapshot)
    }

    /// # Errors
    /// The task reports platform read or GDS metadata failures.
    pub fn read(cx: &mut App) -> Task<Result<Option<Snapshot>, Error>> {
        let read = cx.read_from_clipboard_async();
        cx.spawn(async move |_| {
            let item = read.await?;
            Self::decode(item.as_ref())
        })
    }

    /// # Errors
    /// The task reports platform read or metadata encoding failures.
    pub fn finish_cut(
        expected: Snapshot,
        succeeded: Vec<PathBuf>,
        cx: &mut App,
    ) -> Task<Result<(), Error>> {
        Self::remove_paths(expected, succeeded, false, cx)
    }

    /// # Errors
    /// The task reports platform read or metadata encoding failures.
    pub fn remove_paths(
        expected: Snapshot,
        succeeded: Vec<PathBuf>,
        explicit: bool,
        cx: &mut App,
    ) -> Task<Result<(), Error>> {
        let read = cx.read_from_clipboard_async();
        cx.spawn(async move |cx| {
            let item = read.await?;
            let Some(current) = Self::decode(item.as_ref())? else {
                return Ok(());
            };
            let remaining = if explicit {
                expected.excluding(&current, &succeeded)
            } else {
                expected.remaining_after(&current, &succeeded)
            };
            let Some(remaining) = remaining else {
                return Ok(());
            };
            let item = if remaining.is_empty() {
                ClipboardItem::new_string(String::new())
            } else {
                Self::encode(&Snapshot::new(expected.mode, remaining)?)?
            };
            cx.update(|cx| cx.write_to_clipboard(item));
            Ok(())
        })
    }

    /// # Errors
    /// Rejects invalid private metadata or file URL representations.
    pub fn encode(snapshot: &Snapshot) -> Result<ClipboardItem, Error> {
        let text = String::from_utf8(Formats::encode_uris(&snapshot.paths)?)
            .expect("URL serialization always produces UTF-8");
        // GPUI's X11 metadata cache is keyed by text. Include the per-copy identity
        // in that text so separately copied ordinary URI text cannot reuse old intent.
        let identity = snapshot
            .identity
            .0
            .as_deref()
            .expect("private encoding requires an identity");
        let private = serde_json::to_string(&Metadata {
            mode: snapshot.mode,
            identity: identity.to_owned(),
        })?;
        let text = format!("# {METADATA_PREFIX}{identity}\r\n{text}");
        let mut item =
            ClipboardItem::new_string_with_metadata(text, format!("{METADATA_PREFIX}{private}"));
        item.entries
            .push(ClipboardEntry::ExternalPaths(ExternalPaths(
                snapshot.paths.iter().cloned().collect(),
            )));
        Ok(item)
    }

    /// # Errors
    /// Rejects malformed metadata or a mismatched copy identity.
    pub fn decode(item: Option<&ClipboardItem>) -> Result<Option<Snapshot>, Error> {
        let Some(item) = item else {
            return Ok(None);
        };
        let Some(private) = item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::String(text) => text.metadata.as_deref()?.strip_prefix(METADATA_PREFIX),
            _ => None,
        }) else {
            return Ok(None);
        };
        if private.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::TooLarge);
        }
        let metadata: Metadata = serde_json::from_str(private)?;
        let Some(paths) = item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::ExternalPaths(paths) => Some(paths.paths().to_vec()),
            _ => None,
        }) else {
            return Err(Error::InvalidPayload(
                "file clipboard has no embedded paths".into(),
            ));
        };
        let snapshot = Snapshot {
            mode: metadata.mode,
            paths: Snapshot::validate_paths(paths)?,
            identity: Identity::decode(metadata.identity.as_bytes())?,
        };
        let mut expected = format!(
            "# {METADATA_PREFIX}{}\r\n",
            snapshot
                .identity
                .0
                .as_deref()
                .expect("decoded private identity")
        )
        .into_bytes();
        expected.extend(Formats::encode_uris(&snapshot.paths)?);
        let matches_contents = item.entries().iter().any(|entry| match entry {
            ClipboardEntry::String(text) => text.text.as_bytes() == expected.as_slice(),
            _ => false,
        });
        if !matches_contents {
            return Err(Error::InvalidPayload(
                "GDS metadata and clipboard contents disagree".into(),
            ));
        }
        Ok(Some(snapshot))
    }
}
