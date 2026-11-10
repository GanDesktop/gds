//! Shared asynchronous inspection cache for file previews and details.
use crate::service::file_inspection::{
    Error, Field, Inspection, Metadata, Request, Source, Thumbnail,
};
use gpui_kit::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

const CAPACITY: usize = 256;
const BYTE_BUDGET: usize = 128 * 1024 * 1024;
const WORKERS: usize = 2;

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
    fn request(&mut self, requested: bool) -> bool {
        if requested && matches!(self, Self::NotRequested) {
            *self = Self::Loading;
            return true;
        }
        false
    }

    fn from_field<U>(field: Field<U>, convert: impl FnOnce(U) -> Result<Arc<T>, Error>) -> Self {
        let result = match field {
            Field::NotRequested => return Self::NotRequested,
            Field::Unavailable => return Self::Unavailable,
            Field::Failed(error) => Err(error),
            Field::Available(value) => convert(value),
        };
        match result {
            Ok(value) => Self::Ready(value),
            Err(Error::TooLarge) => Self::TooLarge,
            Err(error) => {
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
            metadata: State::from_field(inspection.metadata, |metadata| Ok(Arc::new(metadata))),
            thumbnail: State::from_field(inspection.thumbnail, Cache::decode),
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
    scope: Option<crate::task_manager::Owner>,
    entries: HashMap<Source, Entry>,
    pending: VecDeque<Source>,
    running: usize,
    clock: u64,
    bytes: usize,
}

impl Cache {
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
            self.scope = Some(crate::ui::task_manager::scope(cx).named_child("File previews"));
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
        let metadata = entry.snapshot.metadata.request(request.metadata());
        let thumbnail = entry.snapshot.thumbnail.request(request.thumbnail());
        let snapshot = entry.snapshot.clone();
        if (metadata || thumbnail) && !entry.running {
            self.pending.push_back(source);
        }
        self.trim(cx);
        self.pump(cx);
        snapshot
    }

    fn trim(&mut self, cx: &mut App) {
        while self.entries.len() > CAPACITY || self.bytes > BYTE_BUDGET {
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

    /// # Errors
    /// Reports malformed generated PNGs.
    fn decode(thumbnail: Thumbnail) -> Result<Arc<RenderImage>, Error> {
        let mut pixels =
            image::load_from_memory_with_format(&thumbnail.bytes, image::ImageFormat::Png)?
                .into_rgba8();
        for pixel in pixels.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Ok(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        while self.running < WORKERS {
            let Some(source) = self.pending.pop_front() else {
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
                Snapshot::from_inspection(work_source.load_cancellable(request, &cancel))
            };
            let Ok(work) =
                scope.track_named(&label, crate::task_manager::Execution::GpuiBackground, work)
            else {
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
                            cache.pending.push_back(source);
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
