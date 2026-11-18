//! Window-owned inspection cache; decoding stays on supervised blocking workers.
use crate::service::{
    file_inspection::{Error, Field, Inspection, Metadata, Request, Source, Thumbnail},
    task_manager::{Owner as TaskOwner, Scope},
};
use anti_webui::{core::images::Bitmap, prelude::*};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::Arc,
};

pub const DEFAULT_CACHE_ENTRIES: usize = 256;
pub const DEFAULT_CACHE_MIB: usize = 128;
pub const DEFAULT_WORKERS: usize = 2;
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
            inspection: Default::default(),
        }
    }
}
#[derive(Default, PartialEq)]
pub enum State<T> {
    #[default]
    NotRequested,
    Loading,
    Ready(Arc<T>),
    Unavailable,
    TooLarge,
    Failed(String),
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
            Field::Failed(error) => Self::Failed(error.to_string()),
        }
    }
}
#[derive(Clone, Default)]
pub struct Snapshot {
    pub metadata: State<Metadata>,
    pub thumbnail: State<Bitmap>,
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
            thumbnail: State::from_field(inspection.thumbnail, Self::bitmap),
        }
    }
    fn bitmap(thumbnail: Thumbnail) -> Arc<Bitmap> {
        let image = thumbnail.into_rgba();
        let (width, height) = image.dimensions();
        let mut bytes = image.into_raw();
        let mut opaque = true;
        for pixel in bytes.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            opaque &= alpha == 255;
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        Arc::new(Bitmap::from_premultiplied(width, height, bytes, opaque))
    }
}
struct Entry {
    snapshot: Snapshot,
    work: Work,
    touched: u64,
    bytes: usize,
}
enum Work {
    Idle,
    Queued,
    Running { ticket: u64 },
}
struct Contents {
    entries: HashMap<Source, Entry>,
    pending: VecDeque<Source>,
    running: usize,
    clock: u64,
    bytes: usize,
    options: Options,
}
struct Completion {
    source: Source,
    ticket: u64,
    request: Request,
    outcome: Result<Snapshot, String>,
}
struct Controller {
    contents: RefCell<Contents>,
    scope: TaskOwner,
    sender: UiSender<Completion>,
    revision: Signal<u64>,
}
#[derive(Clone)]
pub struct Cache(Rc<Controller>);
impl Cache {
    pub fn new(scope: Scope, options: Options) -> Self {
        let scope = scope.named_child("File previews");
        Self(Rc::new_cyclic(|weak: &std::rc::Weak<Controller>| {
            let weak = weak.clone();
            let sender = ui_channel(move |completion| {
                if let Some(controller) = weak.upgrade() {
                    Cache(controller).complete(completion);
                }
            });
            Controller {
                contents: RefCell::new(Contents {
                    entries: HashMap::new(),
                    pending: VecDeque::new(),
                    running: 0,
                    clock: 0,
                    bytes: 0,
                    options,
                }),
                scope,
                sender,
                revision: signal(0),
            }
        }))
    }
    pub fn set_options(&self, options: Options) {
        {
            let mut state = self.0.contents.borrow_mut();
            state.options = options;
            state.trim();
        }
        self.pump();
        self.0
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }
    pub fn get(&self, source: &Source) -> Option<Snapshot> {
        self.0.revision.get();
        self.0
            .contents
            .borrow()
            .entries
            .get(source)
            .map(|entry| entry.snapshot.clone())
    }
    pub fn request(&self, source: Source, request: Request) -> Snapshot {
        self.0.revision.get();
        let snapshot = {
            let mut state = self.0.contents.borrow_mut();
            state.clock = state.clock.wrapping_add(1);
            let touched = state.clock;
            let entry = state
                .entries
                .entry(source.clone())
                .or_insert_with(|| Entry {
                    snapshot: Snapshot::default(),
                    work: Work::Idle,
                    touched,
                    bytes: 0,
                });
            entry.touched = touched;
            entry.snapshot.metadata.request(request.metadata());
            entry.snapshot.thumbnail.request(request.thumbnail());
            let enqueue = matches!(entry.work, Work::Idle) && entry.snapshot.pending().is_some();
            if enqueue {
                entry.work = Work::Queued;
            }
            let snapshot = entry.snapshot.clone();
            if enqueue {
                state.pending.push_back(source);
            }
            state.trim();
            snapshot
        };
        self.pump();
        snapshot
    }
    fn pump(&self) {
        loop {
            let job = {
                let mut state = self.0.contents.borrow_mut();
                if state.running >= state.options.workers.max(1) {
                    return;
                }
                let Some(source) = state.pending.pop_back() else {
                    return;
                };
                state.clock = state.clock.wrapping_add(1);
                let ticket = state.clock;
                let options = state.options.inspection;
                let Some(entry) = state.entries.get_mut(&source) else {
                    continue;
                };
                let Some(request) = entry.snapshot.pending() else {
                    entry.work = Work::Idle;
                    continue;
                };
                if !matches!(entry.work, Work::Queued) {
                    continue;
                }
                entry.work = Work::Running { ticket };
                state.running += 1;
                (source, ticket, request, options)
            };
            let (source, ticket, request, options) = job;
            let work_source = source.clone();
            let cancel = self.0.scope.token();
            let sender = self.0.sender.clone();
            self.0
                .scope
                .spawn_blocking_named("Inspect file", move || {
                    Snapshot::from_inspection(
                        work_source.load_cancellable_with_options(request, options, &cancel),
                    )
                })
                .on_complete(&self.0.scope, move |outcome| {
                    sender.send(Completion {
                        source,
                        ticket,
                        request,
                        outcome: outcome.map_err(|error| error.to_string()),
                    })
                });
        }
    }
    fn complete(&self, completion: Completion) {
        let Completion {
            source,
            ticket,
            request,
            outcome,
        } = completion;
        {
            let mut state = self.0.contents.borrow_mut();
            state.running = state.running.saturating_sub(1);
            let mut extra_bytes = 0;
            let mut queued = false;
            if let Some(entry) = state.entries.get_mut(&source)
                && matches!(entry.work, Work::Running { ticket: expected } if expected == ticket)
            {
                let result = outcome.unwrap_or_else(|error| Snapshot {
                    metadata: State::Failed(error.clone()),
                    thumbnail: State::Failed(error),
                });
                if request.metadata() {
                    entry.snapshot.metadata = result.metadata;
                }
                if request.thumbnail() {
                    entry.snapshot.thumbnail = result.thumbnail;
                    if let State::Ready(bitmap) = &entry.snapshot.thumbnail {
                        extra_bytes = bitmap.width() as usize * bitmap.height() as usize * 4;
                        entry.bytes = extra_bytes;
                    }
                }
                queued = entry.snapshot.pending().is_some();
                entry.work = if queued { Work::Queued } else { Work::Idle };
            }
            state.bytes += extra_bytes;
            if queued {
                state.pending.push_back(source);
            }
            state.trim();
        }
        self.pump();
        self.0
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }
}
impl Contents {
    fn trim(&mut self) {
        while self.entries.len() > self.options.capacity.max(1)
            || self.bytes > self.options.byte_budget.max(1)
        {
            let Some(key) = self
                .entries
                .iter()
                .filter(|(_, entry)| !matches!(entry.work, Work::Running { .. }))
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        self.pending
            .retain(|source| self.entries.contains_key(source));
    }
}
