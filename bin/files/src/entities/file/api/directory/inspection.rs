//! Project filesystem entries onto the shared content inspection service.
use super::{ContentCategory, Entry, EntryKind, content};
use gds::service::file_inspection::{Kind, Source};

impl Entry {
    pub fn inspection_source(&self) -> Source {
        let kind = match self.kind {
            EntryKind::Directory => Kind::Directory,
            EntryKind::SymbolicLink => Kind::Symlink,
            _ => match self.details.content.as_ref() {
                Some(content) if content.category == ContentCategory::Image => Kind::Image,
                Some(content) if content.category == ContentCategory::Audio => Kind::Audio,
                Some(content) if content.category == ContentCategory::Video => Kind::Video,
                Some(content) if content.text_previewable => Kind::Text,
                _ => Kind::Other,
            },
        };
        let mime = self
            .details
            .content
            .as_ref()
            .and_then(|content| content.mime.clone());
        let mime_ancestors = mime
            .as_deref()
            .map(content::mime_ancestors)
            .unwrap_or_default();
        Source {
            path: self.path.clone(),
            kind,
            mime,
            mime_ancestors,
            size: self.size,
            modified: self.modified,
        }
    }

    pub fn thumbnail_source(&self, cache: &gds::ui::file_inspection::Cache) -> Option<Source> {
        if self.kind != EntryKind::File {
            return None;
        }
        let source = self.inspection_source();
        cache.supports_thumbnail(&source).then_some(source)
    }
}
