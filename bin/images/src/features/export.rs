//! Export, atomic save, Save As prompting, and Wayland clipboard serialization.

#[cfg(test)]
mod tests;

use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use gds::service::{
    file_tags,
    image::{
        Error,
        encode::{EncodeOptions, encode_rgba},
        io::{SaveMode, atomic_save},
        metadata::{EditableFields, SanitizePolicy, rewrite_container_metadata},
        ops::{apply_transform, composite_ops},
        source::{FileStamp, Format, Origin},
    },
};
use image::RgbaImage;

use crate::entities::{
    document::{AnnotationItem, DocumentState, TransformState},
    metadata::{self, MetadataState},
    preferences::Preferences,
};

/// Pixel source passed to background export/clipboard workers so stale render caches never
/// skip unbaked transforms or redaction annotations.
#[derive(Clone)]
pub enum PixelSource {
    Ready(Arc<RgbaImage>),
    NeedsComposite {
        base: Arc<RgbaImage>,
        transform: TransformState,
        annotations: Vec<AnnotationItem>,
    },
}

impl PixelSource {
    /// Captures the exact pixel state for `doc`, reusing `RenderCache.complete` only when
    /// `complete.revision == doc.edit_revision`, or sharing `doc.decoded.rgba` via `Arc::clone`
    /// when no edits exist.
    pub fn from_document(doc: &DocumentState) -> Self {
        if !doc.has_unbaked_ops() {
            return Self::Ready(Arc::clone(&doc.decoded.rgba));
        }
        if let Some(cached) = doc.cache.complete_at(doc.edit_revision) {
            return Self::Ready(Arc::clone(cached));
        }
        Self::NeedsComposite {
            base: Arc::clone(&doc.decoded.rgba),
            transform: doc.transform,
            annotations: doc.annotations.clone(),
        }
    }

    /// Evaluates pending transform/annotation ops if needed and returns the RGBA8 pixel buffer.
    ///
    /// # Errors
    /// Returns transform or raster compositing errors.
    pub fn materialize_blocking(self) -> Result<Arc<RgbaImage>, Error> {
        match self {
            Self::Ready(rgba) => Ok(rgba),
            Self::NeedsComposite {
                base,
                transform,
                annotations,
            } => {
                let mut rgba = if transform.is_identity() {
                    (*base).clone()
                } else {
                    apply_transform(&base, &transform.spec)?
                };
                if !annotations.is_empty() {
                    let (cw, ch) = rgba.dimensions();
                    let ops: Vec<_> = annotations
                        .iter()
                        .filter_map(|item| item.to_raster_op(cw, ch))
                        .collect();
                    composite_ops(&mut rgba, &ops)?;
                }
                Ok(Arc::new(rgba))
            }
        }
    }
}

/// Desktop `user.xdg.tags` synchronization plan applied once an image file write succeeds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagSavePlan {
    pub same_file_overwrite: bool,
    pub tags_dirty: bool,
    pub original_tags: Vec<String>,
    pub edited_tags: Vec<String>,
}

/// Pure-CPU save plan executed on a blocking background worker thread.
pub enum SavePlan {
    LosslessMetadataRewrite {
        path: PathBuf,
        expected_stamp: Option<FileStamp>,
        save_mode: SaveMode,
        format: Format,
        source_bytes: Arc<[u8]>,
        edits: EditableFields,
        policy: SanitizePolicy,
        tags: TagSavePlan,
    },
    PixelEncodeSave {
        path: PathBuf,
        save_mode: SaveMode,
        format: Format,
        pixels: PixelSource,
        existing_exif: Option<Vec<u8>>,
        edits: EditableFields,
        jpeg_quality: u8,
        policy: SanitizePolicy,
        tags: TagSavePlan,
    },
}

/// Output produced by [`execute_save_blocking`], avoiding duplicate file re-reads on the caller.
pub struct SaveOutcome {
    pub stamp: FileStamp,
    pub source_bytes: Arc<[u8]>,
    pub format: Format,
    pub composited_rgba: Option<Arc<RgbaImage>>,
    pub saved_tags: Vec<String>,
}

/// Pure-CPU clipboard PNG encoding plan executed on a blocking background worker thread.
pub struct ClipboardPlan {
    pub pixels: PixelSource,
    pub existing_exif: Option<Vec<u8>>,
    pub edits: Option<EditableFields>,
    pub strip_privacy: bool,
}

/// Builds a concrete [`SavePlan`] choosing lossless container rewrite over pixel re-encoding
/// whenever there are no unbaked transforms/annotations and the container supports lossless edits.
pub fn build_save_plan(
    target_path: PathBuf,
    save_mode: SaveMode,
    doc: &DocumentState,
    meta: &MetadataState,
    prefs: &Preferences,
) -> SavePlan {
    let same_file_overwrite = matches!(
        &doc.decoded.origin,
        Origin::LocalFile { path, .. } if *path == target_path
    );
    let target_format = Format::from_path(&target_path)
        .filter(|fmt| fmt.capabilities().can_encode)
        .unwrap_or_else(|| {
            if doc.format().capabilities().can_encode {
                doc.format()
            } else {
                Format::Png
            }
        });

    let tags = TagSavePlan {
        same_file_overwrite,
        tags_dirty: meta.tags_dirty,
        original_tags: meta.original_tags.clone(),
        edited_tags: meta.tags.clone(),
    };

    let edits = meta.draft.normalized();
    if same_file_overwrite
        && !doc.has_unbaked_ops()
        && target_format == doc.format()
        && doc.format().capabilities().can_losslessly_rewrite_metadata
    {
        return SavePlan::LosslessMetadataRewrite {
            path: target_path,
            expected_stamp: match &doc.decoded.origin {
                Origin::LocalFile { stamp, .. } => stamp.clone(),
                Origin::Memory { .. } => None,
            },
            save_mode,
            format: doc.format(),
            source_bytes: Arc::clone(&doc.decoded.source_bytes),
            edits,
            policy: if meta.strip_privacy_on_save {
                SanitizePolicy::StripPrivacy
            } else {
                SanitizePolicy::PreserveWithEdits
            },
            tags,
        };
    }

    SavePlan::PixelEncodeSave {
        path: target_path,
        save_mode,
        format: target_format,
        pixels: PixelSource::from_document(doc),
        existing_exif: doc.decoded.metadata.raw_exif.clone(),
        edits,
        jpeg_quality: prefs.default_jpeg_quality,
        policy: if meta.strip_privacy_on_save {
            SanitizePolicy::StripPrivacy
        } else {
            SanitizePolicy::KeepSafeOnPixelExport
        },
        tags,
    }
}

/// Builds a [`ClipboardPlan`] capturing current pixels (or pending compositing) for `Ctrl+C`.
pub fn build_clipboard_plan(
    doc: &DocumentState,
    meta: Option<&MetadataState>,
    prefs: &Preferences,
) -> ClipboardPlan {
    ClipboardPlan {
        pixels: PixelSource::from_document(doc),
        existing_exif: doc.decoded.metadata.raw_exif.clone(),
        edits: meta.map(|m| m.draft.normalized()),
        strip_privacy: prefs.strip_privacy_on_copy || meta.is_some_and(|m| m.strip_privacy_on_save),
    }
}

/// Executes a [`SavePlan`] synchronously (intended for `Scope::spawn_blocking_named`).
///
/// # Errors
/// Returns transform/composite, codec, container rewrite, conflict, symlink, read-only, or I/O errors.
pub fn execute_save_blocking(plan: SavePlan) -> Result<SaveOutcome, Error> {
    match plan {
        SavePlan::LosslessMetadataRewrite {
            path,
            expected_stamp,
            save_mode,
            format,
            source_bytes,
            edits,
            policy,
            tags,
        } => {
            let bytes = rewrite_container_metadata(&source_bytes, format, Some(&edits), policy)?;
            let effective_mode = match save_mode {
                SaveMode::Overwrite { expected } => SaveMode::Overwrite {
                    expected: expected.or(expected_stamp),
                },
                SaveMode::SaveAs => SaveMode::SaveAs,
            };
            let stamp = atomic_save(&path, &bytes, effective_mode)?;
            let saved_tags = sync_tags_after_save(&path, &tags)?;
            Ok(SaveOutcome {
                stamp,
                source_bytes: Arc::from(bytes),
                format,
                composited_rgba: None,
                saved_tags,
            })
        }
        SavePlan::PixelEncodeSave {
            path,
            save_mode,
            format,
            pixels,
            existing_exif,
            edits,
            jpeg_quality,
            policy,
            tags,
        } => {
            let rgba = pixels.materialize_blocking()?;
            let options = EncodeOptions {
                format,
                jpeg_quality,
                png_fast: false,
                existing_exif: existing_exif.as_deref(),
                metadata_edits: Some(&edits),
                sanitize_policy: policy,
                jpeg_matte: [255, 255, 255],
            };
            let bytes = encode_rgba(&rgba, &options)?;
            let stamp = atomic_save(&path, &bytes, save_mode)?;
            let saved_tags = sync_tags_after_save(&path, &tags)?;
            Ok(SaveOutcome {
                stamp,
                source_bytes: Arc::from(bytes),
                format,
                composited_rgba: Some(rgba),
                saved_tags,
            })
        }
    }
}

fn sync_tags_after_save(path: &Path, tags: &TagSavePlan) -> Result<Vec<String>, Error> {
    if tags.same_file_overwrite {
        if tags.tags_dirty && tags.original_tags != tags.edited_tags {
            return metadata::save_tags(path, &tags.original_tags, &tags.edited_tags).map_err(
                |source| Error::Io {
                    path: Some(path.to_path_buf()),
                    source,
                },
            );
        }
        return Ok(metadata::load_tags(path));
    }

    if tags.tags_dirty {
        file_tags::write(path, &tags.edited_tags).map_err(|source| Error::Io {
            path: Some(path.to_path_buf()),
            source,
        })?;
        return file_tags::read(path).map_err(|source| Error::Io {
            path: Some(path.to_path_buf()),
            source,
        });
    }

    if !tags.edited_tags.is_empty() {
        match file_tags::write(path, &tags.edited_tags) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::Unsupported => {}
            Err(source) => {
                return Err(Error::Io {
                    path: Some(path.to_path_buf()),
                    source,
                });
            }
        }
    }

    Ok(metadata::load_tags(path))
}

/// Executes a [`ClipboardPlan`] synchronously (intended for `Scope::spawn_blocking_named`).
///
/// # Errors
/// Returns compositing or PNG/EXIF encoding errors.
pub fn execute_clipboard_blocking(plan: ClipboardPlan) -> Result<Vec<u8>, Error> {
    let rgba = plan.pixels.materialize_blocking()?;
    encode_clipboard_png_blocking(
        &rgba,
        plan.existing_exif.as_deref(),
        plan.edits.as_ref(),
        plan.strip_privacy,
    )
}

/// Encodes composited pixels as PNG bytes formatted for system clipboard publication.
///
/// # Errors
/// Returns encoding errors from PNG compression or EXIF serialization.
pub fn encode_clipboard_png_blocking(
    rgba: &RgbaImage,
    existing_exif: Option<&[u8]>,
    edits: Option<&EditableFields>,
    strip_privacy: bool,
) -> Result<Vec<u8>, Error> {
    let policy = if strip_privacy {
        SanitizePolicy::StripPrivacy
    } else {
        SanitizePolicy::KeepSafeOnPixelExport
    };
    let options = EncodeOptions {
        format: Format::Png,
        jpeg_quality: 92,
        png_fast: true,
        existing_exif,
        metadata_edits: edits,
        sanitize_policy: policy,
        jpeg_matte: [255, 255, 255],
    };
    encode_rgba(rgba, &options)
}

pub fn suggested_save_filename(doc: &DocumentState) -> String {
    match &doc.decoded.origin {
        Origin::LocalFile { path, .. } => {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "image".to_owned());
            let ext = if doc.format().capabilities().can_encode {
                doc.format().default_extension()
            } else {
                Format::Png.default_extension()
            };
            format!("{stem}.{ext}")
        }
        Origin::Memory { .. } => "screenshot.png".to_owned(),
    }
}
