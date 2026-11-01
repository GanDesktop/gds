//! Normalized desktop clipboard contents, independent of the conversation UI.

#[cfg(target_os = "linux")]
use gpui_kit::ImageFormat;
use gpui_kit::{ClipboardEntry, ClipboardItem, Image};
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
    Native(#[from] gds::service::desktop::clipboard::Error),
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
    /// # Errors
    /// Reports native clipboard access or invalid text representations.
    pub(crate) fn read_native() -> Result<Self, Error> {
        #[cfg(target_os = "linux")]
        {
            use strum::IntoEnumIterator;
            let clipboard = gds::service::desktop::clipboard::Clipboard::default();
            let reader = clipboard.reader()?;
            let mut contents = Self::default();
            if let Some(data) = reader.read("text/uri-list")? {
                contents.paths = Self::file_paths(std::str::from_utf8(&data)?);
            }
            if let Some(mime) = TEXT_TYPES
                .into_iter()
                .find(|mime| reader.mime_types().contains(*mime))
                && let Some(data) = reader.read(mime)?
            {
                contents.text = Some(std::str::from_utf8(&data)?.replace("\r\n", "\n"));
            }
            // File attachments take precedence over an alternative image representation.
            if contents.paths.is_empty()
                && let Some(format) = ImageFormat::iter()
                    .find(|format| reader.mime_types().contains(format.mime_type()))
                && let Some(data) = reader.read(format.mime_type())?
            {
                contents.images.push(Image::from_bytes(format, data));
            }
            Ok(contents)
        }
        #[cfg(target_os = "macos")]
        {
            macos::read().map_err(Error::from)
        }
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
