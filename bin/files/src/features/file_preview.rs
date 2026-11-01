//! Project directory entries onto the shared image/video thumbnail service.
use crate::entities::file::directory::{ContentCategory, Entry, EntryKind};
use gds::service::thumbnail::{Kind, Source};

pub(crate) fn source(entry: &Entry) -> Option<Source> {
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
