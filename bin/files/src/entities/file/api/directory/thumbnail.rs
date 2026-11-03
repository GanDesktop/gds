//! Project directory entries onto the shared image/video thumbnail service.

use gds::service::thumbnail::{Kind, Source};

use super::{ContentCategory, Entry, EntryKind};

/// The thumbnail source for an entry that the shared service can preview.
///
/// Directories, links and non-media files have no source, so callers keep the
/// entry's icon instead of asking the cache for a thumbnail.
pub fn thumbnail_source(entry: &Entry) -> Option<Source> {
    if entry.kind != EntryKind::File {
        return None;
    }
    let kind = match entry.details.content.as_ref()?.category {
        ContentCategory::Image => Kind::Image,
        ContentCategory::Video => Kind::Video,
        _ => return None,
    };
    Some(Source {
        path: entry.path.clone(),
        kind,
        size: entry.size,
        modified: entry.modified,
    })
}
