//! Shared asynchronous inspection cache for file previews and details.

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use tokio_util::sync::CancellationToken;
use zgui::reactive::{LocalStorage, Owner, RwSignal, Task, is_ui_thread, prelude::*, spawn_local};
use zgui_image::ImageBytes;

use crate::app::Gds;
use crate::service::file_inspection::{Error, Field, Metadata, Request, Source, Thumbnail};
use crate::service::task_manager::{Execution, Scope};

pub const DEFAULT_CACHE_ENTRIES: usize = 256;
pub const DEFAULT_CACHE_MIB: usize = 128;
pub const DEFAULT_WORKERS: usize = 2;

/// Configurable cache bounds and worker options for [`Cache`].
#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Asynchronous lookup state for file metadata or thumbnail previews.
#[derive(Clone, Default)]
pub enum State<T> {
    #[default]
    NotRequested,
    Loading,
    Ready(Arc<T>),
    Unavailable,
    TooLarge,
    Failed(Arc<str>),
}

/// Cached inspection snapshot for one file source.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub metadata: State<Metadata>,
    pub thumbnail: State<ImageBytes>,
}

#[derive(Clone, Default)]
struct CacheEntry {
    snapshot: Snapshot,
    bytes: usize,
}

struct PendingJob {
    source: Source,
    request: Request,
}

enum EncodedThumbnail {
    NotRequested,
    Available(Vec<u8>),
    Unavailable,
    TooLarge,
    Failed(Arc<str>),
}

struct WorkerOutcome {
    metadata: State<Metadata>,
    thumbnail: EncodedThumbnail,
}

struct Contents {
    options: Options,
    scope: Option<Scope>,
    owner: Option<Owner>,
    revision: Option<RwSignal<u64, LocalStorage>>,
    entries: HashMap<Source, CacheEntry>,
    lru: VecDeque<Source>,
    pending: Vec<PendingJob>,
    running: HashMap<Source, CancellationToken>,
    tasks: HashMap<Source, Task>,
    bytes: usize,
    generation: u64,
}

impl Contents {
    fn new(
        options: Options,
        scope: Option<Scope>,
        owner: Option<Owner>,
        revision: Option<RwSignal<u64, LocalStorage>>,
    ) -> Self {
        Self {
            options,
            scope,
            owner,
            revision,
            entries: HashMap::new(),
            lru: VecDeque::new(),
            pending: Vec::new(),
            running: HashMap::new(),
            tasks: HashMap::new(),
            bytes: 0,
            generation: 0,
        }
    }

    fn touch_lru(&mut self, source: &Source) {
        if let Some(pos) = self.lru.iter().position(|existing| existing == source) {
            self.lru.remove(pos);
        }
        self.lru.push_back(source.clone());
    }

    fn trim(&mut self, protect: Option<&Source>) {
        while self.entries.len() > self.options.capacity || self.bytes > self.options.byte_budget {
            let candidate = self.lru.iter().position(|key| {
                !self.running.contains_key(key) && protect.is_none_or(|p| p != key)
            });
            let Some(pos) = candidate else {
                break;
            };
            let Some(evict_key) = self.lru.remove(pos) else {
                break;
            };
            if let Some(removed) = self.entries.remove(&evict_key) {
                self.bytes = self.bytes.saturating_sub(removed.bytes);
            }
        }
    }

    fn ensure_revision(&mut self) -> Option<RwSignal<u64, LocalStorage>> {
        if self.revision.is_none() && is_ui_thread() {
            if self.owner.is_none() {
                self.owner = Owner::current();
            }
            self.revision = Some(match &self.owner {
                Some(owner) => owner.with(|| RwSignal::new_local(0)),
                None => RwSignal::new_local(0),
            });
        }
        self.revision
    }
}

/// Reactive inspection and thumbnail cache backed by a bounded off-thread worker pool.
#[derive(Clone)]
pub struct Cache {
    inner: Rc<RefCell<Contents>>,
}

impl Default for Cache {
    fn default() -> Self {
        let owner = is_ui_thread().then(Owner::current).flatten();
        Self {
            inner: Rc::new(RefCell::new(Contents::new(
                Options::default(),
                None,
                owner,
                None,
            ))),
        }
    }
}

impl Cache {
    /// Creates a cache configured with `options`.
    #[must_use]
    pub fn new(options: Options) -> Self {
        let owner = is_ui_thread().then(Owner::current).flatten();
        let revision = is_ui_thread().then(|| match &owner {
            Some(o) => o.with(|| RwSignal::new_local(0)),
            None => RwSignal::new_local(0),
        });
        Self {
            inner: Rc::new(RefCell::new(Contents::new(options, None, owner, revision))),
        }
    }

    /// Creates a cache reporting active blocking jobs into task-manager `scope`.
    #[must_use]
    pub fn with_scope(scope: Scope, options: Options) -> Self {
        let owner = is_ui_thread().then(Owner::current).flatten();
        let revision = is_ui_thread().then(|| match &owner {
            Some(o) => o.with(|| RwSignal::new_local(0)),
            None => RwSignal::new_local(0),
        });
        Self {
            inner: Rc::new(RefCell::new(Contents::new(
                options,
                Some(scope),
                owner,
                revision,
            ))),
        }
    }

    /// Returns the currently active cache options.
    #[must_use]
    pub fn options(&self) -> Options {
        self.inner.borrow().options.clone()
    }

    /// Updates cache options, trims excess cached entries, and schedules queued jobs if needed.
    pub fn set_options(&self, options: Options) {
        let revision = {
            let mut contents = self.inner.borrow_mut();
            contents.options = options;
            contents.trim(None);
            contents.ensure_revision()
        };
        Self::notify(revision);
        self.pump();
    }

    /// Returns a cached snapshot for `source` without enqueueing new inspection work.
    #[must_use]
    pub fn peek(&self, source: &Source) -> Option<Snapshot> {
        let revision = self.inner.borrow_mut().ensure_revision();
        if let Some(rev) = revision {
            rev.track();
        }
        let mut contents = self.inner.borrow_mut();
        let snap = contents
            .entries
            .get(source)
            .map(|entry| entry.snapshot.clone());
        if snap.is_some() {
            contents.touch_lru(source);
        }
        snap
    }

    /// Returns whether `source` can produce a thumbnail via built-in decoders or active external
    /// thumbnailers configured on this cache.
    #[must_use]
    pub fn supports_thumbnail(&self, source: &Source) -> bool {
        source.supports_thumbnail(&self.inner.borrow().options.inspection)
    }

    /// Returns the reactive snapshot for `(source, request)`, scheduling background inspection when
    /// any requested field has not yet been fetched.
    #[must_use]
    pub fn snapshot(&self, source: &Source, request: Request) -> Snapshot {
        self.request(source, request)
    }

    /// Requests inspection/thumbnail fields for `source` and returns its reactive [`Snapshot`].
    #[must_use]
    pub fn request(&self, source: &Source, request: Request) -> Snapshot {
        let revision = self.inner.borrow_mut().ensure_revision();
        if let Some(rev) = revision {
            rev.track();
        }

        let (snapshot, need_pump) = {
            let mut contents = self.inner.borrow_mut();
            if contents.options.capacity == 0 || contents.options.workers == 0 {
                let existing = contents
                    .entries
                    .get(source)
                    .map(|entry| entry.snapshot.clone())
                    .unwrap_or_default();
                return existing;
            }

            let existing = contents
                .entries
                .get(source)
                .map(|entry| entry.snapshot.clone());
            let mut next_snapshot = existing.unwrap_or_default();
            let missing_meta =
                request.metadata() && matches!(next_snapshot.metadata, State::NotRequested);
            let mut missing_thumb =
                request.thumbnail() && matches!(next_snapshot.thumbnail, State::NotRequested);

            if !source.supports_thumbnail(&contents.options.inspection) && missing_thumb {
                next_snapshot.thumbnail = State::Unavailable;
                missing_thumb = false;
            }

            let pending_request = combine_missing_request(missing_meta, missing_thumb);
            if let Some(needed) = pending_request {
                if missing_meta {
                    next_snapshot.metadata = State::Loading;
                }
                if missing_thumb {
                    next_snapshot.thumbnail = State::Loading;
                }
                let entry = contents.entries.entry(source.clone()).or_default();
                entry.snapshot = next_snapshot.clone();
                contents.touch_lru(source);
                contents.trim(Some(source));

                if !contents.running.contains_key(source) {
                    enqueue_lifo(&mut contents.pending, source.clone(), needed);
                }
                (next_snapshot, true)
            } else {
                if contents.entries.contains_key(source) {
                    contents.touch_lru(source);
                }
                (next_snapshot, false)
            }
        };

        if need_pump {
            self.pump();
        }
        snapshot
    }

    /// Invalidates all cached entries for `path`.
    pub fn invalidate(&self, path: &Path) {
        let (revision, cancelled_tasks) = {
            let mut contents = self.inner.borrow_mut();
            contents.generation = contents.generation.saturating_add(1);
            contents.pending.retain(|job| job.source.path != path);

            let matching_keys: Vec<Source> = contents
                .entries
                .keys()
                .filter(|key| key.path == path)
                .cloned()
                .collect();
            for key in &matching_keys {
                if let Some(entry) = contents.entries.remove(key) {
                    contents.bytes = contents.bytes.saturating_sub(entry.bytes);
                }
            }
            contents.lru.retain(|key| key.path != path);

            let mut tasks = Vec::new();
            let running_keys: Vec<Source> = contents
                .running
                .keys()
                .filter(|key| key.path == path)
                .cloned()
                .collect();
            for key in running_keys {
                if let Some(token) = contents.running.remove(&key) {
                    token.cancel();
                }
                if let Some(task) = contents.tasks.remove(&key) {
                    tasks.push(task);
                }
            }

            (contents.revision, tasks)
        };
        for task in cancelled_tasks {
            task.cancel();
        }
        Self::notify(revision);
    }

    /// Clears all cached entries and cancels in-flight background work.
    pub fn clear(&self) {
        let (revision, cancelled_tasks) = {
            let mut contents = self.inner.borrow_mut();
            contents.generation = contents.generation.saturating_add(1);
            contents.pending.clear();
            contents.entries.clear();
            contents.lru.clear();
            contents.bytes = 0;

            for (_, token) in contents.running.drain() {
                token.cancel();
            }
            let tasks: Vec<Task> = contents.tasks.drain().map(|(_, task)| task).collect();
            (contents.revision, tasks)
        };
        for task in cancelled_tasks {
            task.cancel();
        }
        Self::notify(revision);
    }

    /// Number of entries currently resident in the cache.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.inner.borrow().entries.len()
    }

    /// Total encoded thumbnail byte footprint held in the cache.
    #[must_use]
    pub fn byte_usage(&self) -> usize {
        self.inner.borrow().bytes
    }

    /// Number of queued jobs waiting for a worker slot.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.inner.borrow().pending.len()
    }

    /// Number of jobs currently executing on worker threads.
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.inner.borrow().running.len()
    }

    fn notify(revision: Option<RwSignal<u64, LocalStorage>>) {
        if let Some(rev) = revision {
            let _ = rev.try_update(|count| *count = count.saturating_add(1));
        }
    }

    fn pump(&self) {
        struct RunningGuard {
            inner: Weak<RefCell<Contents>>,
            key: Option<Source>,
            cancel: CancellationToken,
        }
        impl Drop for RunningGuard {
            fn drop(&mut self) {
                let Some(key) = self.key.take() else {
                    return;
                };
                self.cancel.cancel();
                if let Some(inner) = self.inner.upgrade()
                    && let Ok(mut contents) = inner.try_borrow_mut()
                {
                    contents.running.remove(&key);
                    let _ = contents.tasks.remove(&key);
                    if let Some(entry) = contents.entries.get_mut(&key) {
                        if matches!(entry.snapshot.metadata, State::Loading) {
                            entry.snapshot.metadata = State::NotRequested;
                        }
                        if matches!(entry.snapshot.thumbnail, State::Loading) {
                            entry.snapshot.thumbnail = State::NotRequested;
                        }
                    }
                }
            }
        }

        loop {
            let next = {
                let mut contents = self.inner.borrow_mut();
                if contents.owner.is_none() && is_ui_thread() {
                    contents.owner = Owner::current();
                }
                if contents.running.len() >= contents.options.workers {
                    None
                } else if let Some(job) = contents.pending.pop() {
                    let token = CancellationToken::new();
                    contents.running.insert(job.source.clone(), token.clone());
                    Some((
                        job,
                        token,
                        contents.options.inspection.clone(),
                        contents.scope.clone(),
                        contents.owner.clone(),
                        contents.generation,
                    ))
                } else {
                    None
                }
            };

            let Some((job, cancel, inspection_opts, scope_opt, owner_opt, job_gen)) = next else {
                break;
            };

            let scope = scope_opt.or_else(|| {
                let make_scope =
                    || Gds::try_current().map(|gds| gds.tasks().owner_scope("File previews"));
                match &owner_opt {
                    Some(owner) => owner.with(make_scope),
                    None => make_scope(),
                }
            });

            let key = job.source.clone();
            let running_key = job.source.clone();
            let worker_source = job.source;
            let worker_request = job.request;
            let worker_cancel = cancel.clone();
            let weak_inner: Weak<RefCell<Contents>> = Rc::downgrade(&self.inner);
            let cache_clone = self.clone();

            let spawn_job = move || {
                spawn_local(async move {
                    let mut guard = RunningGuard {
                        inner: Weak::clone(&weak_inner),
                        key: Some(key.clone()),
                        cancel: worker_cancel.clone(),
                    };

                    let outcome = run_blocking_inspection(
                        scope,
                        worker_source,
                        worker_request,
                        inspection_opts,
                        worker_cancel,
                    )
                    .await;

                    guard.key = None;

                    let Some(inner) = weak_inner.upgrade() else {
                        return;
                    };

                    let revision = {
                        let mut contents = inner.borrow_mut();
                        contents.running.remove(&key);
                        let removed_task = contents.tasks.remove(&key);
                        drop(removed_task);

                        if cancel.is_cancelled()
                            || contents.generation != job_gen
                            || contents.options.capacity == 0
                        {
                            None
                        } else if let Some(outcome) = outcome {
                            apply_outcome(&mut contents, &key, worker_request, outcome);
                            if let Some(entry) = contents.entries.get(&key) {
                                let still_meta = matches!(entry.snapshot.metadata, State::Loading);
                                let still_thumb =
                                    matches!(entry.snapshot.thumbnail, State::Loading);
                                if let Some(follow_up) =
                                    combine_missing_request(still_meta, still_thumb)
                                {
                                    enqueue_lifo(&mut contents.pending, key.clone(), follow_up);
                                }
                            }
                            contents.trim(Some(&key));
                            contents.revision
                        } else {
                            contents.entries.remove(&key);
                            contents.revision
                        }
                    };

                    Self::notify(revision);
                    cache_clone.pump();
                })
            };

            let task = match &owner_opt {
                Some(owner) => owner.with(spawn_job),
                None => spawn_job(),
            };

            let previous_task = {
                let mut contents = self.inner.borrow_mut();
                if contents.running.contains_key(&running_key) {
                    contents.tasks.insert(running_key, task)
                } else {
                    Some(task)
                }
            };
            drop(previous_task);
        }
    }
}

async fn run_blocking_inspection(
    scope: Option<Scope>,
    source: Source,
    request: Request,
    options: crate::service::file_inspection::Options,
    cancel: CancellationToken,
) -> Option<WorkerOutcome> {
    if let Some(scope) = scope.as_ref().filter(|s| s.runtime_handle().is_some()) {
        return scope
            .spawn_blocking_named("File preview", move || {
                inspect_off_thread(&source, request, options, &cancel)
            })
            .await
            .ok();
    }

    let _completion =
        scope.and_then(|s| s.register_named("File preview", Execution::Blocking).ok());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawn_result = std::thread::Builder::new()
        .name("gds-file-preview".to_owned())
        .spawn(move || {
            let outcome = inspect_off_thread(&source, request, options, &cancel);
            let _ = tx.send(outcome);
        });
    if spawn_result.is_err() {
        return None;
    }
    rx.await.ok()
}

fn combine_missing_request(meta: bool, thumb: bool) -> Option<Request> {
    match (meta, thumb) {
        (true, true) => Some(Request::Both),
        (true, false) => Some(Request::Metadata),
        (false, true) => Some(Request::Thumbnail),
        (false, false) => None,
    }
}

fn combine_requests(a: Request, b: Request) -> Request {
    match (a.metadata() || b.metadata(), a.thumbnail() || b.thumbnail()) {
        (true, true) => Request::Both,
        (true, false) => Request::Metadata,
        (false, true) | (false, false) => Request::Thumbnail,
    }
}

fn enqueue_lifo(pending: &mut Vec<PendingJob>, source: Source, request: Request) {
    if let Some(pos) = pending.iter().position(|job| job.source == source) {
        let mut existing = pending.remove(pos);
        existing.request = combine_requests(existing.request, request);
        pending.push(existing);
    } else {
        pending.push(PendingJob { source, request });
    }
}

fn encode_thumbnail_png(thumbnail: &Thumbnail) -> Result<Vec<u8>, String> {
    let rgba = thumbnail.as_rgba();
    let mut encoded = Vec::new();
    let encoder =
        PngEncoder::new_with_quality(&mut encoded, CompressionType::Fast, FilterType::Sub);
    encoder
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|err| err.to_string())?;
    Ok(encoded)
}

fn inspect_off_thread(
    source: &Source,
    request: Request,
    options: crate::service::file_inspection::Options,
    cancel: &CancellationToken,
) -> WorkerOutcome {
    let inspection = source.load_cancellable_with_options(request, options, cancel);
    let metadata = match inspection.metadata {
        Field::NotRequested => State::NotRequested,
        Field::Available(meta) => State::Ready(Arc::new(meta)),
        Field::Unavailable => State::Unavailable,
        Field::Failed(Error::TooLarge) => State::TooLarge,
        Field::Failed(err) => State::Failed(Arc::from(err.to_string())),
    };
    let thumbnail = match inspection.thumbnail {
        Field::NotRequested => EncodedThumbnail::NotRequested,
        Field::Available(thumb) => match encode_thumbnail_png(&thumb) {
            Ok(bytes) => EncodedThumbnail::Available(bytes),
            Err(err) => EncodedThumbnail::Failed(Arc::from(err)),
        },
        Field::Unavailable => EncodedThumbnail::Unavailable,
        Field::Failed(Error::TooLarge) => EncodedThumbnail::TooLarge,
        Field::Failed(err) => EncodedThumbnail::Failed(Arc::from(err.to_string())),
    };
    WorkerOutcome {
        metadata,
        thumbnail,
    }
}

fn apply_outcome(contents: &mut Contents, key: &Source, request: Request, outcome: WorkerOutcome) {
    let (old_bytes, next_bytes) = {
        let entry = contents.entries.entry(key.clone()).or_default();
        let old_bytes = entry.bytes;

        if request.metadata() {
            entry.snapshot.metadata = outcome.metadata;
        }
        if request.thumbnail() {
            match outcome.thumbnail {
                EncodedThumbnail::NotRequested => {
                    entry.snapshot.thumbnail = State::NotRequested;
                    entry.bytes = 0;
                }
                EncodedThumbnail::Available(png_bytes) => {
                    let size = png_bytes.len();
                    let registered = ImageBytes::new(png_bytes);
                    entry.snapshot.thumbnail = State::Ready(Arc::new(registered));
                    entry.bytes = size;
                }
                EncodedThumbnail::Unavailable => {
                    entry.snapshot.thumbnail = State::Unavailable;
                    entry.bytes = 0;
                }
                EncodedThumbnail::TooLarge => {
                    entry.snapshot.thumbnail = State::TooLarge;
                    entry.bytes = 0;
                }
                EncodedThumbnail::Failed(err) => {
                    entry.snapshot.thumbnail = State::Failed(err);
                    entry.bytes = 0;
                }
            }
        }
        (old_bytes, entry.bytes)
    };

    contents.bytes = contents
        .bytes
        .saturating_sub(old_bytes)
        .saturating_add(next_bytes);
    contents.touch_lru(key);
}
