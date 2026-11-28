//! Atomic multi-MIME clipboard transport over `zgui`'s `Clipboards::{read_mime, write_mime}`.

use std::{future::Future, path::PathBuf, sync::Arc};

use gds::{app::Gds, service::desktop::file_transfer};
use zgui::platform::{ClipboardError, ClipboardMime, ClipboardMimeOffer, LazyMimeProvider};
use zgui::prelude::{ClipboardKind, Clipboards};

use super::{
    Error, Identity, Mode, Snapshot,
    protocol::{
        CLIPBOARD_ID, Formats, GNOME_FILES, KDE_CUT, MAX_PAYLOAD_BYTES, PORTAL_FILE_TRANSFER,
        TEXT_PLAIN_MIMES, URI_LIST,
    },
};

const FILE_TYPES: [&str; 4] = [GNOME_FILES, URI_LIST, KDE_CUT, CLIPBOARD_ID];
const MAX_PORTAL_KEY_BYTES: usize = 4096;

pub(super) struct NativeClipboard;

impl NativeClipboard {
    pub(super) fn read_offer(
        clip: &Clipboards,
    ) -> impl Future<Output = Result<Option<ClipboardMimeOffer>, ClipboardError>> + 'static {
        clip.read_mime(
            ClipboardKind::Standard,
            FILE_TYPES.map(str::to_owned).into(),
            MAX_PAYLOAD_BYTES,
        )
    }

    pub(super) fn offers_portal_transfer(offer: &ClipboardMimeOffer) -> bool {
        offer.offers_mime(PORTAL_FILE_TRANSFER)
    }

    pub(super) fn read_portal_transfer_key(
        clip: &Clipboards,
    ) -> impl Future<Output = Result<Option<String>, ClipboardError>> + 'static {
        let fut = clip.read_mime(
            ClipboardKind::Standard,
            vec![PORTAL_FILE_TRANSFER.to_owned()],
            MAX_PORTAL_KEY_BYTES,
        );
        async move {
            let Some(offer) = fut.await? else {
                return Ok(None);
            };
            let key = Self::bytes(&offer, PORTAL_FILE_TRANSFER)
                .and_then(file_transfer::decode_transfer_key);
            Ok(key)
        }
    }

    pub(super) fn portal_lazy_providers(paths: Vec<PathBuf>) -> Vec<(String, LazyMimeProvider)> {
        let Some(runtime) =
            Gds::try_current().and_then(|gds| gds.tasks().scope().runtime_handle().cloned())
        else {
            return Vec::new();
        };
        Self::portal_lazy_provider(runtime, paths)
            .into_iter()
            .collect()
    }

    pub(super) fn portal_lazy_provider(
        runtime: tokio::runtime::Handle,
        paths: Vec<PathBuf>,
    ) -> Option<(String, LazyMimeProvider)> {
        if paths.is_empty() {
            return None;
        }
        let provider: LazyMimeProvider = Arc::new(move |_| {
            file_transfer::export_files_blocking(
                &runtime,
                &paths,
                true,
                file_transfer::DEFAULT_EXPORT_TIMEOUT,
            )
        });
        Some((PORTAL_FILE_TRANSFER.to_owned(), provider))
    }

    pub(super) async fn probe_revision(clip: &Clipboards) -> Option<u64> {
        clip.read_mime(ClipboardKind::Standard, Vec::new(), 0)
            .await
            .ok()
            .flatten()
            .map(|offer| offer.revision)
    }

    /// Encode `snapshot` into canonicalized paths and atomic multi-MIME clipboard entries.
    ///
    /// # Errors
    /// Reports invalid paths or oversized payloads.
    pub(super) fn encode_snapshot(
        snapshot: &Snapshot,
    ) -> Result<(Snapshot, Vec<ClipboardMime>), Error> {
        let formats = Formats::encode(snapshot)?;
        let mut written = Formats::decode_uris(&formats.uri_list, snapshot.mode)?;
        written.identity = snapshot.identity.clone();
        let entries = Self::entries(formats, &written.identity);
        Ok((written, entries))
    }

    /// Encode remote/mixed URIs into atomic multi-MIME clipboard entries.
    ///
    /// # Errors
    /// Reports malformed, empty, password-bearing, or oversized URI lists.
    pub(super) fn encode_uris(
        uris: &[String],
    ) -> Result<(Vec<String>, bool, Vec<ClipboardMime>), Error> {
        let checked = Formats::parse_uri_list(&uris.join("\n"))?;
        let copyable = Formats::copyable_uris(&checked);
        let formats = Formats::from_remote_uris(&checked)?;
        let entries = Self::entries(formats, &Identity(None));
        Ok((checked, copyable, entries))
    }

    fn entries(formats: Formats, identity: &Identity) -> Vec<ClipboardMime> {
        let mut entries = Vec::with_capacity(4 + TEXT_PLAIN_MIMES.len());
        entries.push(ClipboardMime::new(URI_LIST, formats.uri_list));
        entries.push(ClipboardMime::new(GNOME_FILES, formats.gnome));
        entries.push(ClipboardMime::new(KDE_CUT, formats.kde_cut));
        for mime_type in TEXT_PLAIN_MIMES {
            entries.push(ClipboardMime::new(mime_type, formats.plain_text.clone()));
        }
        if let Some(id) = &identity.0 {
            entries.push(ClipboardMime::new(CLIPBOARD_ID, id.as_bytes().to_vec()));
        }
        entries
    }

    fn bytes<'a>(offer: &'a ClipboardMimeOffer, mime: &str) -> Option<&'a [u8]> {
        offer
            .entries
            .iter()
            .find(|entry| entry.mime_type.eq_ignore_ascii_case(mime))
            .map(|entry| entry.bytes.as_slice())
    }

    /// Decode a local file snapshot from a multi-MIME offer, never inferring files from plain text.
    ///
    /// # Errors
    /// Rejects malformed GNOME/KDE file offers, non-local URIs, or invalid clipboard identities.
    pub(super) fn decode_snapshot(offer: &ClipboardMimeOffer) -> Result<Option<Snapshot>, Error> {
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

    /// Decode local file or remote URI offers, never inferring URIs from plain text.
    ///
    /// # Errors
    /// Rejects malformed copy/cut markers or invalid URI lists.
    pub(super) fn decode_uris(
        offer: &ClipboardMimeOffer,
    ) -> Result<Option<(Mode, Vec<String>)>, Error> {
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
            return Ok(Some((mode, Formats::parse_uri_list(uris)?)));
        }
        let Some(bytes) = Self::bytes(offer, URI_LIST) else {
            return Ok(None);
        };
        let mode = Self::bytes(offer, KDE_CUT)
            .map(Formats::kde_mode)
            .transpose()?
            .unwrap_or(Mode::Copy);
        let text = std::str::from_utf8(bytes).map_err(|e| Error::InvalidPayload(e.to_string()))?;
        Ok(Some((mode, Formats::parse_uri_list(text)?)))
    }
}
