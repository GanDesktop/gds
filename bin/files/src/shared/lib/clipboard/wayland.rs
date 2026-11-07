//! File formats transported by GPUI on the focused window's core Wayland connection.
use std::path::PathBuf;

use gpui_kit::{App, ClipboardAccessError, ClipboardMime, ClipboardMimeOffer, Task};

use super::{
    Error, Identity, Mode, Snapshot,
    protocol::{Formats, GNOME_FILES, KDE_CUT, MAX_PAYLOAD_BYTES, URI_LIST},
};

const CLIPBOARD_ID: &str = "application/x-gds-clipboard-id";
const FILE_TYPES: [&str; 4] = [GNOME_FILES, URI_LIST, KDE_CUT, CLIPBOARD_ID];

pub(super) struct Clipboard;

impl Clipboard {
    pub(super) fn write_uris(uris: Vec<String>, cx: &App) -> Result<(), Error> {
        let formats = Formats {
            uri_list: (uris.join("\r\n") + "\r\n").into_bytes(),
            gnome: ("copy\n".to_owned() + &uris.join("\n")).into_bytes(),
            kde_cut: b"0".to_vec(),
        };
        cx.write_clipboard_mime(Self::entries(formats, &Identity(None)), None)?;
        Ok(())
    }

    pub(super) fn read_uris(cx: &mut App) -> Task<Result<Option<(Mode, Vec<String>)>, Error>> {
        let read = Self::offer(cx);
        cx.spawn(async move |_| {
            let offer = read.await?;
            offer
                .as_ref()
                .map(Self::decode_uris)
                .transpose()
                .map(Option::flatten)
        })
    }

    pub fn write(snapshot: &Snapshot, cx: &App) -> Result<Snapshot, Error> {
        let formats = Formats::encode(snapshot)?;
        let mut written = Formats::decode_uris(&formats.uri_list, snapshot.mode)?;
        written.identity = snapshot.identity.clone();
        cx.write_clipboard_mime(Self::entries(formats, &written.identity), None)?;
        Ok(written)
    }

    fn entries(formats: Formats, identity: &Identity) -> Vec<ClipboardMime> {
        let mut entries = [
            (URI_LIST, formats.uri_list),
            (GNOME_FILES, formats.gnome),
            (KDE_CUT, formats.kde_cut),
        ]
        .into_iter()
        .map(|(mime_type, bytes)| ClipboardMime {
            mime_type: mime_type.into(),
            bytes,
        })
        .collect::<Vec<_>>();
        if let Some(identity) = &identity.0 {
            entries.push(ClipboardMime {
                mime_type: CLIPBOARD_ID.into(),
                bytes: identity.as_bytes().to_vec(),
            });
        }
        entries
    }

    fn offer(cx: &App) -> Task<Result<Option<ClipboardMimeOffer>, ClipboardAccessError>> {
        // All representations come from one offer; no reconnecting between formats.
        cx.read_clipboard_mime(FILE_TYPES.map(str::to_owned).into(), MAX_PAYLOAD_BYTES)
    }

    fn bytes<'a>(offer: &'a ClipboardMimeOffer, mime: &str) -> Option<&'a [u8]> {
        offer
            .entries
            .iter()
            .find(|entry| entry.mime_type == mime)
            .map(|entry| entry.bytes.as_slice())
    }

    fn decode_uris(offer: &ClipboardMimeOffer) -> Result<Option<(Mode, Vec<String>)>, Error> {
        if let Some(bytes) = Self::bytes(offer, GNOME_FILES) {
            let text =
                std::str::from_utf8(bytes).map_err(|e| Error::InvalidPayload(e.to_string()))?;
            let (intent, uris) = text
                .split_once('\n')
                .ok_or_else(|| Error::InvalidPayload("missing copy intent".into()))?;
            let mode = match intent.trim_end_matches('\r') {
                "copy" => Mode::Copy,
                "cut" => Mode::Cut,
                _ => return Err(Error::InvalidPayload("invalid copy intent".into())),
            };
            return Ok(Some((mode, super::parse_uri_list(uris)?)));
        }
        let Some(bytes) = Self::bytes(offer, URI_LIST) else {
            return Ok(None);
        };
        let mode = Self::bytes(offer, KDE_CUT)
            .map(Formats::kde_mode)
            .transpose()?
            .unwrap_or(Mode::Copy);
        let text = std::str::from_utf8(bytes).map_err(|e| Error::InvalidPayload(e.to_string()))?;
        Ok(Some((mode, super::parse_uri_list(text)?)))
    }

    fn decode(offer: &ClipboardMimeOffer) -> Result<Option<Snapshot>, Error> {
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

    pub fn read(cx: &mut App) -> Task<Result<Option<Snapshot>, Error>> {
        let read = Self::offer(cx);
        cx.spawn(async move |_| {
            let offer = read.await?;
            offer
                .as_ref()
                .map(Self::decode)
                .transpose()
                .map(Option::flatten)
        })
    }

    pub fn remove_paths(
        expected: Snapshot,
        removed: Vec<PathBuf>,
        explicit: bool,
        cx: &mut App,
    ) -> Task<Result<(), Error>> {
        let read = Self::offer(cx);
        cx.spawn(async move |cx| {
            let Some(offer) = read.await? else {
                return Ok(());
            };
            let Some(current) = Self::decode(&offer)? else {
                return Ok(());
            };
            let remaining = if explicit {
                expected.excluding(&current, &removed)
            } else {
                expected.remaining_after(&current, &removed)
            };
            let Some(remaining) = remaining else {
                return Ok(());
            };
            let entries = if remaining.is_empty() {
                Vec::new()
            } else {
                let snapshot = Snapshot::new(expected.mode, remaining)?;
                Self::entries(Formats::encode(&snapshot)?, &snapshot.identity)
            };
            // The offer revision is checked on the UI thread immediately before publication.
            match cx.update(|cx| cx.write_clipboard_mime(entries, Some(offer.id))) {
                Ok(()) | Err(ClipboardAccessError::Changed) => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
    }
}
