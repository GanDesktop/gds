//! Direct child counting for the folder rows of the size column.
//!
//! Counting reads one directory's own entries, never its subtree, and stops at a
//! limit so a folder holding a huge number of children cannot stall a listing.
//! A directory that cannot be read, or whose entries cannot be enumerated,
//! reports no count instead of zero or a partial count.

use std::{fs, io, path::Path};

/// Direct children counted before the number is shown as a lower bound.
pub(super) const CHILD_COUNT_LIMIT: u32 = 1000;

/// Number of direct children of a directory, including hidden entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildCount {
    /// Counted children; at most [`CHILD_COUNT_LIMIT`].
    pub count: u32,
    /// The directory holds more children than `count`; the value is a lower bound.
    pub truncated: bool,
}

/// Count the direct children of `path`.
///
/// Returns `None` when the directory cannot be read or its entries cannot be
/// enumerated. A missing count is never shown or sorted as zero.
pub(super) fn count(path: &Path) -> Option<ChildCount> {
    count_with_limit(path, CHILD_COUNT_LIMIT)
}

/// Count with an explicit bound; exposed for the limit behaviour to be tested
/// without creating thousands of entries.
pub(super) fn count_with_limit(path: &Path, limit: u32) -> Option<ChildCount> {
    let entries = fs::read_dir(path).ok()?;
    count_entries(entries, limit)
}

/// Count the items an entry iterator yields successfully.
///
/// An iteration error means the directory content is unknown, not that one more
/// child exists, so it yields `None` instead of a count.
pub(super) fn count_entries<I, T>(entries: I, limit: u32) -> Option<ChildCount>
where
    I: IntoIterator<Item = io::Result<T>>,
{
    let mut count = 0;
    for entry in entries {
        entry.ok()?;
        count += 1;
        if count > limit {
            return Some(ChildCount {
                count: limit,
                truncated: true,
            });
        }
    }
    Some(ChildCount {
        count,
        truncated: false,
    })
}
