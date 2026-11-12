//! Viewport-first, bounded-concurrency background metadata enrichment for local directories.

use std::{
    cell::{Cell, RefCell},
    collections::{HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

use gds::service::task_manager::{Scope, Task as IoTask};
use tokio_util::sync::CancellationToken;

use crate::entities::file::{
    ViewMode,
    columns::Column,
    directory::{self, EnrichmentNeed, Entry},
    sorting,
};
use gpui_kit::*;

use super::{Browser, OpenTab, picker};

const DEFAULT_VIEWPORT_ITEMS: usize = 64;
const VIEWPORT_MARGIN_ITEMS: usize = 16;
const MAX_CONCURRENT_BATCHES: usize = 4;
const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(32);
const COALESCE_MAX_UPDATES: usize = 512;

struct EnrichBatch {
    entries: Vec<Entry>,
    is_viewport: bool,
}

struct CompletedBatch {
    updates: Result<Vec<Entry>, directory::Error>,
    is_viewport: bool,
}

struct BatchSender {
    tx: Option<tokio::sync::mpsc::UnboundedSender<CompletedBatch>>,
    is_viewport: bool,
}

impl BatchSender {
    fn new(tx: tokio::sync::mpsc::UnboundedSender<CompletedBatch>, is_viewport: bool) -> Self {
        Self {
            tx: Some(tx),
            is_viewport,
        }
    }

    fn send(mut self, updates: Result<Vec<Entry>, directory::Error>) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(CompletedBatch {
                updates,
                is_viewport: self.is_viewport,
            });
        }
    }
}

impl Drop for BatchSender {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(CompletedBatch {
                updates: Err(directory::Error::Cancelled),
                is_viewport: self.is_viewport,
            });
        }
    }
}

#[derive(Default)]
struct EnrichQueue {
    priority: VecDeque<Entry>,
    backlog: VecDeque<Entry>,
    queued_or_done: HashSet<PathBuf>,
}

impl EnrichQueue {
    fn new(
        snapshot: &directory::DirectorySnapshot,
        priority_paths: &[PathBuf],
        need: EnrichmentNeed,
    ) -> Self {
        if need.is_empty() {
            return Self::default();
        }
        let mut priority = VecDeque::with_capacity(priority_paths.len());
        let mut queued_or_done = HashSet::with_capacity(priority_paths.len());
        for path in priority_paths {
            if let Some(entry) = snapshot.entry_for_path(path)
                && entry.needs_enrichment(need)
                && queued_or_done.insert(entry.path.clone())
            {
                priority.push_back(entry.clone());
            }
        }

        let mut backlog = VecDeque::new();
        let mut offscreen = 0usize;
        for entry in snapshot.entries.iter() {
            if !need.full_budget && offscreen >= directory::DEFAULT_PROBE_BUDGET {
                break;
            }
            if !entry.needs_enrichment(need) || queued_or_done.contains(&entry.path) {
                continue;
            }
            backlog.push_back(entry.clone());
            offscreen += 1;
        }

        Self {
            priority,
            backlog,
            queued_or_done,
        }
    }

    fn is_empty(&self) -> bool {
        self.priority.is_empty() && self.backlog.is_empty()
    }

    fn prioritize_viewport(&mut self, rows: &[Entry], need: EnrichmentNeed) {
        for entry in rows.iter().rev() {
            if entry.needs_enrichment(need) && self.queued_or_done.insert(entry.path.clone()) {
                self.priority.push_front(entry.clone());
            }
        }
    }

    fn pop_batch(&mut self) -> Option<EnrichBatch> {
        if !self.priority.is_empty() {
            let count = self.priority.len().min(directory::BATCH_SIZE);
            let entries = self.priority.drain(..count).collect();
            return Some(EnrichBatch {
                entries,
                is_viewport: true,
            });
        }
        let mut entries = Vec::with_capacity(directory::BATCH_SIZE);
        while entries.len() < directory::BATCH_SIZE {
            let Some(entry) = self.backlog.pop_front() else {
                break;
            };
            if self.queued_or_done.insert(entry.path.clone()) {
                entries.push(entry);
            }
        }
        (!entries.is_empty()).then_some(EnrichBatch {
            entries,
            is_viewport: false,
        })
    }
}

/// Cancellable handle for an in-flight local directory base scan.
pub(in crate::pages::workspace) struct LocalJob {
    cancel: CancellationToken,
    _task: Task<()>,
}

impl LocalJob {
    pub(in crate::pages::workspace) fn new(cancel: CancellationToken, task: Task<()>) -> Self {
        Self {
            cancel,
            _task: task,
        }
    }
}

impl Drop for LocalJob {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Cancellable handle for an in-flight or completed directory enrichment pass.
pub(in crate::pages::workspace) struct EnrichJob {
    revision: u64,
    need: EnrichmentNeed,
    in_flight: Rc<Cell<bool>>,
    queue: Rc<RefCell<EnrichQueue>>,
    cancel: CancellationToken,
    _task: Task<()>,
}

impl EnrichJob {
    fn new(
        revision: u64,
        need: EnrichmentNeed,
        in_flight: Rc<Cell<bool>>,
        queue: Rc<RefCell<EnrichQueue>>,
        cancel: CancellationToken,
        task: Task<()>,
    ) -> Self {
        Self {
            revision,
            need,
            in_flight,
            queue,
            cancel,
            _task: task,
        }
    }

    pub(in crate::pages::workspace) fn completed(revision: u64, need: EnrichmentNeed) -> Self {
        Self {
            revision,
            need,
            in_flight: Rc::new(Cell::new(false)),
            queue: Rc::new(RefCell::new(EnrichQueue::default())),
            cancel: CancellationToken::new(),
            _task: Task::ready(()),
        }
    }

    pub(in crate::pages::workspace) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(in crate::pages::workspace) const fn need(&self) -> EnrichmentNeed {
        self.need
    }

    pub(in crate::pages::workspace) fn is_in_flight(&self) -> bool {
        self.in_flight.get()
    }

    pub(in crate::pages::workspace) fn prioritize_viewport(&self, rows: &[Entry]) {
        self.queue.borrow_mut().prioritize_viewport(rows, self.need);
    }
}

impl Drop for EnrichJob {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Browser {
    /// Schedule viewport-first background metadata enrichment for `id` when its current
    /// view mode, visible columns, active sort, or picker filter need additional probes.
    pub(in crate::pages::workspace) fn schedule_enrichment(
        &mut self,
        id: u64,
        cx: &mut Context<Self>,
    ) {
        enum LoopEvent {
            Batch(Option<CompletedBatch>),
            FlushDeadline,
        }

        let Some(slot) = self.tab_slot(id) else {
            return;
        };
        if !slot.files_available() || slot.is_loading() {
            return;
        }
        let Some(directory) = slot.navigation.directory_arc() else {
            return;
        };
        let revision = slot.navigation.revision();
        let Some(mut need) = self.enrichment_need(id) else {
            return;
        };
        let Some(slot) = self.tab_slot(id) else {
            return;
        };
        let priority_paths = viewport_priority_paths(slot);
        if let Some(existing) = slot.local_enrich() {
            if existing.revision() == revision && need.is_satisfied_by(existing.need()) {
                let viewport_needs_probe = !existing.need().full_budget
                    && priority_paths.iter().any(|path| {
                        directory
                            .entry_for_path(path)
                            .is_some_and(|entry| entry.needs_enrichment(need))
                    });
                if existing.is_in_flight() {
                    if viewport_needs_probe {
                        let viewport_entries: Vec<Entry> = priority_paths
                            .iter()
                            .filter_map(|path| directory.entry_for_path(path).cloned())
                            .collect();
                        existing.prioritize_viewport(&viewport_entries);
                    }
                    return;
                }
                if !viewport_needs_probe {
                    return;
                }
            }
            if existing.revision() == revision {
                need = need.union(existing.need());
            }
        }
        let queue = EnrichQueue::new(&directory, &priority_paths, need);
        if queue.is_empty() {
            if let Some(slot) = self.tab_slot_mut(id) {
                slot.set_local_enrich(Some(EnrichJob::completed(revision, need)));
            }
            return;
        }

        let queue = Rc::new(RefCell::new(queue));
        let worker_queue = Rc::clone(&queue);
        let cancel = self.window.tasks.token().child_token();
        let worker_cancel = cancel.clone();
        let in_flight_flag = Rc::new(Cell::new(true));
        let task_in_flight = Rc::clone(&in_flight_flag);
        let tasks = self.window.tasks.clone();
        let task = cx.spawn(async move |this, cx| {
            let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel::<CompletedBatch>();
            let mut in_flight =
                fill_probe_slots(&tasks, &worker_queue, need, &worker_cancel, &done_tx, 0);
            let mut pending_updates: Vec<Entry> = Vec::new();
            let mut coalesce_deadline: Option<std::time::Instant> = None;
            let mut coalesce_timer: Option<IoTask<()>> = None;

            while in_flight > 0 || !pending_updates.is_empty() {
                if in_flight == 0 {
                    let _ = this.update(cx, |this, cx| {
                        this.apply_enrichment_batch(id, revision, &pending_updates, cx)
                    });
                    pending_updates.clear();
                    break;
                }

                let event = if let Some(timer) = &mut coalesce_timer {
                    tokio::select! {
                        biased;
                        completed = done_rx.recv() => LoopEvent::Batch(completed),
                        _ = timer => LoopEvent::FlushDeadline,
                    }
                } else {
                    LoopEvent::Batch(done_rx.recv().await)
                };

                let mut flush_now = false;
                match event {
                    LoopEvent::FlushDeadline => {
                        coalesce_timer = None;
                        in_flight = fill_probe_slots(
                            &tasks,
                            &worker_queue,
                            need,
                            &worker_cancel,
                            &done_tx,
                            in_flight,
                        );
                        flush_now = true;
                    }
                    LoopEvent::Batch(Some(first)) => {
                        let mut next = Some(first);
                        let mut aborted = false;
                        while let Some(completed) = next {
                            in_flight = in_flight.saturating_sub(1);
                            let Ok(batch_updates) = completed.updates else {
                                aborted = true;
                                break;
                            };
                            if worker_cancel.is_cancelled() {
                                aborted = true;
                                break;
                            }
                            in_flight = fill_probe_slots(
                                &tasks,
                                &worker_queue,
                                need,
                                &worker_cancel,
                                &done_tx,
                                in_flight,
                            );
                            if !batch_updates.is_empty() {
                                pending_updates.extend(batch_updates);
                                if completed.is_viewport {
                                    flush_now = true;
                                }
                            }
                            if pending_updates.len() >= COALESCE_MAX_UPDATES {
                                flush_now = true;
                                break;
                            }
                            next = done_rx.try_recv().ok();
                        }
                        if aborted {
                            worker_cancel.cancel();
                            break;
                        }
                    }
                    LoopEvent::Batch(None) => break,
                }

                if pending_updates.is_empty() {
                    coalesce_deadline = None;
                    coalesce_timer = None;
                    continue;
                }

                if in_flight == 0
                    || pending_updates.len() >= COALESCE_MAX_UPDATES
                    || coalesce_deadline
                        .is_some_and(|deadline| std::time::Instant::now() >= deadline)
                {
                    flush_now = true;
                }

                if flush_now {
                    let Ok(keep_going) = this.update(cx, |this, cx| {
                        this.apply_enrichment_batch(id, revision, &pending_updates, cx)
                    }) else {
                        worker_cancel.cancel();
                        break;
                    };
                    pending_updates.clear();
                    coalesce_deadline = None;
                    coalesce_timer = None;
                    if !keep_going {
                        worker_cancel.cancel();
                        break;
                    }
                } else if coalesce_deadline.is_none() {
                    coalesce_deadline = Some(std::time::Instant::now() + COALESCE_WINDOW);
                    coalesce_timer = Some(
                        tasks
                            .spawn_cancellable_named("Coalesce directory enrichment", async move {
                                tokio::time::sleep(COALESCE_WINDOW).await;
                            })
                            .cancel_on_drop(),
                    );
                }
            }

            task_in_flight.set(false);
            if !worker_cancel.is_cancelled() {
                let _ = this.update(cx, |this, cx| {
                    this.schedule_enrichment(id, cx);
                });
            }
        });

        if let Some(slot) = self.tab_slot_mut(id) {
            slot.set_local_enrich(Some(EnrichJob::new(
                revision,
                need,
                in_flight_flag,
                queue,
                cancel,
                task,
            )));
        }
    }

    /// Trigger background enrichment when newly scrolled-into-view entries still need probing.
    pub(in crate::pages::workspace) fn request_viewport_enrichment(
        &mut self,
        rows: &[Entry],
        cx: &mut Context<Self>,
    ) {
        let id = self.tabs.active_id;
        let Some(slot) = self.tab_slot(id) else {
            return;
        };
        if !slot.files_available() || slot.is_loading() {
            return;
        }
        let Some(need) = self.enrichment_need(id) else {
            return;
        };
        if !rows.iter().any(|entry| entry.needs_enrichment(need)) {
            return;
        }
        if let Some(job) = slot.local_enrich()
            && job.is_in_flight()
            && job.revision() == slot.navigation.revision()
            && need.is_satisfied_by(job.need())
        {
            job.prioritize_viewport(rows);
            return;
        }
        self.schedule_enrichment(id, cx);
    }

    fn enrichment_need(&self, id: u64) -> Option<EnrichmentNeed> {
        let slot = self.tab_slot(id)?;
        let location = slot.location();
        let layout = self.columns_layout(&location);
        let is_list = self.locations.places.effective_view_mode(location) == ViewMode::List;
        let details = self.details_visible() && id == self.tabs.active_id;
        let children = (is_list && layout.columns().contains(&Column::Size))
            || slot.sort.key == sorting::Key::Size
            || details;
        let picker_narrows = slot
            .picker
            .as_ref()
            .is_some_and(picker::Projection::narrows);
        let full_budget = matches!(
            slot.sort.key,
            sorting::Key::Size | sorting::Key::Type | sorting::Key::DetailedType
        ) || picker_narrows;
        Some(EnrichmentNeed {
            children,
            content: true,
            symlinks: true,
            full_budget,
        })
    }

    fn apply_enrichment_batch(
        &mut self,
        id: u64,
        revision: u64,
        batch_updates: &[Entry],
        cx: &mut Context<Self>,
    ) -> bool {
        let is_active = id == self.tabs.active_id;
        let details_visible = self.details_visible();
        let Some(slot) = self.tab_slot_mut(id) else {
            return false;
        };
        if slot.is_loading() || slot.navigation.revision() != revision {
            return false;
        }
        let Some(mut next_snapshot) = slot.navigation.directory_arc() else {
            return false;
        };
        let Some(changed_visible) =
            directory::merge_enriched_in_place(&mut next_snapshot, batch_updates)
        else {
            return true;
        };
        if !slot
            .navigation
            .update_directory(revision, Arc::clone(&next_snapshot))
        {
            return false;
        }
        if !is_active {
            if changed_visible {
                slot.projection_dirty = true;
            }
            return true;
        }
        let picker_narrows = slot
            .picker
            .as_ref()
            .is_some_and(picker::Projection::narrows);
        let can_alter_order_or_membership = picker_narrows
            || matches!(
                slot.sort.key,
                sorting::Key::Size | sorting::Key::Type | sorting::Key::DetailedType
            );
        if !changed_visible {
            if !slot.projection_dirty && !can_alter_order_or_membership {
                slot.apply_in_place_enrichment(&next_snapshot, batch_updates, None);
                let updated_rows = Arc::clone(&slot.table_rows);
                super::table::FileTable::update_rows(&self.files.table, updated_rows, false, cx);
            }
            return true;
        }
        if slot.projection_dirty || can_alter_order_or_membership {
            slot.refresh_visible();
            cx.notify();
            return true;
        }
        let touched_preview = details_visible
            && slot.navigation.view().selected.len() == 1
            && slot
                .navigation
                .view()
                .selected
                .first()
                .is_some_and(|selected_path| {
                    batch_updates
                        .iter()
                        .any(|entry| entry.path == *selected_path)
                });
        let viewport_range = viewport_item_range(slot, slot.table_rows.len());
        let touched_viewport =
            slot.apply_in_place_enrichment(&next_snapshot, batch_updates, viewport_range);
        let updated_rows = Arc::clone(&slot.table_rows);
        if touched_viewport || touched_preview {
            cx.notify();
        } else {
            super::table::FileTable::update_rows(&self.files.table, updated_rows, false, cx);
        }
        true
    }
}

fn fill_probe_slots(
    tasks: &Scope,
    queue: &Rc<RefCell<EnrichQueue>>,
    need: EnrichmentNeed,
    cancel: &CancellationToken,
    done_tx: &tokio::sync::mpsc::UnboundedSender<CompletedBatch>,
    mut in_flight: usize,
) -> usize {
    while in_flight < MAX_CONCURRENT_BATCHES {
        let Some(batch) = queue.borrow_mut().pop_batch() else {
            break;
        };
        spawn_probe_batch(tasks, batch, need, cancel, done_tx.clone());
        in_flight += 1;
    }
    in_flight
}

fn spawn_probe_batch(
    tasks: &Scope,
    batch: EnrichBatch,
    need: EnrichmentNeed,
    cancel: &CancellationToken,
    done_tx: tokio::sync::mpsc::UnboundedSender<CompletedBatch>,
) {
    let batch_cancel = cancel.clone();
    let sender = BatchSender::new(done_tx, batch.is_viewport);
    let _task: IoTask<()> = tasks.spawn_blocking_named("Enrich directory entries", move || {
        let updates = directory::probe_batch(&batch.entries, need, &batch_cancel);
        sender.send(updates);
    });
}

fn viewport_priority_paths(slot: &OpenTab) -> Vec<PathBuf> {
    let total = slot.table_rows.len();
    if total == 0 {
        return Vec::new();
    }
    let range = viewport_item_range(slot, total).unwrap_or(0..total.min(DEFAULT_VIEWPORT_ITEMS));
    slot.table_rows[range]
        .iter()
        .map(|entry| entry.path.clone())
        .collect()
}

fn viewport_item_range(slot: &OpenTab, total: usize) -> Option<std::ops::Range<usize>> {
    let state = slot.scroll.0.borrow();
    let item_size = state.last_item_size?;
    let row_height = f32::from(item_size.item.height);
    let viewport = f32::from(state.base_handle.bounds().size.height);
    if row_height <= 0. || viewport <= 0. {
        return None;
    }
    let top = (-f32::from(state.base_handle.offset().y)).max(0.);
    let start_row = (top / row_height).floor() as usize;
    let end_row = ((top + viewport) / row_height).ceil() as usize;
    let (start, end) = if slot.view_mode == ViewMode::Grid {
        let columns = slot.grid.columns.max(1);
        (
            (start_row * columns).min(total),
            (end_row.saturating_add(2) * columns).min(total),
        )
    } else {
        (
            start_row.min(total),
            end_row.saturating_add(VIEWPORT_MARGIN_ITEMS).min(total),
        )
    };
    (start < end).then_some(start..end)
}
