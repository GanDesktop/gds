use super::{Clipboard, Error};
use std::{collections::HashSet, time::Instant};
use wl_clipboard_rs::paste::{self, ClipboardType, MimeType, Seat};

impl Clipboard {
    /// # Errors
    /// Reports unavailable data-control or failed MIME negotiation.
    pub(super) fn mime_types(&self) -> Result<HashSet<String>, Error> {
        match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
            Ok(types) => Ok(types),
            Err(
                paste::Error::ClipboardEmpty | paste::Error::NoMimeType | paste::Error::NoSeats,
            ) => Ok(HashSet::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Read one offered MIME with the caller's shared deadline.
    ///
    /// # Errors
    /// Reports protocol, I/O, timeout and payload-size failures.
    pub(super) fn read_mime(
        &self,
        mime: &str,
        deadline: Instant,
    ) -> Result<Option<Vec<u8>>, Error> {
        let (pipe, _) = match paste::get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            MimeType::Specific(mime),
        ) {
            Ok(result) => result,
            Err(paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        self.read_pipe(pipe, deadline).map(Some)
    }
}
