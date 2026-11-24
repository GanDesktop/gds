//! Sibling folder catalog scan, farthest-first thumbnail prefetch, and next/prev navigation.

use std::{io::ErrorKind, path::PathBuf};

use gds::{
    service::file_inspection::{Kind as InspectionKind, Request as InspectionRequest, Source},
    ui::file_inspection::State as InspectionState,
};
use gpui_kit::{Context, Window};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::entities::catalog::{CatalogEntry, scan_siblings};

const PREFETCH_RADIUS: usize = 6;

impl WorkspacePage {
    pub(in crate::pages::workspace::ui) fn start_sibling_scan(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if let Some(cancel) = self.session.scan_cancel.take() {
            cancel.cancel();
        }
        self.session.scan_generation = self.session.scan_generation.wrapping_add(1);
        let scan_gen = self.session.scan_generation;
        let cancel = CancellationToken::new();
        self.session.scan_cancel = Some(cancel.clone());

        let work = self
            .tasks
            .spawn_blocking_named("Scan folder images", move || scan_siblings(&path, &cancel));
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.session.scan_generation != scan_gen {
                    return;
                }
                this.session.scan_cancel = None;
                match result {
                    Ok(Ok(mut catalog)) => {
                        if let Some(gds::service::image::source::Origin::LocalFile {
                            path, ..
                        }) = this.session.document.as_ref().map(|d| &d.decoded.origin)
                        {
                            catalog.select_path(path);
                        }
                        this.session.catalog = catalog;
                        this.prefetch_neighbor_thumbnails(cx);
                        cx.notify();
                    }
                    Ok(Err(err)) if err.kind() == ErrorKind::Interrupted => {}
                    Ok(Err(err)) => {
                        tracing::warn!(error = %err, "sibling catalog scan failed");
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "sibling catalog worker aborted");
                    }
                }
            });
        })
        .detach();
    }

    pub(in crate::pages::workspace::ui) fn prefetch_neighbor_thumbnails(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let Some(center) = self.session.catalog.current_index else {
            return;
        };
        if self.session.catalog.entries.is_empty() {
            return;
        }

        // Enqueue farthest neighbors first so InspectionCache's LIFO queue pops nearest items first.
        let mut sources = Vec::with_capacity(PREFETCH_RADIUS * 2 + 1);
        for offset in (1..=PREFETCH_RADIUS).rev() {
            if let Some(entry) = self.session.catalog.entry_at(center + offset) {
                sources.push(entry_inspection_source(entry));
            }
            if let Some(prev_idx) = center.checked_sub(offset)
                && let Some(entry) = self.session.catalog.entry_at(prev_idx)
            {
                sources.push(entry_inspection_source(entry));
            }
        }
        if let Some(entry) = self.session.catalog.entry_at(center) {
            sources.push(entry_inspection_source(entry));
        }

        self.runtime.inspections.update(cx, |cache, cx| {
            for source in sources {
                let already_ready = cache
                    .get(&source)
                    .is_some_and(|snap| matches!(snap.thumbnail, InspectionState::Ready(_)));
                if !already_ready {
                    cache.request(source, InspectionRequest::Thumbnail, cx);
                }
            }
        });
    }

    pub(in crate::pages::workspace::ui) fn navigate_sibling(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = if delta < 0 {
            self.session.catalog.peek_prev()
        } else {
            self.session.catalog.peek_next()
        };
        let Some(target) = target else {
            return;
        };
        self.open_local_with_confirm(target, window, cx);
    }

    pub(in crate::pages::workspace::ui) fn navigate_edge(
        &mut self,
        to_last: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = if to_last {
            self.session.catalog.last()
        } else {
            self.session.catalog.first()
        };
        let Some(target) = target else {
            return;
        };
        if self.session.catalog.current_path() == Some(&target) {
            return;
        }
        self.open_local_with_confirm(target, window, cx);
    }

    pub(in crate::pages::workspace::ui) fn open_local_with_confirm(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.runtime.is_loading
            && let Some(doc) = &self.session.document
            && let Origin::LocalFile { path: active, .. } = &doc.decoded.origin
            && active == &path
        {
            return;
        }
        self.confirm_discard_changes(window, cx, move |this, window, cx| {
            this.open_local_path(path, window, cx);
        });
    }
}

pub(in crate::pages::workspace::ui) fn entry_inspection_source(entry: &CatalogEntry) -> Source {
    Source {
        path: entry.path.clone(),
        kind: InspectionKind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: entry.meta.bytes,
        modified: entry.meta.modified,
    }
}
