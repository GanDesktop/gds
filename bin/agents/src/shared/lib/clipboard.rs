//! Normalized desktop clipboard contents, independent of the conversation UI.

#[cfg(target_os = "linux")]
use gpui_kit::ImageFormat;
use gpui_kit::{App, ClipboardEntry, ClipboardItem, Image, Task};
use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(test, target_os = "linux"))]
mod tests;

#[cfg(target_os = "linux")]
const TEXT_TYPES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "text/plain;charset=UTF-8",
    "UTF8_STRING",
    "text/plain",
];

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Native(#[from] gpui_kit::ClipboardAccessError),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Platform(#[from] gpui_kit::ClipboardReadError),
    #[cfg(target_os = "macos")]
    #[error(transparent)]
    Native(#[from] macos::Error),
    #[cfg(target_os = "linux")]
    #[error("clipboard contains invalid UTF-8")]
    InvalidText(#[from] std::str::Utf8Error),
}

#[derive(Clone, Default)]
pub(crate) struct Contents {
    pub text: Option<String>,
    pub images: Vec<Image>,
    pub paths: Vec<PathBuf>,
}

impl Contents {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn read(cx: &mut App) -> Task<Result<Self, Error>> {
        #[cfg(target_os = "linux")]
        {
            use strum::IntoEnumIterator;
            if !cx.supports_clipboard_mime() {
                let read = cx.read_from_clipboard_async();
                return cx.spawn(async move |_| Ok(Self::from_item(read.await?)));
            }
            let types = ["text/uri-list", "x-special/gnome-copied-files"]
                .into_iter()
                .chain(TEXT_TYPES)
                .chain(ImageFormat::iter().map(|format| format.mime_type()))
                .map(str::to_owned)
                .collect();
            let read = cx.read_clipboard_mime(types, 16 * 1024 * 1024);
            cx.spawn(async move |_| Self::from_mime(read.await?))
        }
        #[cfg(target_os = "macos")]
        {
            cx.background_executor()
                .spawn(async { macos::read().map_err(Error::from) })
        }
    }

    #[cfg(target_os = "linux")]
    fn from_mime(offer: Option<gpui_kit::ClipboardMimeOffer>) -> Result<Self, Error> {
        use strum::IntoEnumIterator;
        let Some(offer) = offer else {
            return Ok(Self::default());
        };
        let bytes = |mime: &str| {
            offer
                .entries
                .iter()
                .find(|entry| entry.mime_type == mime)
                .map(|entry| entry.bytes.as_slice())
        };
        let mut contents = Self::default();
        if let Some(data) = bytes("text/uri-list") {
            contents.paths = Self::file_paths(std::str::from_utf8(data)?);
        } else if let Some(data) = bytes("x-special/gnome-copied-files") {
            if let Some((_, uris)) = std::str::from_utf8(data)?.split_once('\n') {
                contents.paths = Self::file_paths(uris);
            }
        }
        if let Some(data) = TEXT_TYPES.into_iter().find_map(bytes) {
            contents.text = Some(std::str::from_utf8(data)?.replace("\r\n", "\n"));
        }
        if contents.paths.is_empty() {
            if let Some((format, data)) = ImageFormat::iter()
                .find_map(|format| bytes(format.mime_type()).map(|data| (format, data)))
            {
                contents
                    .images
                    .push(Image::from_bytes(format, data.to_vec()));
            }
        }
        Ok(contents)
    }

    pub fn from_item(item: Option<ClipboardItem>) -> Self {
        let Some(item) = item else {
            return Self::default();
        };
        let mut contents = Self {
            text: item.text().map(|text| text.replace("\r\n", "\n")),
            ..Self::default()
        };
        for entry in item.entries {
            match entry {
                ClipboardEntry::Image(image) => contents.images.push(image),
                ClipboardEntry::ExternalPaths(paths) => {
                    for path in paths.paths() {
                        if !contents.paths.contains(path) {
                            contents.paths.push(path.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        contents
    }

    pub fn has_attachments(&self) -> bool {
        !self.paths.is_empty() || !self.images.is_empty()
    }

    #[cfg(target_os = "linux")]
    fn file_paths(text: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let Ok(uri) = url::Url::parse(line) else {
                continue;
            };
            if uri.scheme() != "file" || uri.host_str().is_some_and(|host| host != "localhost") {
                continue;
            }
            if let Ok(path) = uri.to_file_path()
                && !paths.contains(&path)
            {
                paths.push(path);
            }
        }
        paths
    }
}
