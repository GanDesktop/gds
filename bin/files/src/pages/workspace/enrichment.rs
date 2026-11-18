//! Bounded, viewport-prioritized secondary metadata with tab-scoped cancellation.
use super::ui::Browser;
use crate::entities::file::{
    ViewMode,
    columns::Column,
    directory::{self, EnrichmentNeed, Entry},
    sorting,
};
use anti_webui::prelude::*;
use gds::service::task_manager::Owner as TaskOwner;
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
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

struct Probed {
    before: Vec<Entry>,
    after: Vec<Entry>,
}
enum Delivery {
    Updates(Vec<Probed>),
    Finished,
    Failed(String),
}

impl Browser {
    pub(super) fn enrich_directory(&self, id: u64) {
        let preferences = self.0.preferences.snapshot().files;
        let settings = preferences.enrichment;
        let details = self.0.details.get_untracked();
        let Some((snapshot, revision, need, scope, priority)) = self.read(|state| {
            let tab = state.tabs.iter().find(|tab| tab.id == id)?;
            if tab.navigation.is_loading() {
                return None;
            }
            let snapshot = tab.navigation.directory_arc()?;
            let full = matches!(
                tab.sort.key,
                sorting::Key::Size | sorting::Key::Type | sorting::Key::DetailedType
            ) || state
                .picker
                .as_ref()
                .is_some_and(|picker| picker.projection().narrows());
            let need = EnrichmentNeed {
                children: (tab.mode == ViewMode::List && tab.columns.contains(Column::Size))
                    || tab.sort.key == sorting::Key::Size
                    || (details && tab.id == state.tab().id),
                content: true,
                symlinks: true,
                budget: if full {
                    directory::ProbeBudget::Full
                } else {
                    directory::ProbeBudget::Bounded(preferences.probe_budget)
                },
            };
            let columns = self.grid_columns();
            let start = if tab.id == state.tab().id {
                ((self.0.list.scroll_offset().y / self.row_height()).max(0.0) as usize)
                    * if tab.mode == ViewMode::List {
                        1
                    } else {
                        columns
                    }
            } else {
                0
            };
            let count = if tab.mode == ViewMode::List {
                64
            } else {
                16 * columns
            };
            let priority = tab
                .rows
                .iter()
                .skip(start.saturating_sub(16))
                .take(count)
                .filter_map(|row| row.path().map(PathBuf::from))
                .collect::<Vec<_>>();
            Some((
                snapshot,
                tab.navigation.revision(),
                need,
                (**tab.work.as_ref()?).clone(),
                priority,
            ))
        }) else {
            return;
        };
        let mut covered = need;
        let mut queued = need;
        let mut probe = need;
        let running = self.update(|state| {
            let Some(tab) = state.tab_by_id(id) else {
                return true;
            };
            if let Some(existing) = &tab.enrichment
                && let Some(previous) = existing.coverage(revision)
            {
                covered = need.union(previous);
                if need.is_satisfied_by(previous)
                    && let Enrichment::Running { queue, .. } = existing
                {
                    let entries = priority
                        .iter()
                        .filter_map(|path| snapshot.entry_for_path(path).cloned())
                        .collect::<Vec<_>>();
                    queue
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .prioritize_viewport(&entries, need);
                    return true;
                }
                if matches!(existing, Enrichment::Running { .. }) {
                    queued = covered;
                    probe = covered;
                }
                if matches!(existing, Enrichment::Complete { .. })
                    && (!need.children || previous.children)
                    && (!need.content || previous.content)
                    && (!need.symlinks || previous.symlinks)
                    && let (
                        directory::ProbeBudget::Bounded(target),
                        directory::ProbeBudget::Bounded(done),
                    ) = (need.budget, previous.budget)
                {
                    queued.budget = directory::ProbeBudget::Bounded(target.saturating_sub(done));
                }
            }
            false
        });
        if running {
            return;
        }
        let queue = EnrichQueue::new(
            &snapshot,
            &priority,
            queued,
            settings.priority_limit,
            settings.batch_size,
        );
        if queue.is_empty() {
            self.update(|state| {
                if let Some(tab) = state.tab_by_id(id) {
                    tab.enrichment = Some(Enrichment::Complete {
                        revision,
                        need: covered,
                    });
                }
            });
            return;
        }
        let queue = Arc::new(Mutex::new(queue));
        let work = scope.named_child("Directory metadata");
        let cancel = work.token();
        let tasks = (*work).clone();
        self.update(|state| {
            if let Some(tab) = state.tab_by_id(id) {
                tab.enrichment = Some(Enrichment::Running {
                    revision,
                    need: covered,
                    _work: work,
                    queue: queue.clone(),
                });
            }
        });
        let weak = self.downgrade();
        let valid = cancel.clone();
        let delivery = self.0.owner.with(|| {
            ui_channel(move |delivery| {
                if valid.is_cancelled() {
                    return;
                }
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                match delivery {
                    Delivery::Updates(batches) => {
                        let changed = browser.update(|state| {
                            state
                                .tab_by_id(id)
                                .and_then(|tab| {
                                    tab.navigation.mutate_directory(revision, |snapshot| {
                                        // A watcher may replace an entry while its probe is in flight.
                                        let updates = batches
                                            .into_iter()
                                            .flat_map(|batch| {
                                                batch.before.into_iter().zip(batch.after)
                                            })
                                            .filter_map(|(before, after)| {
                                                snapshot
                                                    .entry_for_path(&before.path)
                                                    .filter(|current| current.ptr_eq(&before))
                                                    .map(|_| after)
                                            })
                                            .collect::<Vec<_>>();
                                        directory::merge_enriched_in_place(snapshot, &updates)
                                    })
                                })
                                .flatten()
                                .is_some()
                        });
                        if changed {
                            browser.reproject(id);
                        }
                    }
                    Delivery::Finished => {
                        browser.update(|state| {
                            if let Some(tab) = state.tab_by_id(id) {
                                tab.enrichment = Some(Enrichment::Complete {
                                    revision,
                                    need: covered,
                                });
                            }
                        });
                    }
                    Delivery::Failed(error) => browser.report_notice(error),
                }
            })
        });
        let worker_tasks = tasks.clone();
        tasks.spawn_named("Coordinate directory metadata", async move {
            let (finished, mut results) = tokio::sync::mpsc::unbounded_channel();
            for _ in 0..settings.max_concurrent_batches.max(1) {
                let queue = queue.clone();
                let finished = finished.clone();
                let cancel = cancel.clone();
                let io = worker_tasks.clone();
                worker_tasks.spawn_named("Probe directory metadata", async move {
                    loop {
                        if cancel.is_cancelled() {
                            break;
                        }
                        let batch = queue
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .pop_batch();
                        let Some(batch) = batch else {
                            break;
                        };
                        let before = batch.entries;
                        let entries = before.clone();
                        let worker_cancel = cancel.clone();
                        let result = io
                            .spawn_blocking_named("Read secondary file metadata", move || {
                                directory::probe_batch_with_limit(
                                    &entries,
                                    probe,
                                    settings.child_count_limit,
                                    &worker_cancel,
                                )
                            })
                            .await
                            .map_err(|error| error.to_string())
                            .and_then(|result| result.map_err(|error| error.to_string()));
                        match result {
                            Ok(after) => {
                                if finished
                                    .send(Ok((Probed { before, after }, batch.is_viewport)))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(error) => {
                                let _ = finished.send(Err(error));
                                break;
                            }
                        }
                    }
                });
            }
            drop(finished);
            let delay = Duration::from_millis(if need.is_full_budget() {
                settings.reorder_coalesce_ms
            } else {
                settings.coalesce_ms
            });
            let mut pending = Vec::new();
            let mut count = 0usize;
            let mut deadline = None;
            loop {
                let next = if let Some(until) = deadline {
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        _ = tokio::time::sleep_until(until) => {
                            delivery.send(Delivery::Updates(std::mem::take(&mut pending)));
                            count = 0;
                            deadline = None;
                            continue;
                        }
                        next = results.recv() => next,
                    }
                } else {
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        next = results.recv() => next,
                    }
                };
                match next {
                    Some(Ok((batch, viewport))) => {
                        count += batch.after.len();
                        pending.push(batch);
                        if (viewport && !need.is_full_budget())
                            || count >= settings.coalesce_max_updates.max(1)
                            || delay.is_zero()
                        {
                            delivery.send(Delivery::Updates(std::mem::take(&mut pending)));
                            count = 0;
                            deadline = None;
                        } else if deadline.is_none() {
                            deadline = Some(tokio::time::Instant::now() + delay);
                        }
                    }
                    Some(Err(error)) => {
                        if !cancel.is_cancelled() {
                            delivery.send(Delivery::Failed(error));
                        }
                    }
                    None => break,
                }
            }
            if !pending.is_empty() {
                delivery.send(Delivery::Updates(pending));
            }
            delivery.send(Delivery::Finished);
        });
    }
    pub(super) fn viewport_changed(&self, position: PointF) {
        let id = {
            let mut state = self.0.state.borrow_mut();
            let tab = state.tab_mut();
            tab.navigation.view_mut().scroll = position.y;
            tab.id
        };
        self.enrich_directory(id);
    }
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
        let mut queue = EnrichQueue::new(
            &snapshot,
            &[],
            need,
            custom_limit,
            directory::DEFAULT_BATCH_SIZE,
        );
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
        let mut queue = EnrichQueue::new(
            &snapshot,
            &[],
            need,
            directory::DEFAULT_PRIORITY_LIMIT,
            directory::DEFAULT_BATCH_SIZE,
        );

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
