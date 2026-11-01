use std::{path::PathBuf, time::Duration};

use gds::service::desktop::clipboard::Clipboard as NativeClipboard;
use wl_clipboard_rs::copy;

use super::{
    Error, Identity, Mode, Snapshot,
    protocol::{Formats, GNOME_FILES, KDE_CUT, MAX_PAYLOAD_BYTES, URI_LIST},
};

const READ_TIMEOUT: Duration = Duration::from_secs(5);
// Optional identity for cut cleanup; file contents always use interoperable formats.
const CLIPBOARD_ID: &str = "application/x-gds-clipboard-id";

pub(super) struct Clipboard;

impl Clipboard {
    pub(super) fn write_uris(uris: Vec<String>) -> Result<(), Error> {
        let joined = uris.join("\r\n") + "\r\n";
        Self::publish(
            Formats {
                uri_list: joined.into_bytes(),
                gnome: ("copy\n".to_owned() + &uris.join("\n")).into_bytes(),
                kde_cut: b"0".to_vec(),
            },
            &Identity(None),
        )
    }

    pub(super) fn read_uris() -> Result<Option<(Mode, Vec<String>)>, Error> {
        let clipboard = NativeClipboard::new(READ_TIMEOUT, MAX_PAYLOAD_BYTES);
        let reader = clipboard.reader()?;
        if let Some(bytes) = reader.read(GNOME_FILES)? {
            let text =
                std::str::from_utf8(&bytes).map_err(|e| Error::InvalidPayload(e.to_string()))?;
            let (mode, uris) = text
                .split_once('\n')
                .ok_or_else(|| Error::InvalidPayload("missing copy intent".into()))?;
            let mode = match mode.trim_end_matches('\r') {
                "copy" => Mode::Copy,
                "cut" => Mode::Cut,
                _ => return Err(Error::InvalidPayload("invalid copy intent".into())),
            };
            return Ok(Some((mode, super::parse_uri_list(uris)?)));
        }
        let Some(bytes) = reader.read(URI_LIST)? else {
            return Ok(None);
        };
        let marker = reader.read(KDE_CUT)?;
        if reader.read(URI_LIST)?.as_ref() != Some(&bytes) {
            return Err(Error::Changed);
        }
        let mode = marker
            .as_deref()
            .map(Formats::kde_mode)
            .transpose()?
            .unwrap_or(Mode::Copy);
        let text = std::str::from_utf8(&bytes).map_err(|e| Error::InvalidPayload(e.to_string()))?;
        Ok(Some((mode, super::parse_uri_list(text)?)))
    }
    /// # Errors
    /// Reports payload encoding or Wayland offer failures.
    pub fn write(snapshot: &Snapshot) -> Result<Snapshot, Error> {
        let formats = Formats::encode(snapshot)?;
        let mut written = Formats::decode_uris(&formats.uri_list, snapshot.mode)?;
        written.identity = snapshot.identity.clone();
        Self::publish(formats, &written.identity)?;
        Ok(written)
    }

    /// # Errors
    /// Propagates clipboard protocol and publication failures.
    fn publish(formats: Formats, identity: &Identity) -> Result<(), Error> {
        let mut sources = [
            (URI_LIST, formats.uri_list),
            (GNOME_FILES, formats.gnome),
            (KDE_CUT, formats.kde_cut),
        ]
        .into_iter()
        .map(|(mime, bytes)| copy::MimeSource {
            source: copy::Source::Bytes(bytes.into_boxed_slice()),
            mime_type: copy::MimeType::Specific(mime.to_owned()),
        })
        .collect::<Vec<_>>();
        if let Some(identity) = &identity.0 {
            sources.push(copy::MimeSource {
                source: copy::Source::Bytes(identity.as_bytes().into()),
                mime_type: copy::MimeType::Specific(CLIPBOARD_ID.to_owned()),
            });
        }
        let mut options = copy::Options::new();
        options
            .clipboard(copy::ClipboardType::Regular)
            .omit_additional_text_mime_types(true);
        // The library owns the serving thread. This returns after preparation,
        // not when another application eventually replaces the clipboard.
        options.copy_multi(sources)?;
        Ok(())
    }

    /// # Errors
    /// Reports malformed/changing offers, protocol errors or a stalled data source.
    pub fn read() -> Result<Option<Snapshot>, Error> {
        let clipboard = NativeClipboard::new(READ_TIMEOUT, MAX_PAYLOAD_BYTES);
        let reader = clipboard.reader()?;
        let identity = reader.read(CLIPBOARD_ID)?;
        let mut snapshot = if let Some(bytes) = reader.read(GNOME_FILES)? {
            Formats::decode_gnome(&bytes)?
        } else {
            let Some(uris) = reader.read(URI_LIST)? else {
                return Ok(None);
            };
            let marker = reader.read(KDE_CUT)?;
            // Each MIME uses a new connection. Recheck before combining representations.
            if reader.read(URI_LIST)?.as_ref() != Some(&uris) {
                return Err(Error::Changed);
            }
            let mode = marker
                .as_deref()
                .map(Formats::kde_mode)
                .transpose()?
                .unwrap_or(Mode::Copy);
            Formats::decode_uris(&uris, mode)?
        };
        if reader.read(CLIPBOARD_ID)? != identity {
            return Err(Error::Changed);
        }
        if let Some(identity) = identity {
            snapshot.identity = Identity::decode(&identity)?;
        }
        Ok(Some(snapshot))
    }

    /// # Errors
    /// Reports failures reading or updating the still-owned clipboard.
    pub fn finish_cut(expected: &Snapshot, succeeded: &[PathBuf]) -> Result<(), Error> {
        let Some(current) = Self::read()? else {
            return Ok(());
        };
        let Some(remaining) = expected.remaining_after(&current, succeeded) else {
            return Ok(());
        };
        let replacement = if remaining.is_empty() {
            None
        } else {
            let snapshot = Snapshot::new(Mode::Cut, remaining)?;
            Some((Formats::encode(&snapshot)?, snapshot.identity))
        };
        // Encoding can resolve parent traversal; do not trust an offer read before it.
        if Self::read()?.as_ref() != Some(&current) {
            return Ok(());
        }
        if let Some((formats, identity)) = replacement {
            Self::publish(formats, &identity)?;
        } else {
            copy::clear(copy::ClipboardType::Regular, copy::Seat::All)?;
        }
        Ok(())
    }
}
