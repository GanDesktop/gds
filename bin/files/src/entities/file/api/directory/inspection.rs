//! Project filesystem entries onto the shared content inspection service.
use super::{ContentCategory, Entry, EntryKind};
use gds::service::file_inspection::{Kind, Source};

pub fn inspection_source(entry: &Entry) -> Source {
    let kind = match entry.kind {
        EntryKind::Directory => Kind::Directory,
        EntryKind::SymbolicLink => Kind::Symlink,
        _ => match entry
            .details
            .content
            .as_ref()
            .map(|content| content.category)
        {
            Some(ContentCategory::Image) => Kind::Image,
            Some(ContentCategory::Audio) => Kind::Audio,
            Some(ContentCategory::Video) => Kind::Video,
            Some(ContentCategory::Text | ContentCategory::Code | ContentCategory::Document) => {
                Kind::Text
            }
            _ => Kind::Other,
        },
    };
    Source {
        path: entry.path.clone(),
        kind,
        size: entry.size,
        modified: entry.modified,
    }
}

pub fn thumbnail_source(entry: &Entry) -> Option<Source> {
    let source = inspection_source(entry);
    (entry.kind == EntryKind::File && source.kind.supports_thumbnail()).then_some(source)
}
