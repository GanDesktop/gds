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

struct EnrichQueue {
    priority: VecDeque<Entry>,
    backlog: VecDeque<Entry>,
    /// Paths already popped by [`Self::pop_batch`] and handed to worker tasks.
    dispatched: HashSet<PathBuf>,
    priority_limit: usize,
}

impl Default for EnrichQueue {
    fn default() -> Self {
        Self {
            priority: VecDeque::new(),
            backlog: VecDeque::new(),
            dispatched: HashSet::new(),
            priority_limit: directory::DEFAULT_PRIORITY_LIMIT,
        }
    }
}

impl EnrichQueue {
    fn new(
        snapshot: &directory::DirectorySnapshot,
        priority_paths: &[PathBuf],
        need: EnrichmentNeed,
        priority_limit: usize,
    ) -> Self {
        if need.is_empty() {
            return Self {
                priority_limit,
                ..Self::default()
            };
        }
        let mut priority = VecDeque::with_capacity(priority_paths.len());
        let mut in_priority = HashSet::with_capacity(priority_paths.len());
        for path in priority_paths {
            if let Some(entry) = snapshot.entry_for_path(path)
                && entry.needs_enrichment(need)
                && in_priority.insert(entry.path.clone())
            {
                priority.push_back(entry.clone());
            }
        }

        let mut backlog = VecDeque::new();
        let mut offscreen = 0usize;
        for entry in snapshot.entries.iter() {
            if let directory::ProbeBudget::Bounded(limit) = need.budget
                && offscreen >= limit
            {
                break;
            }
            if !entry.needs_enrichment(need) || in_priority.contains(&entry.path) {
                continue;
            }
            backlog.push_back(entry.clone());
            offscreen += 1;
        }

        Self {
            priority,
            backlog,
            dispatched: HashSet::new(),
            priority_limit,
        }
    }

    fn is_empty(&self) -> bool {
        self.priority.is_empty() && self.backlog.is_empty()
    }

    fn prioritize_viewport(&mut self, rows: &[Entry], need: EnrichmentNeed) {
        let cap = self.priority_limit.max(rows.len());
        for entry in rows.iter().rev() {
            if !entry.needs_enrichment(need) || self.dispatched.contains(&entry.path) {
                continue;
            }
            if let Some(pos) = self
                .priority
                .iter()
                .position(|existing| existing.path == entry.path)
            {
                self.priority.remove(pos);
            }
            self.priority.push_front(entry.clone());
        }
        while self.priority.len() > cap {
            let Some(evicted) = self.priority.pop_back() else {
                break;
            };
            if need.is_full_budget() {
                self.backlog.push_back(evicted);
            }
        }
    }

    fn pop_batch(&mut self) -> Option<EnrichBatch> {
        let mut entries = Vec::with_capacity(directory::BATCH_SIZE);
        while entries.len() < directory::BATCH_SIZE {
            let Some(entry) = self.priority.pop_front() else {
                break;
            };
            if self.dispatched.insert(entry.path.clone()) {
                entries.push(entry);
            }
        }
        if !entries.is_empty() {
            return Some(EnrichBatch {
                entries,
                is_viewport: true,
            });
        }
        while entries.len() < directory::BATCH_SIZE {
            let Some(entry) = self.backlog.pop_front() else {
                break;
            };
            if self.dispatched.insert(entry.path.clone()) {
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

    fn set_priority_limit(&self, priority_limit: usize) {
        self.queue.borrow_mut().priority_limit = priority_limit;
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
        self.schedule_enrichment_inner(id, &[], cx);
    }

    fn schedule_enrichment_inner(
        &mut self,
        id: u64,
        extra_priority: &[Entry],
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
        let enrich_prefs = self.settings.preferences.files.enrichment;
        let Some(slot) = self.tab_slot(id) else {
            return;
        };
        let mut priority_paths = Vec::with_capacity(extra_priority.len() + DEFAULT_VIEWPORT_ITEMS);
        priority_paths.extend(extra_priority.iter().map(|entry| entry.path.clone()));
        priority_paths.extend(viewport_priority_paths(slot));
        let mut queue_need = need;
        if let Some(existing) = slot.local_enrich() {
            if existing.revision() == revision && need.is_satisfied_by(existing.need()) {
                let viewport_needs_probe = priority_paths.iter().any(|path| {
                    directory
                        .entry_for_path(path)
                        .is_some_and(|entry| entry.needs_enrichment(need))
                });
                if existing.is_in_flight() {
                    existing.set_priority_limit(enrich_prefs.priority_limit);
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
                let prev = existing.need();
                let same_dimensions = (!need.children || prev.children)
                    && (!need.content || prev.content)
                    && (!need.symlinks || prev.symlinks);
                if !existing.is_in_flight()
                    && same_dimensions
                    && let (
                        directory::ProbeBudget::Bounded(target),
                        directory::ProbeBudget::Bounded(done),
                    ) = (need.budget, prev.budget)
                {
                    // Initial off-screen budget was already spent on this snapshot revision:
                    // scroll re-entries probe only the viewport (`0` extra backlog), while a
                    // live increase of `probe_budget` probes only the delta.
                    queue_need.budget = directory::ProbeBudget::Bounded(target.saturating_sub(done));
                }
                need = need.union(prev);
            }
        }
        let queue = EnrichQueue::new(
            &directory,
            &priority_paths,
            queue_need,
            enrich_prefs.priority_limit,
        );
        if queue.is_empty() {
            if let Some(slot) = self.tab_slot_mut(id) {
                slot.set_local_enrich(Some(EnrichJob::completed(revision, need)));
            }
            return;
        }

        let max_concurrent = enrich_prefs.max_concurrent_batches.max(1);
        let coalesce_window = std::time::Duration::from_millis(if need.is_full_budget() {
            enrich_prefs.reorder_coalesce_ms
        } else {
            enrich_prefs.coalesce_ms
        });
        let max_updates = enrich_prefs.coalesce_max_updates.max(1);
        let queue = Rc::new(RefCell::new(queue));
        let worker_queue = Rc::clone(&queue);
        let cancel = self.window.tasks.token().child_token();
        let worker_cancel = cancel.clone();
        let in_flight_flag = Rc::new(Cell::new(true));
        let task_in_flight = Rc::clone(&in_flight_flag);
        let tasks = self.window.tasks.clone();
        let task = cx.spawn(async move |this, cx| {
            let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel::<CompletedBatch>();
            let mut in_flight = fill_probe_slots(
                &tasks,
                &worker_queue,
                need,
                &worker_cancel,
                &done_tx,
                0,
                max_concurrent,
            );
            let mut pending_updates: Vec<Entry> = Vec::new();
            let mut pending_has_viewport = false;
            let mut coalesce_deadline: Option<std::time::Instant> = None;
            let mut coalesce_timer: Option<IoTask<()>> = None;

            while in_flight > 0 || !pending_updates.is_empty() {
                if in_flight == 0 {
                    let has_viewport = std::mem::replace(&mut pending_has_viewport, false);
                    let _ = this.update(cx, |this, cx| {
                        this.apply_enrichment_batch(
                            id,
                            revision,
                            &pending_updates,
                            has_viewport,
                            cx,
                        )
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
                            max_concurrent,
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
                                max_concurrent,
                            );
                            if !batch_updates.is_empty() {
                                pending_updates.extend(batch_updates);
                                if completed.is_viewport {
                                    pending_has_viewport = true;
                                    if !need.is_full_budget() {
                                        flush_now = true;
                                    }
                                }
                            }
                            if pending_updates.len() >= max_updates {
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

                if coalesce_window.is_zero()
                    || in_flight == 0
                    || pending_updates.len() >= max_updates
                    || coalesce_deadline
                        .is_some_and(|deadline| std::time::Instant::now() >= deadline)
                {
                    flush_now = true;
                }

                if flush_now {
                    let has_viewport = std::mem::replace(&mut pending_has_viewport, false);
                    let Ok(keep_going) = this.update(cx, |this, cx| {
                        this.apply_enrichment_batch(
                            id,
                            revision,
                            &pending_updates,
                            has_viewport,
                            cx,
                        )
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
                    coalesce_deadline = Some(std::time::Instant::now() + coalesce_window);
                    coalesce_timer = Some(
                        tasks
                            .spawn_cancellable_named("Coalesce directory enrichment", async move {
                                tokio::time::sleep(coalesce_window).await;
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
        self.schedule_enrichment_inner(id, rows, cx);
    }

    pub(in crate::pages::workspace) fn enrichment_need(&self, id: u64) -> Option<EnrichmentNeed> {
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
        let budget = if full_budget {
            directory::ProbeBudget::Full
        } else {
            directory::ProbeBudget::Bounded(self.settings.preferences.files.probe_budget)
        };
        Some(EnrichmentNeed {
            children,
            content: true,
            symlinks: true,
            budget,
        })
    }

    fn apply_enrichment_batch(
        &mut self,
        id: u64,
        revision: u64,
        batch_updates: &[Entry],
        is_viewport: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let is_active = id == self.tabs.active_id;
        let details_visible = self.details_visible();
        let marquee_active = self.files.marquee.is_active();
        if !self.tab_slot(id).is_some_and(|slot| {
            !slot.is_loading()
                && slot.navigation.revision() == revision
                && slot.navigation.directory().is_some()
        }) {
            return false;
        }
        if !is_active {
            let Some(slot) = self.tab_slot_mut(id) else {
                return false;
            };
            slot.table_rows = Arc::from([]);
            slot.sorted_projection = None;
            let Some(changed_opt) = slot.navigation.mutate_directory(revision, |snapshot| {
                directory::merge_enriched_in_place(snapshot, batch_updates)
            }) else {
                return false;
            };
            if changed_opt.is_some() {
                slot.projection_dirty = true;
            }
            return true;
        }

        let suspended_table_drag =
            super::table::FileTable::suspend_rows_for_enrichment(&self.files.table, cx);
        let suspended_grid = self.files.grid_selection.suspend_for_enrichment();

        let Some(slot) = self.tab_slot_mut(id) else {
            return false;
        };
        let picker_narrows = slot
            .picker
            .as_ref()
            .is_some_and(picker::Projection::narrows);
        let picker_membership_changed = if picker_narrows
            && !slot.projection_dirty
            && let (Some(picker), Some(current_dir)) =
                (slot.picker.as_ref(), slot.navigation.directory())
        {
            let raw_filter = slot.filter();
            let filter = (!raw_filter.is_empty()).then(|| raw_filter.to_lowercase());
            let filter_str = filter.as_deref().unwrap_or("");
            batch_updates.iter().any(|update| {
                current_dir.entry_for_path(&update.path).is_some_and(|old| {
                    old.matches_filter(filter_str) && picker.keeps(old) != picker.keeps(update)
                })
            })
        } else {
            false
        };
        let mut restore_canonical_rows = false;
        let mut restore_proj_source = false;
        if let Some(current_entries) = slot.navigation.directory().map(|dir| &dir.entries) {
            if Arc::ptr_eq(&slot.table_rows, current_entries) {
                slot.table_rows = Arc::from([]);
                restore_canonical_rows = true;
            }
            if let Some(proj) = &mut slot.sorted_projection
                && Arc::ptr_eq(&proj.source, current_entries)
            {
                proj.source = Arc::from([]);
                restore_proj_source = true;
            }
        }

        let Some(changed_opt) = slot.navigation.mutate_directory(revision, |snapshot| {
            directory::merge_enriched_in_place(snapshot, batch_updates)
        }) else {
            if let Some(dir) = slot.navigation.directory() {
                if restore_canonical_rows {
                    slot.table_rows = Arc::clone(&dir.entries);
                }
                if restore_proj_source && let Some(proj) = &mut slot.sorted_projection {
                    proj.source = Arc::clone(&dir.entries);
                }
            }
            let current_rows = Arc::clone(&slot.table_rows);
            super::table::FileTable::restore_rows_after_enrichment(
                &self.files.table,
                Arc::clone(&current_rows),
                suspended_table_drag,
                false,
                cx,
            );
            self.files
                .grid_selection
                .restore_after_enrichment(suspended_grid, &current_rows);
            return false;
        };

        let Some(next_snapshot) = slot.navigation.directory_arc() else {
            return false;
        };
        if restore_canonical_rows {
            slot.table_rows = Arc::clone(&next_snapshot.entries);
        }

        let Some(changed_visible) = changed_opt else {
            if restore_proj_source && let Some(proj) = &mut slot.sorted_projection {
                proj.source = Arc::clone(&next_snapshot.entries);
            }
            let current_rows = Arc::clone(&slot.table_rows);
            super::table::FileTable::restore_rows_after_enrichment(
                &self.files.table,
                Arc::clone(&current_rows),
                suspended_table_drag,
                false,
                cx,
            );
            self.files
                .grid_selection
                .restore_after_enrichment(suspended_grid, &current_rows);
            return true;
        };

        let sort_can_alter_order = matches!(
            slot.sort.key,
            sorting::Key::Size | sorting::Key::Type | sorting::Key::DetailedType
        );

        if !changed_visible {
            if slot.projection_dirty && !marquee_active {
                self.release_active_selection_mirrors(cx);
                if let Some(slot) = self.tab_slot_mut(id) {
                    slot.refresh_visible();
                }
                cx.notify();
                return true;
            }
            if !slot.projection_dirty {
                slot.apply_in_place_enrichment(&next_snapshot, batch_updates, None);
            }
            let updated_rows = Arc::clone(&slot.table_rows);
            super::table::FileTable::restore_rows_after_enrichment(
                &self.files.table,
                Arc::clone(&updated_rows),
                suspended_table_drag,
                false,
                cx,
            );
            self.files
                .grid_selection
                .restore_after_enrichment(suspended_grid, &updated_rows);
            return true;
        }

        if slot.projection_dirty
            || picker_membership_changed
            || (marquee_active && sort_can_alter_order)
        {
            slot.sorted_projection = None;
            if marquee_active {
                slot.projection_dirty = true;
                let current_rows = Arc::clone(&slot.table_rows);
                super::table::FileTable::restore_rows_after_enrichment(
                    &self.files.table,
                    Arc::clone(&current_rows),
                    suspended_table_drag,
                    false,
                    cx,
                );
                self.files
                    .grid_selection
                    .restore_after_enrichment(suspended_grid, &current_rows);
                return true;
            }
            self.release_active_selection_mirrors(cx);
            if let Some(slot) = self.tab_slot_mut(id) {
                slot.refresh_visible();
            }
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
        let in_viewport =
            slot.apply_in_place_enrichment(&next_snapshot, batch_updates, viewport_range);

        // Pre-check whether secondary metadata actually broke the active sort order (Dolphin style):
        // when probed MIME/size values preserve neighbor ordering, keep `table_rows` and `row_backing`
        // in place instead of running an `O(N log N)` full resort.
        if sort_can_alter_order {
            if !slot.sort.is_sorted(&slot.table_rows) {
                slot.sorted_projection = None;
                self.release_active_selection_mirrors(cx);
                if let Some(slot) = self.tab_slot_mut(id) {
                    slot.refresh_visible();
                }
                cx.notify();
                return true;
            }
            if slot
                .sorted_projection
                .as_ref()
                .is_some_and(|proj| !proj.sort.is_sorted(&proj.entries))
            {
                slot.sorted_projection = None;
            }
        }

        let touched_viewport = is_viewport || in_viewport;
        let updated_rows = Arc::clone(&slot.table_rows);
        super::table::FileTable::restore_rows_after_enrichment(
            &self.files.table,
            Arc::clone(&updated_rows),
            suspended_table_drag,
            touched_viewport,
            cx,
        );
        self.files
            .grid_selection
            .restore_after_enrichment(suspended_grid, &updated_rows);
        if touched_viewport || touched_preview {
            cx.notify();
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
    max_concurrent: usize,
) -> usize {
    while in_flight < max_concurrent {
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
    let row_height = if slot.view_mode == ViewMode::Grid {
        slot.grid.row_height
    } else {
        f32::from(state.last_item_size?.item.height)
    };
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

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use super::{EnrichQueue, EnrichmentNeed, Entry, directory};

    fn unprobed_dir_snapshot(count: usize) -> directory::DirectorySnapshot {
        let entries: Vec<Entry> = (0..count)
            .map(|index| {
                let name = format!("dir-{index:04}");
                Entry::new(
                    PathBuf::from(format!("/tmp/{name}")),
                    OsString::from(&name),
                    directory::EntryKind::Directory,
                    None,
                    None,
                    directory::EntryDetails::default(),
                )
            })
            .collect();
        directory::DirectorySnapshot {
            path: PathBuf::from("/tmp"),
            entries: entries.into(),
            warnings: Vec::new(),
            options: directory::ReadOptions::default(),
        }
    }

    #[test]
    fn prioritize_viewport_sheds_transient_scroll_items_under_bounded_budget() {
        let snapshot = unprobed_dir_snapshot(300);
        let need = EnrichmentNeed {
            children: true,
            budget: directory::ProbeBudget::Bounded(0),
            ..Default::default()
        };
        let custom_limit = 64usize;
        let mut queue = EnrichQueue::new(&snapshot, &[], need, custom_limit);
        assert!(queue.is_empty());

        // Simulate rapid scrolling across 10 viewports (200 entries total).
        for step in 0..10 {
            let start = step * 20;
            queue.prioritize_viewport(&snapshot.entries[start..start + 20], need);
        }

        let mut popped = Vec::new();
        while let Some(batch) = queue.pop_batch() {
            assert!(batch.is_viewport);
            popped.extend(batch.entries);
        }
        assert_eq!(popped.len(), custom_limit);
        // Most recent viewport (180..200) is popped first; earliest scrolled-past items (0..20) were shed.
        assert_eq!(popped[0].path, snapshot.entries[180].path);
        assert!(
            !popped
                .iter()
                .any(|entry| entry.path == snapshot.entries[0].path)
        );

        // Revisiting an earlier shed viewport later can still enqueue and probe it cleanly.
        queue.prioritize_viewport(&snapshot.entries[0..10], need);
        let revisited = queue.pop_batch().expect("revisited viewport batch");
        assert_eq!(revisited.entries.len(), 10);
        assert_eq!(revisited.entries[0].path, snapshot.entries[0].path);
    }

    #[test]
    fn prioritize_viewport_retains_overflow_in_backlog_under_full_budget() {
        let snapshot = unprobed_dir_snapshot(200);
        let need = EnrichmentNeed {
            children: true,
            budget: directory::ProbeBudget::Full,
            ..Default::default()
        };
        let mut queue =
            EnrichQueue::new(&snapshot, &[], need, directory::DEFAULT_PRIORITY_LIMIT);

        for step in 0..10 {
            let start = step * 20;
            queue.prioritize_viewport(&snapshot.entries[start..start + 20], need);
        }

        let mut popped = Vec::new();
        while let Some(batch) = queue.pop_batch() {
            popped.extend(batch.entries);
        }
        // Every unique entry is dispatched exactly once under Full budget.
        assert_eq!(popped.len(), 200);
        assert_eq!(popped[0].path, snapshot.entries[180].path);
    }
}
