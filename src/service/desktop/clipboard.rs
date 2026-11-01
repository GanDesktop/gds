//! Native Wayland clipboard reads, independent of GPUI and application payloads.
mod pipe;
#[cfg(test)]
mod tests;
mod wayland;

use std::{
    collections::HashSet,
    io,
    time::{Duration, Instant},
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Wayland clipboard is unavailable: {0}")]
    Wayland(#[from] wl_clipboard_rs::paste::Error),
    #[error("clipboard I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("clipboard source timed out")]
    Timeout,
    #[error("clipboard payload exceeds the supported size")]
    TooLarge,
    #[error("clipboard offer changed while reading")]
    Changed,
}

pub struct Clipboard {
    timeout: Duration,
    limit: usize,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new(DEFAULT_TIMEOUT, DEFAULT_LIMIT)
    }
}

impl Clipboard {
    pub fn new(timeout: Duration, limit: usize) -> Self {
        Self { timeout, limit }
    }

    /// Discover offered formats and start a reader with a shared payload deadline.
    /// Call from a background executor.
    ///
    /// # Errors
    /// Reports clipboard connection or format discovery failures.
    pub fn reader(&self) -> Result<Reader<'_>, Error> {
        let deadline = Instant::now() + self.timeout;
        let mime_types = self.mime_types()?;
        Ok(Reader {
            clipboard: self,
            mime_types,
            deadline,
        })
    }
}

/// Reads requested representations without decoding or selecting between formats.
/// The backend reconnects for each read; clipboard contents may change between calls.
pub struct Reader<'a> {
    clipboard: &'a Clipboard,
    mime_types: HashSet<String>,
    deadline: Instant,
}

impl Reader<'_> {
    pub fn mime_types(&self) -> &HashSet<String> {
        &self.mime_types
    }

    /// Read the raw bytes for an advertised format, or `None` if it was not offered.
    /// Each payload is bounded by the clipboard size limit; reads share one deadline.
    ///
    /// # Errors
    /// Reports unavailable advertised formats, transport, timeout or size-limit failures.
    pub fn read(&self, mime: &str) -> Result<Option<Vec<u8>>, Error> {
        if !self.mime_types.contains(mime) {
            return Ok(None);
        }
        self.clipboard
            .read_mime(mime, self.deadline)?
            .map(Some)
            .ok_or(Error::Changed)
    }
}
