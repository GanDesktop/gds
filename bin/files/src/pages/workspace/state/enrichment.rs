//! Pure state and viewport-prioritized queue for directory metadata enrichment.

use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use gds::service::task_manager::Owner as TaskOwner;

use crate::entities::file::directory::{self, EnrichmentNeed, Entry};

pub(in crate::pages::workspace) enum Enrichment {
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
    pub(in crate::pages::workspace) fn coverage(&self, revision: u64) -> Option<EnrichmentNeed> {
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

pub(in crate::pages::workspace) struct EnrichBatch {
    pub(in crate::pages::workspace) entries: Vec<Entry>,
    pub(in crate::pages::workspace) is_viewport: bool,
}

pub(in crate::pages::workspace) struct EnrichQueue {
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
    pub(in crate::pages::workspace) fn new(
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

    pub(in crate::pages::workspace) fn is_empty(&self) -> bool {
        self.priority.is_empty() && self.backlog.is_empty()
    }

    pub(in crate::pages::workspace) fn prioritize_viewport(
        &mut self,
        rows: &[Entry],
        need: EnrichmentNeed,
    ) {
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

    pub(in crate::pages::workspace) fn pop_batch(&mut self) -> Option<EnrichBatch> {
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
