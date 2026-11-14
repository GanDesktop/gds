//! Direct child counting for the folder rows of the size column.
//!
//! Counting reads one directory's own entries, never its subtree, and stops at a
//! limit so a folder holding a huge number of children cannot stall a listing.
//! A directory that cannot be read, or whose entries cannot be enumerated,
//! reports no count instead of zero or a partial count.

use std::{fs, io, path::Path};

/// Direct children counted before the number is shown as a lower bound.
pub const DEFAULT_CHILD_COUNT_LIMIT: u32 = 1000;

/// Number of direct children of a directory, including hidden entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildCount {
    /// Counted children; at most the configured limit (default [`DEFAULT_CHILD_COUNT_LIMIT`]).
    pub count: u32,
    /// The directory holds more children than `count`; the value is a lower bound.
    pub truncated: bool,
}

impl ChildCount {
    /// Count the direct children of `path` using [`DEFAULT_CHILD_COUNT_LIMIT`].
    ///
    /// Returns `None` when the directory cannot be read or its entries cannot be
    /// enumerated. A missing count is never shown or sorted as zero.
    #[allow(dead_code)]
    pub(super) fn from_path(path: &Path) -> Option<Self> {
        Self::from_path_with_limit(path, DEFAULT_CHILD_COUNT_LIMIT)
    }

    pub(super) fn from_path_with_limit(path: &Path, limit: u32) -> Option<Self> {
        Self::count_with_limit(path, limit.max(1))
    }

    fn count_with_limit(path: &Path, limit: u32) -> Option<Self> {
        let entries = fs::read_dir(path).ok()?;
        Self::count_entries(entries, limit)
    }

    /// Count the items an entry iterator yields successfully.
    ///
    /// An iteration error means the directory content is unknown, not that one more
    /// child exists, so it yields `None` instead of a count.
    fn count_entries<I, T>(entries: I, limit: u32) -> Option<Self>
    where
        I: IntoIterator<Item = io::Result<T>>,
    {
        let mut count = 0;
        for entry in entries {
            entry.ok()?;
            count += 1;
            if count > limit {
                return Some(Self {
                    count: limit,
                    truncated: true,
                });
            }
        }
        Some(Self {
            count,
            truncated: false,
        })
    }
}
