//! Decode, clipboard, portal and disk workflows.
use std::path::Path;

use gds::{
    service::{
        file_inspection::{Kind as InspectionKind, Request as InspectionRequest, Source},
        image::{
            self as engine,
            decode::{DecodeLimits, decode_origin, decode_preview},
            io::SaveMode,
            metadata::inspect_bytes,
        },
    },
    t_untracked,
    ui::file_inspection::State as InspectionState,
};

use super::{commands::Command, *};
use crate::{
    entities::{catalog, metadata},
    features::{
        desktop_actions::{self, PickMode, RenameError},
        export, metadata_edit,
    },
    shared::clipboard,
};

const PREFETCH_RADIUS: usize = 6;

pub(super) fn inspection_source(path: &Path, meta: &catalog::CatalogEntryMeta) -> Source {
    Source {
        path: path.to_path_buf(),
        kind: InspectionKind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: meta.bytes,
        modified: meta.modified,
    }
}

impl Page {
    pub(super) fn path(self) -> Option<PathBuf> {
        self.read(|s| s.doc.as_ref()?.decoded.origin.path().map(ToOwned::to_owned))
    }

    pub(super) fn open_path(self, path: PathBuf) {
        self.request(Pending::Open(Origin::LocalFile { path, stamp: None }));
    }

    pub(super) fn load(self, origin: Origin) {
        let w = self.window();
        let epoch = w.epoch.get() + 1;
        w.epoch.set(epoch);
        let cancel = w.scope.token().child_token();
        w.cancel.replace(cancel.clone()).cancel();
        w.loading.set(true);
        self.close_inline_edit();
        self.dismiss_popovers();
        self.changed();
        self.background(
            "Decode image",
            move || {
                let (image, warning) =
                    match decode_origin(&origin, DecodeLimits::default(), &cancel) {
                        Ok(image) => (image, None),
                        Err(
                            err @ (engine::Error::TooLarge { .. }
                            | engine::Error::DimensionsTooLarge { .. }),
                        ) => (
                            decode_preview(&origin, 2048, DecodeLimits::default(), &cancel)?,
                            Some(err.to_string()),
                        ),
                        Err(e) => return Err(e.into()),
                    };
                let tags = image
                    .origin
                    .path()
                    .map(metadata::load_tags)
                    .unwrap_or_default();
                let thumbnail = if image.origin.path().is_none() {
                    let pixels = image::imageops::thumbnail(&*image.rgba, 76, 76);
                    let mut png = std::io::Cursor::new(Vec::new());
                    pixels.write_to(&mut png, image::ImageFormat::Png)?;
                    Some(zgui_image::ImageBytes::new(png.into_inner()))
                } else {
                    None
                };
                Ok((image, tags, warning, thumbnail))
            },
            move |result| {
                if self.window().epoch.get() != epoch {
                    return;
                }
                self.window().loading.set(false);
                match result {
                    Ok((image, tags, warning, thumbnail)) => {
                        let path = image.origin.path().map(ToOwned::to_owned);
                        let meta = MetadataState::from_bundle(image.metadata.clone(), tags);
                        let scan = self.change(|s| {
                            let reuse = path
                                .as_deref()
                                .is_some_and(|target| s.catalog.select_path(target));
                            if !reuse {
                                s.catalog = CatalogState::default();
                            }
                            s.clear_document();
                            s.doc = Some(DocumentState::from_decoded(image));
                            s.metadata = Some(meta);
                            s.memory_thumbnail = thumbnail;
                            !reuse
                        });
                        let w = self.window();
                        w.clipboard_offer.replace(None);
                        w.clipboard_had_offer.set(false);
                        if let Some(error) = warning {
                            notifications::warning(t_untracked!(
                                "toast-preview-fallback",
                                error = error
                            ));
                        }
                        self.catalog_changed();
                        if scan {
                            self.scan(path);
                        } else {
                            self.prefetch_neighbor_thumbnails();
                        }
                        self.fit();
                        self.render();
                        self.focus();
                    }
                    Err(e) => self.error(t_untracked!("toast-load-failed", error = e.to_string())),
                }
                self.changed();
            },
        );
    }

    fn scan(self, path: Option<PathBuf>) {
        let Some(path) = path else { return };
        let w = self.window();
        let epoch = w.epoch.get();
        let cancel = w.cancel.borrow().clone();
        let target = path.clone();
        self.background(
            "Scan sibling images",
            move || Ok(catalog::scan_siblings(&target, &cancel)?),
            move |result| {
                if self.window().epoch.get() != epoch || self.path().as_ref() != Some(&path) {
                    return;
                }
                match result {
                    Ok(list) => {
                        self.change(|s| s.catalog = list);
                        self.catalog_changed();
                        self.prefetch_neighbor_thumbnails();
                    }
                    Err(e) => self.error(e),
                }
            },
        );
    }

    pub(super) fn prefetch_neighbor_thumbnails(self) {
        let sources = self.read(|s| {
            let Some(center) = s.catalog.current_index else {
                return Vec::new();
            };
            if s.catalog.entries.is_empty() {
                return Vec::new();
            }
            // Enqueue farthest neighbors first so the cache's LIFO queue pops nearest items first.
            let mut sources = Vec::with_capacity(PREFETCH_RADIUS * 2 + 1);
            for offset in (1..=PREFETCH_RADIUS).rev() {
                if let Some(entry) = s.catalog.entry_at(center + offset) {
                    sources.push(inspection_source(&entry.path, &entry.meta));
                }
                if let Some(prev_idx) = center.checked_sub(offset)
                    && let Some(entry) = s.catalog.entry_at(prev_idx)
                {
                    sources.push(inspection_source(&entry.path, &entry.meta));
                }
            }
            if let Some(entry) = s.catalog.entry_at(center) {
                sources.push(inspection_source(&entry.path, &entry.meta));
            }
            sources
        });
        let cache = &self.window().cache;
        for source in sources {
            let already_ready = cache
                .peek(&source)
                .is_some_and(|snap| matches!(snap.thumbnail, InspectionState::Ready(_)));
            if !already_ready {
                let _ = cache.request(&source, InspectionRequest::Thumbnail);
            }
        }
    }

    pub(super) fn probe_clipboard(self) {
        let w = self.window();
        if w.clipboard_probing.replace(true) {
            return;
        }
        let epoch = w.epoch.get();
        self.local(async move {
            struct ClearProbing(Rc<Workspace>);
            impl Drop for ClearProbing {
                fn drop(&mut self) {
                    self.0.clipboard_probing.set(false);
                }
            }
            let _guard = ClearProbing(Rc::clone(&w));
            let offer = clipboard::probe_offer(w.scope.clone()).await;
            if w.epoch.get() != epoch || self.read(|s| s.doc.is_some()) {
                return;
            }
            let changed = {
                let mut slot = w.clipboard_offer.borrow_mut();
                if *slot != offer {
                    *slot = offer.clone();
                    true
                } else {
                    false
                }
            };
            if offer.is_some() {
                w.clipboard_had_offer.set(true);
            } else if changed {
                w.clipboard_had_offer.set(false);
            }
            if changed {
                self.changed();
            }
        });
    }

    pub(super) fn open_clipboard(self) {
        let w = self.window();
        w.clipboard_had_offer.set(false);
        let epoch = w.epoch.get();
        self.local(async move {
            let origin = clipboard::read_origin(w.scope.clone()).await;
            if w.epoch.get() != epoch {
                return;
            }
            match origin {
                Ok(Some(origin)) => self.request(Pending::Open(origin)),
                Ok(None) => self.error(text("clipboard-empty")),
                Err(e) => self.error(t_untracked!(
                    "toast-clipboard-open-failed",
                    error = e.to_string()
                )),
            }
        });
    }

    pub(super) fn pick(self, command: Command) {
        let w = self.window();
        if w.busy.get_untracked() {
            return;
        }
        let directory = self.read(|s| match s.doc.as_ref().map(|d| &d.decoded.origin) {
            Some(Origin::LocalFile { path, .. }) => path.parent().map(ToOwned::to_owned),
            Some(Origin::Memory { .. }) => w
                .workflow
                .output
                .as_deref()
                .and_then(Path::parent)
                .map(ToOwned::to_owned),
            None => None,
        });
        let mode = match command {
            Command::Open => PickMode::OpenImage { directory },
            Command::OpenFolder => PickMode::OpenFolder { directory },
            Command::SaveAs => {
                let Some(suggested_name) =
                    self.read(|s| s.doc.as_ref().map(export::suggested_save_filename))
                else {
                    return;
                };
                PickMode::SaveImage {
                    suggested_name,
                    directory,
                }
            }
            _ => return,
        };
        let is_folder = command == Command::OpenFolder;
        let is_save = command == Command::SaveAs;
        let title = command.label();
        let task = w
            .scope
            .spawn_named("Choose image path", desktop_actions::pick_path(title, mode));
        let epoch = w.epoch.get();
        self.local(async move {
            let result = task
                .await
                .map_err(anyhow::Error::from)
                .and_then(std::convert::identity);
            if w.epoch.get() != epoch {
                return;
            }
            match result {
                Ok(Some(dir)) if is_folder => {
                    let cancel = w.cancel.borrow().clone();
                    self.background(
                        "Open image folder",
                        move || Ok(catalog::first_in_folder(&dir, &cancel)?),
                        move |found| {
                            if self.window().epoch.get() != epoch {
                                return;
                            }
                            match found {
                                Ok(Some(first)) => self.open_path(first),
                                Ok(None) => notifications::info(text("empty-folder")),
                                Err(error) => self.error(error),
                            }
                        },
                    );
                }
                Ok(Some(path)) if is_save => self.save_to(path),
                Ok(Some(path)) => self.open_path(path),
                Ok(None) => {}
                Err(e) => self.error(e),
            }
        });
    }

    pub(super) fn copy(self) {
        self.flush_text();
        self.cancel_gesture();
        let w = self.window();
        let Some(plan) = self.read(|s| {
            Some(export::build_clipboard_plan(
                s.doc.as_ref()?,
                s.metadata.as_ref(),
                &w.prefs.snapshot(),
            ))
        }) else {
            return;
        };
        w.busy.set(true);
        self.background(
            "Encode clipboard image",
            move || Ok(export::execute_clipboard_blocking(plan)?),
            move |result| {
                self.local(async move {
                    let result = match result {
                        Ok(bytes) => clipboard::write_png(bytes).await,
                        Err(e) => Err(e),
                    };
                    w.busy.set(false);
                    match result {
                        Ok(()) => {
                            self.notice(text("toast-copy-success"));
                            if w.workflow.early_exit {
                                Gds::current().tasks().request_quit();
                            }
                        }
                        Err(e) => {
                            self.error(t_untracked!("toast-copy-failed", error = e.to_string()));
                        }
                    }
                });
            },
        );
    }

    pub(super) fn save(self) {
        let w = self.window();
        let target = self.read(|s| {
            let doc = s.doc.as_ref()?;
            if doc.decoded.is_preview_downscaled {
                return None;
            }
            match &doc.decoded.origin {
                Origin::LocalFile { path, .. } if doc.format().capabilities().can_encode => {
                    Some(path.clone())
                }
                Origin::Memory { .. } => w.workflow.output.clone(),
                Origin::LocalFile { .. } => None,
            }
        });
        if let Some(path) = target {
            self.save_to(path);
        } else {
            self.pick(Command::SaveAs);
        }
    }

    fn save_to(self, path: PathBuf) {
        let w = self.window();
        if w.busy.get_untracked() || w.loading.get_untracked() {
            return;
        }
        self.flush_text();
        self.cancel_gesture();
        let Some(plan) = self.read(|s| {
            let doc = s.doc.as_ref()?;
            let mode = match &doc.decoded.origin {
                Origin::LocalFile { path: old, stamp } if old == &path => SaveMode::Overwrite {
                    expected: stamp.clone(),
                },
                _ => SaveMode::SaveAs,
            };
            Some(export::build_save_plan(
                path.clone(),
                mode,
                doc,
                s.metadata.as_ref()?,
                &w.prefs.snapshot(),
            ))
        }) else {
            return;
        };
        w.busy.set(true);
        self.background(
            "Save image",
            move || {
                let out = export::execute_save_blocking(plan)?;
                let bundle = inspect_bytes(&out.source_bytes, out.format, Some(out.stamp.size))?;
                Ok((out, bundle))
            },
            move |result| {
                w.busy.set(false);
                match result {
                    Ok((out, bundle)) => {
                        let cached = self.change(|s| {
                            let Some(doc) = &mut s.doc else { return false };
                            let cached = s.catalog.update_entry_meta(
                                &path,
                                out.stamp.size,
                                out.stamp.modified,
                            ) && s.catalog.select_path(&path);
                            if let Some(rgba) = out.composited_rgba {
                                doc.apply_saved_pixels(rgba);
                                s.history.clear();
                                s.tool.selection.clear();
                                s.tool.staged_crop = None;
                            } else {
                                doc.mark_clean();
                            }
                            s.memory_thumbnail = None;
                            doc.decoded.origin = Origin::LocalFile {
                                path: path.clone(),
                                stamp: Some(out.stamp),
                            };
                            doc.decoded.source_bytes = out.source_bytes;
                            doc.decoded.format = out.format;
                            doc.decoded.metadata = bundle.clone();
                            s.metadata = Some(MetadataState::from_bundle(bundle, out.saved_tags));
                            cached
                        });
                        if cached {
                            self.catalog_changed();
                        } else {
                            self.scan(Some(path.clone()));
                        }
                        self.render();
                        self.changed();
                        self.notice(t_untracked!(
                            "toast-save-success",
                            path = path.display().to_string()
                        ));
                        if w.workflow.copy_on_save {
                            self.copy();
                        } else if w.workflow.early_exit {
                            Gds::current().tasks().request_quit();
                        }
                    }
                    Err(e) => match e.downcast_ref::<engine::Error>() {
                        Some(engine::Error::ExternalConflict(path)) => self.error(t_untracked!(
                            "toast-save-conflict",
                            path = path.display().to_string()
                        )),
                        _ => self.error(t_untracked!("toast-save-failed", error = e.to_string())),
                    },
                }
            },
        );
    }

    pub(super) fn rename(self, name: String) {
        let w = self.window();
        let Some(old) = self.path() else { return };
        self.close_rename();
        w.busy.set(true);
        self.background(
            "Rename image",
            move || Ok(desktop_actions::rename(&old, &name)),
            move |result| {
                w.busy.set(false);
                match result {
                    Ok(Ok(Some(path))) => {
                        self.change(|s| {
                            if let Some(doc) = &mut s.doc
                                && let Origin::LocalFile { path: source, .. } =
                                    &mut doc.decoded.origin
                            {
                                if let Some(meta) = s.catalog.meta_for_path(source) {
                                    let meta = catalog::CatalogEntryMeta::new(
                                        &path,
                                        meta.bytes,
                                        meta.modified,
                                    );
                                    s.catalog.rename_path(source, path.clone(), meta);
                                }
                                source.clone_from(&path);
                            }
                        });
                        if self.read(|s| s.catalog.current_path() != Some(&path)) {
                            self.scan(Some(path));
                        } else {
                            self.catalog_changed();
                        }
                        self.changed();
                        self.notice(text("rename-done"));
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(RenameError::InvalidName)) => {
                        notifications::warning(text("rename-error-invalid"));
                    }
                    Ok(Err(RenameError::AlreadyExists(existing))) => {
                        notifications::warning(t_untracked!(
                            "toast-rename-exists",
                            name = existing
                        ));
                    }
                    Ok(Err(RenameError::Io(e))) => {
                        self.error(t_untracked!("toast-rename-failed", error = e.to_string()));
                    }
                    Err(e) => {
                        self.error(t_untracked!("toast-rename-failed", error = e.to_string()));
                    }
                }
                self.focus();
            },
        );
    }

    pub(super) fn trash(self, path: PathBuf) {
        let w = self.window();
        w.busy.set(true);
        let source = path.clone();
        self.background(
            "Move image to Trash",
            move || Ok(trash::delete(source)?),
            move |result| {
                w.busy.set(false);
                match result {
                    Ok(()) => {
                        let next = self.change(|s| {
                            let next = s.catalog.remove_path(&path);
                            s.clear_document();
                            next
                        });
                        w.epoch.set(w.epoch.get() + 1);
                        self.close_inline_edit();
                        self.catalog_changed();
                        self.changed();
                        self.repaint();
                        if let Some(next) = next {
                            self.load(Origin::LocalFile {
                                path: next,
                                stamp: None,
                            });
                        } else {
                            w.clipboard_had_offer.set(false);
                            self.probe_clipboard();
                        }
                        self.notice(t_untracked!(
                            "toast-trash-success",
                            path = path.display().to_string()
                        ));
                    }
                    Err(e) => self.error(t_untracked!("toast-trash-failed", error = e.to_string())),
                }
            },
        );
    }

    pub(super) fn reveal_in_files(self) {
        let Some(path) = self.path() else { return };
        let job = self
            .window()
            .scope
            .spawn_named("Reveal image in file manager", async move {
                gds::service::desktop::file_manager::show_items(&[path], "").await?;
                anyhow::Ok(())
            });
        self.local(async move {
            if let Err(e) = job
                .await
                .map_err(anyhow::Error::from)
                .and_then(std::convert::identity)
            {
                self.error(t_untracked!("error-reveal-failed", error = e.to_string()));
            }
        });
    }

    pub(super) fn open_with_chooser(self) {
        let Some(path) = self.path() else { return };
        let job = self
            .window()
            .scope
            .spawn_named("Open image with chooser", async move {
                use gds::service::desktop::{Desktop, OpenMode};
                Desktop::default()
                    .open_file(&path, OpenMode::ChooseApplication)
                    .await?;
                anyhow::Ok(())
            });
        self.local(async move {
            if let Err(e) = job
                .await
                .map_err(anyhow::Error::from)
                .and_then(std::convert::identity)
            {
                self.error(t_untracked!(
                    "error-open-with-failed",
                    error = e.to_string()
                ));
            }
        });
    }

    pub(super) fn strip(self) {
        self.flush_text();
        self.cancel_gesture();
        let request = self.change(|s| {
            let (doc, meta) = (s.doc.as_ref()?, s.metadata.as_mut()?);
            if metadata_edit::can_losslessly_commit_metadata(doc)
                && let Origin::LocalFile { path, stamp } = &doc.decoded.origin
            {
                return Some(metadata_edit::MetadataCommitRequest {
                    path: path.clone(),
                    expected_stamp: stamp.clone(),
                    format: doc.format(),
                    source_bytes: doc.decoded.source_bytes.clone(),
                    edits: None,
                    strip_privacy: true,
                    original_tags: meta.original_tags.clone(),
                    edited_tags: meta.tags.clone(),
                });
            }
            metadata_edit::clear_privacy_in_draft(meta);
            None
        });
        let Some(req) = request else {
            if self.read(|s| s.doc.is_some() && s.metadata.is_some()) {
                self.changed();
                self.notice(text("toast-metadata-draft-cleared"));
            }
            return;
        };
        let path = req.path.clone();
        self.window().busy.set(true);
        self.background(
            "Strip image metadata",
            move || {
                let fmt = req.format;
                let out = metadata_edit::commit_metadata_and_tags_blocking(req)?;
                let bytes = out
                    .rewritten_bytes
                    .expect("lossless privacy rewrite returns encoded bytes");
                let bundle = inspect_bytes(&bytes, fmt, Some(bytes.len() as u64))?;
                Ok((out.updated_stamp, out.updated_tags, bytes, bundle))
            },
            move |result| {
                self.window().busy.set(false);
                match result {
                    Ok((stamp, tags, bytes, bundle)) => {
                        let byte_len = stamp.as_ref().map_or(bytes.len() as u64, |s| s.size);
                        let modified = stamp.as_ref().and_then(|s| s.modified);
                        let updated = self.change(|s| {
                            let updated = s.catalog.update_entry_meta(&path, byte_len, modified);
                            if let Some(doc) = &mut s.doc
                                && let Origin::LocalFile {
                                    path: current_path,
                                    stamp: current_stamp,
                                } = &mut doc.decoded.origin
                                && *current_path == path
                            {
                                *current_stamp = stamp;
                                doc.decoded.source_bytes = bytes;
                                doc.decoded.metadata = bundle.clone();
                                s.metadata = Some(MetadataState::from_bundle(bundle, tags));
                            }
                            updated
                        });
                        if updated {
                            self.catalog_changed();
                        }
                        self.changed();
                        self.notice(text("toast-metadata-cleared"));
                    }
                    Err(e) => match e.downcast_ref::<engine::Error>() {
                        Some(engine::Error::ExternalConflict(conflict_path)) => {
                            self.error(t_untracked!(
                                "toast-save-conflict",
                                path = conflict_path.display().to_string()
                            ));
                        }
                        _ => {
                            self.error(t_untracked!(
                                "toast-metadata-failed",
                                error = e.to_string()
                            ));
                        }
                    },
                }
            },
        );
    }
}
