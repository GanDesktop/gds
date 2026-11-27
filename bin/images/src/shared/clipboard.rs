//! Desktop MIME clipboard used by the viewer and screenshot workflows.

#[cfg(test)]
mod tests;

use std::{path::PathBuf, sync::Arc};

use gds::service::{
    image::source::{Format, MIME_TYPES, MemoryOrigin, Origin},
    task_manager::Scope,
};
use zgui::platform::{ClipboardKind, ClipboardMime, ClipboardMimeOffer};
use zgui::prelude::try_use_clipboard;

const URI_LIST: &str = "text/uri-list";
const GNOME_FILES: &str = "x-special/gnome-copied-files";
const MAX_URI_LIST_BYTES: usize = 1024 * 1024;
const MAX_IMAGE_CLIPBOARD_BYTES: usize = 128 * 1024 * 1024;

/// Lightweight descriptor of what the clipboard currently holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardOfferKind {
    LocalImageFile(PathBuf),
    RawImageMime(Format),
}

/// Probes the current clipboard selection without retaining raw image bytes in memory.
pub async fn probe_offer(tasks: Scope) -> Option<ClipboardOfferKind> {
    let clip = try_use_clipboard()?;
    let offer = clip
        .read_mime(
            ClipboardKind::Standard,
            requested_mime_types(),
            MAX_IMAGE_CLIPBOARD_BYTES,
        )
        .await
        .ok()??;
    offer_from_mime_offer(offer, &tasks).await
}

/// Reads an image [`Origin`] from the system clipboard, preferring existing local image URIs.
///
/// # Errors
/// Returns an error if the clipboard service is unavailable or MIME reading fails.
pub async fn read_origin(tasks: Scope) -> anyhow::Result<Option<Origin>> {
    let clip = try_use_clipboard().ok_or_else(|| anyhow::anyhow!("Clipboard unavailable"))?;
    let Some(offer) = clip
        .read_mime(
            ClipboardKind::Standard,
            requested_mime_types(),
            MAX_IMAGE_CLIPBOARD_BYTES,
        )
        .await?
    else {
        return Ok(None);
    };
    Ok(origin_from_mime_offer(offer, &tasks).await)
}

/// Publishes PNG-encoded image bytes to the system clipboard.
///
/// # Errors
/// Returns an error if the clipboard service is unavailable or refuses the write.
pub async fn write_png(bytes: Vec<u8>) -> anyhow::Result<()> {
    let clip = try_use_clipboard().ok_or_else(|| anyhow::anyhow!("Clipboard unavailable"))?;
    clip.write_mime(
        ClipboardKind::Standard,
        vec![ClipboardMime::new(Format::Png.mime_type(), bytes)],
        None,
    )
    .await?;
    Ok(())
}

fn requested_mime_types() -> Vec<String> {
    let mut types = Vec::with_capacity(2 + MIME_TYPES.len());
    types.push(GNOME_FILES.to_owned());
    types.push(URI_LIST.to_owned());
    types.extend(MIME_TYPES.iter().map(|mime| (*mime).to_owned()));
    types
}

async fn first_existing_file(candidates: Vec<PathBuf>, tasks: &Scope) -> Option<PathBuf> {
    if candidates.is_empty() {
        return None;
    }
    tasks
        .spawn_blocking_named("clipboard-verify-local-files", move || {
            candidates
                .into_iter()
                .find(|path| path.is_file() && Format::from_path(path).is_some())
        })
        .await
        .ok()
        .flatten()
}

async fn offer_from_mime_offer(
    offer: ClipboardMimeOffer,
    tasks: &Scope,
) -> Option<ClipboardOfferKind> {
    let candidates = extract_candidate_local_image_paths(&offer);
    let raw_format = first_raw_mime_format(&offer);
    // Drop the clipboard payload before awaiting blocking file checks so large image buffers
    // are never held in memory during probing.
    drop(offer);
    if let Some(path) = first_existing_file(candidates, tasks).await {
        return Some(ClipboardOfferKind::LocalImageFile(path));
    }
    raw_format.map(ClipboardOfferKind::RawImageMime)
}

async fn origin_from_mime_offer(offer: ClipboardMimeOffer, tasks: &Scope) -> Option<Origin> {
    let candidates = extract_candidate_local_image_paths(&offer);
    if let Some(path) = first_existing_file(candidates, tasks).await {
        return Some(Origin::local(path));
    }
    let (format, bytes) = select_raw_mime_entry(&offer)?;
    Some(Origin::Memory {
        kind: MemoryOrigin::Clipboard,
        bytes: Arc::from(bytes),
        format_hint: Some(format),
    })
}

fn select_raw_mime_entry(offer: &ClipboardMimeOffer) -> Option<(Format, &[u8])> {
    for mime in MIME_TYPES {
        if let Some(entry) = offer.entries.iter().find(|entry| entry.mime_type == *mime)
            && !entry.bytes.is_empty()
            && let Some(format) = Format::from_mime(&entry.mime_type)
        {
            return Some((format, entry.bytes.as_slice()));
        }
    }
    None
}

fn first_raw_mime_format(offer: &ClipboardMimeOffer) -> Option<Format> {
    select_raw_mime_entry(offer).map(|(format, _)| format)
}

fn extract_candidate_local_image_paths(offer: &ClipboardMimeOffer) -> Vec<PathBuf> {
    if let Some(gnome) = offer
        .entries
        .iter()
        .find(|entry| entry.mime_type == GNOME_FILES)
    {
        return parse_gnome_files_uris(&gnome.bytes)
            .map(parse_supported_uris)
            .unwrap_or_default();
    }

    if let Some(uri_list) = offer
        .entries
        .iter()
        .find(|entry| entry.mime_type == URI_LIST)
        && uri_list.bytes.len() <= MAX_URI_LIST_BYTES
        && let Ok(text) = std::str::from_utf8(&uri_list.bytes)
    {
        return parse_supported_uris(text);
    }

    Vec::new()
}

fn parse_gnome_files_uris(bytes: &[u8]) -> Option<&str> {
    if bytes.len() > MAX_URI_LIST_BYTES {
        return None;
    }
    let (intent, rest) = std::str::from_utf8(bytes).ok()?.split_once('\n')?;
    let intent = intent.strip_suffix('\r').unwrap_or(intent);
    matches!(intent, "copy" | "cut").then_some(rest)
}

fn parse_supported_uris(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(local_file_uri)
        .filter(|path| Format::from_path(path).is_some())
        .collect()
}

fn local_file_uri(text: &str) -> Option<PathBuf> {
    let absolute = text
        .split_once(':')
        .is_some_and(|(_, rest)| rest.starts_with('/'));
    if text.as_bytes().contains(&0) || !absolute {
        return None;
    }
    let uri = url::Url::parse(text).ok()?;
    if uri.scheme() != "file"
        || uri.host_str().is_some_and(|host| host != "localhost")
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.port().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return None;
    }
    uri.to_file_path().ok()
}
