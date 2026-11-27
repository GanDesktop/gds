//! Draft state holding inspected container metadata, 7 editable EXIF/PNG fields, and desktop tags.

use gds::service::image::metadata::{EditableFields, MetadataBundle};

#[derive(Clone, Debug)]
pub struct MetadataState {
    pub bundle: MetadataBundle,
    pub draft: EditableFields,
    pub original_tags: Vec<String>,
    pub tags: Vec<String>,
    pub strip_privacy_on_save: bool,
    pub tags_dirty: bool,
}

impl MetadataState {
    pub fn from_bundle(bundle: MetadataBundle, tags: Vec<String>) -> Self {
        let draft = bundle.editable.clone();
        Self {
            bundle,
            draft,
            original_tags: tags.clone(),
            tags,
            strip_privacy_on_save: false,
            tags_dirty: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.strip_privacy_on_save || self.tags_dirty
    }
}
