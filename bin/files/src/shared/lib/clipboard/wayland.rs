//! Multi-format snapshots from the focused native Wayland selection.
use super::{
    Error, Identity, Mode, Snapshot,
    protocol::{Formats, GNOME_FILES, KDE_CUT, MAX_PAYLOAD_BYTES, URI_LIST},
};
use anti_webui::prelude::*;
use std::{path::PathBuf, sync::Arc};
const CLIPBOARD_ID: &str = "application/x-gds-clipboard-id";
const FILE_TYPES: [&str; 4] = [GNOME_FILES, URI_LIST, KDE_CUT, CLIPBOARD_ID];
pub(super) struct Clipboard;
impl Clipboard {
    pub fn offer() -> Task<Result<ClipboardSnapshot, ClipboardError>> {
        read_clipboard_formats(
            ClipboardKind::Clipboard,
            FILE_TYPES.map(str::to_owned).into(),
            MAX_PAYLOAD_BYTES,
        )
    }
    fn entries(formats: Formats, identity: &Identity) -> Vec<ClipboardData> {
        let mut entries = [
            (URI_LIST, formats.uri_list),
            (GNOME_FILES, formats.gnome),
            (KDE_CUT, formats.kde_cut),
        ]
        .into_iter()
        .map(|(mime, bytes)| ClipboardData {
            mime: mime.into(),
            bytes: bytes.into(),
        })
        .collect::<Vec<_>>();
        if let Some(identity) = &identity.0 {
            entries.push(ClipboardData {
                mime: CLIPBOARD_ID.into(),
                bytes: identity.as_bytes().into(),
            });
        }
        entries
    }
    pub fn write(mut snapshot: Snapshot) -> Task<Result<Snapshot, Error>> {
        let formats = match Formats::encode(&snapshot) {
            Ok(formats) => formats,
            Err(error) => return Task::ready(Err(error)),
        };
        snapshot.paths = match Formats::decode_uris(&formats.uri_list, snapshot.mode) {
            Ok(decoded) => decoded.paths,
            Err(error) => return Task::ready(Err(error)),
        };
        write_clipboard(
            ClipboardKind::Clipboard,
            Self::entries(formats, &snapshot.identity),
        )
        .map(move |result| result.map(|_| snapshot).map_err(Error::from))
    }
    pub fn write_uris(uris: Vec<String>) -> Task<Result<(), Error>> {
        let uris = match Formats::parse_uri_list(&uris.join("\n")) {
            Ok(uris) => uris,
            Err(error) => return Task::ready(Err(error)),
        };
        let formats = Formats {
            uri_list: (uris.join("\r\n") + "\r\n").into_bytes(),
            gnome: ("copy\n".to_owned() + &uris.join("\n")).into_bytes(),
            kde_cut: b"0".to_vec(),
        };
        write_clipboard(
            ClipboardKind::Clipboard,
            Self::entries(formats, &Identity(None)),
        )
        .map(|result| result.map_err(Error::from))
    }
    fn bytes<'a>(offer: &'a ClipboardSnapshot, mime: &str) -> Option<&'a [u8]> {
        offer
            .entries
            .iter()
            .find(|entry| entry.mime == mime)
            .map(|entry| entry.bytes.as_ref())
    }
    pub fn decode_uris(offer: &ClipboardSnapshot) -> Result<Option<(Mode, Vec<String>)>, Error> {
        if let Some(bytes) = Self::bytes(offer, GNOME_FILES) {
            let text = std::str::from_utf8(bytes)
                .map_err(|error| Error::InvalidPayload(error.to_string()))?;
            let (intent, uris) = text
                .split_once('\n')
                .ok_or_else(|| Error::InvalidPayload("missing copy intent".into()))?;
            let mode = match intent.trim_end_matches('\r') {
                "copy" => Mode::Copy,
                "cut" => Mode::Cut,
                _ => return Err(Error::InvalidPayload("invalid copy intent".into())),
            };
            return Ok(Some((mode, Formats::parse_uri_list(uris)?)));
        }
        let Some(bytes) = Self::bytes(offer, URI_LIST) else {
            return Ok(None);
        };
        let mode = Self::bytes(offer, KDE_CUT)
            .map(Formats::kde_mode)
            .transpose()?
            .unwrap_or(Mode::Copy);
        let text =
            std::str::from_utf8(bytes).map_err(|error| Error::InvalidPayload(error.to_string()))?;
        Ok(Some((mode, Formats::parse_uri_list(text)?)))
    }
    pub fn decode(offer: &ClipboardSnapshot) -> Result<Option<Snapshot>, Error> {
        let mut snapshot = if let Some(bytes) = Self::bytes(offer, GNOME_FILES) {
            Formats::decode_gnome(bytes)?
        } else {
            let Some(bytes) = Self::bytes(offer, URI_LIST) else {
                return Ok(None);
            };
            let mode = Self::bytes(offer, KDE_CUT)
                .map(Formats::kde_mode)
                .transpose()?
                .unwrap_or(Mode::Copy);
            Formats::decode_uris(bytes, mode)?
        };
        if let Some(identity) = Self::bytes(offer, CLIPBOARD_ID) {
            snapshot.identity = Identity::decode(identity)?;
        }
        Ok(Some(snapshot))
    }
    #[expect(
        dead_code,
        reason = "The atomic local-file clipboard protocol remains independently callable"
    )]
    pub fn read() -> Task<Result<Option<Snapshot>, Error>> {
        Self::offer().map(|result| Self::decode(&result?))
    }
    pub fn read_uris() -> Task<Result<Option<(Mode, Vec<String>)>, Error>> {
        Self::offer().map(|result| Self::decode_uris(&result?))
    }
    pub fn remove_paths(
        expected: Arc<Snapshot>,
        removed: Vec<PathBuf>,
        explicit: bool,
    ) -> Task<Result<(), Error>> {
        let (task, done) = Task::pending();
        Self::offer().then(move |offer| {
            let prepared = (|| {
                let offer = offer?;
                let Some(current) = Self::decode(&offer)? else {
                    return Ok(None);
                };
                let remaining = if explicit {
                    expected.excluding(&current, &removed)
                } else {
                    expected.remaining_after(&current, &removed)
                };
                let Some(remaining) = remaining else {
                    return Ok(None);
                };
                let entries = if remaining.is_empty() {
                    Vec::new()
                } else {
                    let snapshot = Snapshot::new(expected.mode, remaining)?;
                    Self::entries(Formats::encode(&snapshot)?, &snapshot.identity)
                };
                Ok::<_, Error>(Some((offer.revision, entries)))
            })();
            match prepared {
                Ok(Some((revision, entries))) => {
                    write_clipboard_if_unchanged(ClipboardKind::Clipboard, entries, revision).then(
                        move |result| {
                            done.complete(match result {
                                Ok(()) | Err(ClipboardError::Changed) => Ok(()),
                                Err(error) => Err(error.into()),
                            })
                        },
                    )
                }
                Ok(None) => done.complete(Ok(())),
                Err(error) => done.complete(Err(error)),
            }
        });
        task
    }
}
