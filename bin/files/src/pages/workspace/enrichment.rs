//! Bounded, viewport-prioritized secondary metadata with tab-scoped cancellation.

use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use gds::service::task_manager::Owner as TaskOwner;
use tokio::sync::mpsc;
use zgui::geom::Point;
use zgui::prelude::*;
use zgui::reactive::spawn_local;

use super::{
    geometry::{Grid, virtual_row_height},
    model::{Contents, Row, Tab},
    ui::Browser,
};
use crate::entities::file::{
    ViewMode,
    columns::Column,
    directory::{self, EnrichmentNeed, Entry},
    sorting,
};

pub(super) enum Enrichment {
    Running {
        revision: u64,
        need: EnrichmentNeed,
        _work: TaskOwner,
        queue: Arc<Mutex<EnrichQueue>>,
    },
    Complete {
        revision: u64,
        need: EnrichmentNeed,
    },
}

impl Enrichment {
    fn coverage(&self, revision: u64) -> Option<EnrichmentNeed> {
        match self {
            Self::Running {
                revision: current,
                need,
                ..
            }
            | Self::Complete {
                revision: current,
                need,
            } if *current == revision => Some(*need),
            _ => None,
        }
    }
}

struct EnrichBatch {
    entries: Vec<Entry>,
    is_viewport: bool,
}

pub(super) struct EnrichQueue {
    priority: VecDeque<Entry>,
    backlog: VecDeque<Entry>,
    /// Paths already popped by [`Self::pop_batch`] and handed to worker tasks.
    dispatched: HashSet<PathBuf>,
    priority_limit: usize,
    batch_size: usize,
}

impl Default for EnrichQueue {
    fn default() -> Self {
        Self {
            priority: VecDeque::new(),
            backlog: VecDeque::new(),
            dispatched: HashSet::new(),
            priority_limit: directory::DEFAULT_PRIORITY_LIMIT,
            batch_size: directory::DEFAULT_BATCH_SIZE,
        }
    }
}

impl EnrichQueue {
    fn new(
        snapshot: &directory::DirectorySnapshot,
        priority_paths: &[PathBuf],
        need: EnrichmentNeed,
        priority_limit: usize,
        batch_size: usize,
    ) -> Self {
        let batch_size = batch_size.max(1);
        if need.is_empty() {
            return Self {
                priority_limit,
                batch_size,
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
            batch_size,
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
        let batch_size = self.batch_size.max(1);
        let mut entries = Vec::with_capacity(batch_size);
        while entries.len() < batch_size {
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
        while entries.len() < batch_size {
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

#[cfg(test)]
mod tests;

enum EnrichMessage {
    Batch {
        updates: Vec<Entry>,
        reorder_relevant: bool,
    },
    Done {
        revision: u64,
        need: EnrichmentNeed,
    },
}

struct EnrichPlan {
    revision: u64,
    need: EnrichmentNeed,
    queue: Arc<Mutex<EnrichQueue>>,
    max_concurrent_batches: usize,
    child_count_limit: u32,
    coalesce_ms: u64,
    reorder_coalesce_ms: u64,
    coalesce_max_updates: usize,
    sort_depends_on_metadata: bool,
}

impl Browser {
    pub(super) fn enrich_directory(&self, id: u64) {
        self.preload_mime_descriptions(id);
        let initial_scroll = self.current_scroll_top_css(id);
        self.start_enrichment(id, initial_scroll, false);
    }

    fn preload_mime_descriptions(&self, id: u64) {
        let details_open = self.0.details.get_untracked();
        let missing: Vec<Arc<str>> = {
            let state = self.0.state.borrow();
            let Some(tab) = state.tabs.iter().find(|tab| tab.id == id) else {
                return;
            };
            if !matches!(tab.contents, Contents::Directory) {
                return;
            }
            let needs_descriptions = (tab.mode == ViewMode::List
                && tab.columns.contains(Column::DetailedType))
                || tab.sort.key == sorting::Key::DetailedType
                || details_open;
            if !needs_descriptions {
                return;
            }
            let Some(snapshot) = tab.navigation.directory() else {
                return;
            };
            let mut seen = HashSet::new();
            snapshot
                .entries
                .iter()
                .filter_map(|entry| entry.details.display_content()?.mime.as_ref())
                .filter(|mime| {
                    !directory::mime_descriptions::is_cached(mime) && seen.insert(Arc::clone(mime))
                })
                .cloned()
                .collect()
        };
        if missing.is_empty() {
            return;
        }
        let browser = self.clone();
        let task = self
            .0
            .tasks
            .spawn_blocking_named("Load MIME descriptions", move || {
                directory::mime_descriptions::preload_mimes(missing)
            });
        drop(spawn_local(async move {
            let Ok(true) = task.await else {
                return;
            };
            let sort_by_detailed_type = browser.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .is_some_and(|tab| tab.sort.key == sorting::Key::DetailedType)
            });
            if sort_by_detailed_type {
                browser.reproject(id);
            } else {
                browser.changed();
            }
        }));
    }

    pub(super) fn viewport_changed(&self, position: Point<zgui::geom::CssPx, zgui::geom::Css>) {
        let top = position.y.0.max(0.0);
        let grid = self.grid_geometry_untracked();
        let row_h = self.row_height_untracked();
        let scale = self.0.list.scale().max(1.0);
        let viewport_h = self.viewport_height_css();
        let details_open = self.0.details.get_untracked();
        let files_prefs = self.0.preferences.snapshot().files;
        let probe_budget = files_prefs.probe_budget;
        let content_probing = files_prefs.enrichment.content_probing;
        enum Action {
            None,
            StartViewportOnly(u64),
            StartFull(u64),
        }
        let action = {
            let mut state = self.0.state.borrow_mut();
            let has_picker_filter = state
                .picker
                .as_ref()
                .is_some_and(|picker| picker.projection().narrows());
            let active = state.active;
            let Some(tab) = state.tabs.get_mut(active) else {
                return;
            };
            tab.navigation.view_mut().scroll = top;
            if !matches!(tab.contents, Contents::Directory) {
                return;
            }
            let revision = tab.navigation.revision();
            let visible = viewport_entries_from(tab, &grid, scale, row_h, viewport_h, top);
            if visible.is_empty() {
                return;
            }
            let (need, _) = enrichment_need_of(
                tab,
                probe_budget,
                content_probing,
                details_open,
                has_picker_filter,
            );
            if need.is_empty() {
                return;
            }
            match tab.enrichment.as_ref() {
                Some(Enrichment::Running {
                    revision: current,
                    need: covered,
                    queue,
                    ..
                }) if *current == revision && need.is_satisfied_by(*covered) => {
                    if let Ok(mut queue) = queue.lock() {
                        queue.prioritize_viewport(&visible, *covered);
                    }
                    Action::None
                }
                Some(Enrichment::Running { .. }) => Action::StartFull(tab.id),
                Some(Enrichment::Complete {
                    revision: current,
                    need: covered,
                }) if *current == revision && need.is_satisfied_by(*covered) => {
                    if visible.iter().any(|entry| entry.needs_enrichment(need)) {
                        Action::StartViewportOnly(tab.id)
                    } else {
                        Action::None
                    }
                }
                _ => Action::StartFull(tab.id),
            }
        };
        match action {
            Action::None => {}
            Action::StartViewportOnly(id) => self.start_enrichment(id, top, true),
            Action::StartFull(id) => self.start_enrichment(id, top, false),
        }
    }

    fn current_scroll_top_css(&self, id: u64) -> f32 {
        let is_active = self
            .read_untracked(|state| state.tabs.get(state.active).is_some_and(|tab| tab.id == id));
        if is_active {
            let scale = self.0.list.scale().max(1.0);
            let offset = self.0.list.scroll_offset().y.0 / scale;
            if offset > 0.0 {
                return offset;
            }
        }
        self.read_untracked(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map_or(0.0, |tab| tab.navigation.view().scroll.max(0.0))
        })
    }

    fn viewport_height_css(&self) -> f32 {
        let scale = self.0.list.scale().max(1.0);
        (self.0.list.scroll_position().scrollport.height.0 / scale).max(400.0)
    }

    fn start_enrichment(&self, id: u64, scroll_top_css: f32, viewport_only: bool) {
        let grid = self.grid_geometry_untracked();
        let row_h = self.row_height_untracked();
        let scale = self.0.list.scale().max(1.0);
        let viewport_h = self.viewport_height_css();
        let details_open = self.0.details.get_untracked();
        let files_prefs = self.0.preferences.snapshot().files;
        let probe_budget = files_prefs.probe_budget;
        let prefs = files_prefs.enrichment;
        let plan = {
            let mut state = self.0.state.borrow_mut();
            let has_picker_filter = state
                .picker
                .as_ref()
                .is_some_and(|picker| picker.projection().narrows());
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            if !matches!(tab.contents, Contents::Directory) {
                tab.enrichment = None;
                return;
            }
            let Some(snapshot) = tab.navigation.directory_arc() else {
                tab.enrichment = None;
                return;
            };
            let revision = tab.navigation.revision();
            let (mut need, sort_depends_on_metadata) = enrichment_need_of(
                tab,
                probe_budget,
                prefs.content_probing,
                details_open,
                has_picker_filter,
            );
            if viewport_only {
                need.budget = directory::ProbeBudget::Bounded(0);
            }
            if need.is_empty() {
                tab.enrichment = Some(Enrichment::Complete { revision, need });
                return;
            }
            if !viewport_only
                && let Some(covered) = tab.enrichment.as_ref().and_then(|e| e.coverage(revision))
                && need.is_satisfied_by(covered)
            {
                return;
            }
            let priority_paths: Vec<PathBuf> =
                viewport_entries_from(tab, &grid, scale, row_h, viewport_h, scroll_top_css)
                    .into_iter()
                    .map(|entry| entry.path.clone())
                    .collect();
            let queue = EnrichQueue::new(
                &snapshot,
                &priority_paths,
                need,
                prefs.priority_limit,
                prefs.batch_size,
            );
            if queue.is_empty() {
                tab.enrichment = Some(Enrichment::Complete { revision, need });
                return;
            }
            let work = self.0.tasks.named_child("Enrich directory metadata");
            let queue = Arc::new(Mutex::new(queue));
            let plan = EnrichPlan {
                revision,
                need,
                queue: Arc::clone(&queue),
                max_concurrent_batches: prefs.max_concurrent_batches,
                child_count_limit: prefs.child_count_limit,
                coalesce_ms: prefs.coalesce_ms,
                reorder_coalesce_ms: prefs.reorder_coalesce_ms,
                coalesce_max_updates: prefs.coalesce_max_updates,
                sort_depends_on_metadata,
            };
            tab.enrichment = Some(Enrichment::Running {
                revision,
                need,
                _work: work,
                queue,
            });
            plan
        };

        let (work_scope, cancel) = {
            let state = self.0.state.borrow();
            let Some(tab) = state.tabs.iter().find(|tab| tab.id == id) else {
                return;
            };
            let Some(Enrichment::Running { _work, .. }) = tab.enrichment.as_ref() else {
                return;
            };
            ((**_work).clone(), _work.token())
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        let worker_scope = work_scope.clone();
        drop(
            work_scope.spawn_named("Directory metadata enrichment", async move {
                let max_concurrent = plan.max_concurrent_batches.max(1);
                let child_limit = plan.child_count_limit.max(1);
                let max_coalesce = plan.coalesce_max_updates.max(1);
                let coalesce_wait = Duration::from_millis(if plan.sort_depends_on_metadata {
                    plan.reorder_coalesce_ms
                } else {
                    plan.coalesce_ms
                });

                loop {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let mut batches = Vec::with_capacity(max_concurrent);
                    if let Ok(mut guard) = plan.queue.lock() {
                        while batches.len() < max_concurrent {
                            let Some(batch) = guard.pop_batch() else {
                                break;
                            };
                            batches.push(batch);
                        }
                    }
                    if batches.is_empty() {
                        let _ = tx.send(EnrichMessage::Done {
                            revision: plan.revision,
                            need: plan.need,
                        });
                        return;
                    }

                    let mut handles = Vec::with_capacity(batches.len());
                    for batch in batches {
                        let batch_cancel = cancel.clone();
                        let need = plan.need;
                        let is_viewport = batch.is_viewport;
                        let task = worker_scope.spawn_blocking_named(
                            "Probe directory entry metadata",
                            move || {
                                directory::probe_batch_with_limit(
                                    &batch.entries,
                                    need,
                                    child_limit,
                                    &batch_cancel,
                                )
                            },
                        );
                        handles.push((task, is_viewport));
                    }

                    let mut merged = Vec::new();
                    let mut had_viewport = false;
                    for (handle, is_viewport) in handles {
                        if cancel.is_cancelled() {
                            return;
                        }
                        if let Ok(Ok(entries)) = handle.await {
                            merged.extend(entries);
                            had_viewport |= is_viewport;
                        }
                    }
                    if !merged.is_empty()
                        && merged.len() < max_coalesce
                        && !had_viewport
                        && !coalesce_wait.is_zero()
                    {
                        tokio::select! {
                            () = cancel.cancelled() => return,
                            () = tokio::time::sleep(coalesce_wait) => {}
                        }
                        while merged.len() < max_coalesce {
                            let next_batch = plan
                                .queue
                                .lock()
                                .ok()
                                .and_then(|mut guard| guard.pop_batch());
                            let Some(batch) = next_batch else {
                                break;
                            };
                            let batch_cancel = cancel.clone();
                            let need = plan.need;
                            let extra = worker_scope
                                .spawn_blocking_named("Probe coalesced metadata batch", move || {
                                    directory::probe_batch_with_limit(
                                        &batch.entries,
                                        need,
                                        child_limit,
                                        &batch_cancel,
                                    )
                                })
                                .await;
                            if let Ok(Ok(entries)) = extra {
                                merged.extend(entries);
                            }
                            if cancel.is_cancelled() {
                                return;
                            }
                        }
                    }
                    if !merged.is_empty()
                        && tx
                            .send(EnrichMessage::Batch {
                                updates: merged,
                                reorder_relevant: plan.sort_depends_on_metadata,
                            })
                            .is_err()
                    {
                        return;
                    }
                }
            }),
        );

        let weak = self.downgrade();
        let revision = plan.revision;
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                while let Some(msg) = rx.recv().await {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    match msg {
                        EnrichMessage::Batch {
                            updates,
                            reorder_relevant,
                        } => {
                            let mut should_reproject = false;
                            let mut changed_rows = false;
                            {
                                let mut state = browser.0.state.borrow_mut();
                                let Some(tab) = state.tab_by_id(id) else {
                                    break;
                                };
                                if !matches!(tab.contents, Contents::Directory)
                                    || tab.navigation.revision() != revision
                                {
                                    break;
                                }
                                let merged =
                                    tab.navigation.mutate_directory(revision, |snapshot| {
                                        directory::merge_enriched_in_place(snapshot, &updates)
                                    });
                                if let Some(Some(visible_changed)) = merged {
                                    changed_rows = visible_changed;
                                    should_reproject = reorder_relevant || visible_changed;
                                }
                            }
                            if should_reproject {
                                browser.reproject(id);
                            } else if changed_rows {
                                browser.changed();
                            }
                        }
                        EnrichMessage::Done { revision, need } => {
                            let mut state = browser.0.state.borrow_mut();
                            let Some(tab) = state.tab_by_id(id) else {
                                break;
                            };
                            if matches!(
                                tab.enrichment,
                                Some(Enrichment::Running {
                                    revision: current, ..
                                }) if current == revision
                            ) {
                                tab.enrichment = Some(Enrichment::Complete { revision, need });
                            }
                            break;
                        }
                    }
                }
            }));
        });
    }
}

fn viewport_entries_from(
    tab: &Tab,
    grid: &Grid,
    scale: f32,
    row_h: f32,
    viewport_h: f32,
    top_css: f32,
) -> Vec<Entry> {
    if tab.rows.is_empty() {
        return Vec::new();
    }
    let top = top_css.max(0.0);
    let bottom = top + viewport_h.max(1.0);
    let range = match tab.mode {
        ViewMode::Grid => {
            let height = virtual_row_height(grid.row_height, scale).max(1.0);
            let first = (top / height).floor() as usize * grid.columns;
            let end = (bottom / height).ceil() as usize * grid.columns;
            first.min(tab.rows.len())..end.min(tab.rows.len())
        }
        ViewMode::List => {
            let row_h = row_h.max(1.0);
            let first = (top / row_h).floor().max(0.0) as usize;
            let last = ((bottom / row_h).ceil().max(0.0) as usize).min(tab.rows.len());
            first.min(last)..last
        }
    };
    tab.rows[range]
        .iter()
        .filter_map(|row| match row {
            Row::File(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect()
}

fn enrichment_need_of(
    tab: &Tab,
    probe_budget: usize,
    content_probing: bool,
    details_open: bool,
    has_picker_filter: bool,
) -> (EnrichmentNeed, bool) {
    let children = (tab.mode == ViewMode::List && tab.columns.contains(Column::Size))
        || tab.sort.key == sorting::Key::Size;
    let wants_content = (tab.mode == ViewMode::List
        && (tab.columns.contains(Column::Type) || tab.columns.contains(Column::DetailedType)))
        || matches!(
            tab.sort.key,
            sorting::Key::Type | sorting::Key::DetailedType
        )
        || details_open
        || has_picker_filter;
    let content = content_probing && wants_content;
    let symlinks =
        wants_content || (tab.mode == ViewMode::List && tab.columns.contains(Column::Type));
    let sort_depends_on_metadata = matches!(
        tab.sort.key,
        sorting::Key::Type | sorting::Key::DetailedType | sorting::Key::Size
    );
    let need = EnrichmentNeed {
        children,
        content,
        symlinks,
        budget: if sort_depends_on_metadata || has_picker_filter {
            directory::ProbeBudget::Full
        } else {
            directory::ProbeBudget::Bounded(probe_budget)
        },
    };
    (need, sort_depends_on_metadata)
}
