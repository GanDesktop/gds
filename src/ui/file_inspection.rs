//! Shared asynchronous inspection cache for file previews and details.
use crate::service::file_inspection::{
    Error, Field, Inspection, Metadata, Request, Source, Thumbnail,
};
use gds::app::AppExt as _;
use gpui_kit::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub const DEFAULT_CACHE_ENTRIES: usize = 256;
pub const DEFAULT_CACHE_MIB: usize = 128;
pub const DEFAULT_WORKERS: usize = 2;

/// Configurable cache bounds and worker options for [`Cache`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    pub capacity: usize,
    pub byte_budget: usize,
    pub workers: usize,
    pub inspection: crate::service::file_inspection::Options,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CACHE_ENTRIES,
            byte_budget: DEFAULT_CACHE_MIB * 1024 * 1024,
            workers: DEFAULT_WORKERS,
            inspection: crate::service::file_inspection::Options::default(),
        }
    }
}

pub enum State<T> {
    NotRequested,
    Loading,
    Ready(Arc<T>),
    Unavailable,
    TooLarge,
    Failed(SharedString),
}

impl<T> Default for State<T> {
    fn default() -> Self {
        Self::NotRequested
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        match self {
            Self::NotRequested => Self::NotRequested,
            Self::Loading => Self::Loading,
            Self::Ready(value) => Self::Ready(value.clone()),
            Self::Unavailable => Self::Unavailable,
            Self::TooLarge => Self::TooLarge,
            Self::Failed(error) => Self::Failed(error.clone()),
        }
    }
}

impl<T> State<T> {
    fn request(&mut self, requested: bool) {
        if requested && matches!(self, Self::NotRequested) {
            *self = Self::Loading;
        }
    }

    fn from_field<U>(field: Field<U>, convert: impl FnOnce(U) -> Arc<T>) -> Self {
        match field {
            Field::NotRequested => Self::NotRequested,
            Field::Unavailable => Self::Unavailable,
            Field::Available(value) => Self::Ready(convert(value)),
            Field::Failed(Error::TooLarge) => Self::TooLarge,
            Field::Failed(error) => {
                tracing::debug!(%error, "file inspection unavailable");
                Self::Failed(error.to_string().into())
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub metadata: State<Metadata>,
    pub thumbnail: State<RenderImage>,
}

impl Snapshot {
    fn pending(&self) -> Option<Request> {
        match (
            matches!(self.metadata, State::Loading),
            matches!(self.thumbnail, State::Loading),
        ) {
            (true, true) => Some(Request::Both),
            (true, false) => Some(Request::Metadata),
            (false, true) => Some(Request::Thumbnail),
            (false, false) => None,
        }
    }

    fn from_inspection(inspection: Inspection) -> Self {
        Self {
            metadata: State::from_field(inspection.metadata, Arc::new),
            thumbnail: State::from_field(inspection.thumbnail, Cache::render_image),
        }
    }
}

struct Entry {
    snapshot: Snapshot,
    running: bool,
    touched: u64,
    ticket: u64,
    bytes: usize,
}

#[derive(Default)]
pub struct Cache {
    scope: Option<crate::service::task_manager::Owner>,
    entries: HashMap<Source, Entry>,
    pending: VecDeque<Source>,
    running: usize,
    clock: u64,
    bytes: usize,
    options: Options,
}

impl Cache {
    pub fn new(options: Options) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }

    pub fn set_options(&mut self, options: Options, cx: &mut Context<Self>) {
        if self.options == options {
            return;
        }
        self.options = options;
        self.trim(cx);
        self.pump(cx);
        cx.notify();
    }

    pub fn get(&self, source: &Source) -> Option<Snapshot> {
        self.entries.get(source).map(|entry| entry.snapshot.clone())
    }

    pub fn request(
        &mut self,
        source: Source,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Snapshot {
        if self.scope.is_none() {
            self.scope = Some(cx.gds().tasks().scope().named_child("File previews"));
        }
        if self.clock == 0 {
            cx.on_release(|cache, cx| {
                for (_, entry) in cache.entries.drain() {
                    if let State::Ready(image) = entry.snapshot.thumbnail {
                        cx.drop_image(image, None);
                    }
                }
            })
            .detach();
        }
        self.clock += 1;
        let entry = self.entries.entry(source.clone()).or_insert_with(|| Entry {
            snapshot: Snapshot::default(),
            running: false,
            touched: self.clock,
            ticket: self.clock,
            bytes: 0,
        });
        entry.touched = self.clock;
        entry.snapshot.metadata.request(request.metadata());
        entry.snapshot.thumbnail.request(request.thumbnail());
        let pending = !entry.running && entry.snapshot.pending().is_some();
        let snapshot = entry.snapshot.clone();
        if pending {
            self.enqueue_pending(source);
        }
        self.trim(cx);
        self.pump(cx);
        snapshot
    }

    fn trim(&mut self, cx: &mut App) {
        let capacity = self.options.capacity.max(1);
        let byte_budget = self.options.byte_budget.max(1);
        while self.entries.len() > capacity || self.bytes > byte_budget {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.bytes;
                if let State::Ready(image) = entry.snapshot.thumbnail {
                    cx.drop_image(image, None);
                }
            }
        }
        self.pending
            .retain(|source| self.entries.contains_key(source));
    }

    fn render_image(thumbnail: Thumbnail) -> Arc<RenderImage> {
        let mut pixels = thumbnail.into_rgba();
        for pixel in pixels.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
    }

    fn enqueue_pending(&mut self, source: Source) {
        self.pending.retain(|queued| queued != &source);
        self.pending.push_back(source);
    }

    fn next_pending(&mut self) -> Option<Source> {
        self.pending.pop_back()
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        let workers = self.options.workers.max(1);
        let inspection = self.options.inspection;
        while self.running < workers {
            let Some(source) = self.next_pending() else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&source).filter(|entry| !entry.running) else {
                continue;
            };
            let Some(request) = entry.snapshot.pending() else {
                continue;
            };
            entry.running = true;
            let ticket = entry.ticket;
            self.running += 1;
            let work_source = source.clone();
            let scope = self.scope.as_ref().expect("cache scope");
            let cancel = scope.token();
            let label = if scope.inspection_enabled() {
                work_source.path.display().to_string()
            } else {
                String::new()
            };
            let work = async move {
                Snapshot::from_inspection(
                    work_source.load_cancellable_with_options(request, inspection, &cancel),
                )
            };
            let Ok(work) = scope.track_named(
                &label,
                crate::service::task_manager::Execution::GpuiBackground,
                work,
            ) else {
                self.running -= 1;
                return;
            };
            let work = cx.background_executor().spawn(work);
            cx.spawn(async move |cache, cx| {
                let result = work.await;
                if let Err(error) = cache.update(cx, |cache, cx| {
                    cache.running -= 1;
                    if let Some(entry) = cache
                        .entries
                        .get_mut(&source)
                        .filter(|entry| entry.ticket == ticket)
                    {
                        entry.running = false;
                        if request.metadata() {
                            entry.snapshot.metadata = result.metadata;
                        }
                        if request.thumbnail() {
                            entry.snapshot.thumbnail = result.thumbnail;
                            if let State::Ready(image) = &entry.snapshot.thumbnail {
                                entry.bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                                cache.bytes += entry.bytes;
                            }
                        }
                        // A request for the other field may have arrived while this job ran.
                        if entry.snapshot.pending().is_some() {
                            cache.enqueue_pending(source);
                        }
                    }
                    cache.trim(cx);
                    cache.pump(cx);
                    cx.notify();
                }) {
                    tracing::debug!(%error, "inspection owner closed before completion");
                }
            })
            .detach();
        }
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
