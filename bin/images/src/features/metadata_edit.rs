//! Metadata validation, lossless container privacy stripping, and desktop tag persistence.

#[cfg(test)]
mod tests;

use std::{path::PathBuf, sync::Arc};

use gds::service::image::{
    Error,
    io::{SaveMode, atomic_save},
    metadata::{EditableFields, SanitizePolicy, rewrite_container_metadata},
    source::{FileStamp, Format, Origin},
};

use crate::entities::{
    document::DocumentState,
    metadata::{self, MetadataState},
};

/// Result of committing metadata + desktop tags to disk for a local image file.
pub struct MetadataCommitOutcome {
    pub updated_stamp: Option<FileStamp>,
    pub updated_tags: Vec<String>,
    pub rewritten_bytes: Option<Arc<[u8]>>,
}

/// Applies lossless metadata stripping (`SanitizePolicy::StripPrivacy`) directly on disk when
/// supported, or clears privacy-sensitive fields in the draft when pixels must be re-exported.
pub fn clear_privacy_in_draft(meta: &mut MetadataState) {
    meta.draft.artist = None;
    meta.draft.copyright = None;
    meta.draft.gps = None;
    meta.strip_privacy_on_save = true;
}

/// Parameters for committing metadata and extended-attribute tag updates on a blocking worker.
pub struct MetadataCommitRequest {
    pub path: PathBuf,
    pub expected_stamp: Option<FileStamp>,
    pub format: Format,
    pub source_bytes: Arc<[u8]>,
    pub edits: Option<EditableFields>,
    pub strip_privacy: bool,
    pub original_tags: Vec<String>,
    pub edited_tags: Vec<String>,
}

/// Persists edited metadata fields and `user.xdg.tags` for a local file without altering pixels
/// when the container supports lossless chunk rewriting.
///
/// # Errors
/// Returns [`Error::UnsupportedOperation`] when metadata changes or privacy stripping are
/// requested on a format that does not support lossless metadata rewrite, plus atomic save and
/// extended-attribute errors.
pub fn commit_metadata_and_tags_blocking(
    req: MetadataCommitRequest,
) -> Result<MetadataCommitOutcome, Error> {
    if (req.edits.is_some() || req.strip_privacy)
        && !req.format.capabilities().can_losslessly_rewrite_metadata
    {
        return Err(Error::UnsupportedOperation(
            "container format does not support lossless metadata rewrite",
        ));
    }

    let policy = if req.strip_privacy {
        SanitizePolicy::StripPrivacy
    } else {
        SanitizePolicy::PreserveWithEdits
    };

    let (updated_stamp, rewritten_bytes) = if req
        .format
        .capabilities()
        .can_losslessly_rewrite_metadata
        && (req.edits.is_some() || req.strip_privacy)
    {
        let rewritten =
            rewrite_container_metadata(&req.source_bytes, req.format, req.edits.as_ref(), policy)?;
        let stamp = atomic_save(
            &req.path,
            &rewritten,
            SaveMode::Overwrite {
                expected: req.expected_stamp,
            },
        )?;
        (Some(stamp), Some(Arc::from(rewritten)))
    } else {
        (None, None)
    };

    let updated_tags = if req.original_tags != req.edited_tags {
        metadata::save_tags(&req.path, &req.original_tags, &req.edited_tags).map_err(|source| {
            Error::Io {
                path: Some(req.path.clone()),
                source,
            }
        })?
    } else {
        metadata::load_tags(&req.path)
    };

    Ok(MetadataCommitOutcome {
        updated_stamp,
        updated_tags,
        rewritten_bytes,
    })
}

pub fn can_losslessly_commit_metadata(doc: &DocumentState) -> bool {
    matches!(doc.decoded.origin, Origin::LocalFile { .. })
        && !doc.has_unbaked_ops()
        && doc.format().capabilities().can_losslessly_rewrite_metadata
}
