//! Project filesystem entries onto the shared content inspection service.
use super::{ContentCategory, Entry, EntryKind};
use gds::service::file_inspection::{Kind, Source};

impl Entry {
    pub fn inspection_source(&self) -> Source {
        let kind = match self.kind {
            EntryKind::Directory => Kind::Directory,
            EntryKind::SymbolicLink => Kind::Symlink,
            _ => match self
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
            path: self.path.clone(),
            kind,
            size: self.size,
            modified: self.modified,
        }
    }

    pub fn thumbnail_source(&self) -> Option<Source> {
        let source = self.inspection_source();
        (self.kind == EntryKind::File && source.kind.supports_thumbnail()).then_some(source)
    }
}
