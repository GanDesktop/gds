//! File entity presentation: icons, artwork, and formatted metadata labels.

mod icon;
mod presentation;

pub(crate) use icon::{FileArtwork, FileIcon};
pub(crate) use presentation::{EntryPresentation, OwnerLabel, UNKNOWN_VALUE, format_size};
