//! Explicit file-layout relationships exposed to other entity slices.
pub(crate) mod location {
    pub(crate) use super::super::ViewMode;
    pub(crate) use super::super::columns::Layout;
    pub(crate) use super::super::directory::{self, DirectorySnapshot, ReadOptions};
    pub(crate) use super::super::sorting::Sort;
}

pub(crate) mod preferences {
    pub(crate) use super::super::directory::{
        DEFAULT_BATCH_SIZE, DEFAULT_CHILD_COUNT_LIMIT, DEFAULT_COALESCE_MAX_UPDATES,
        DEFAULT_COALESCE_MS, DEFAULT_MAX_CONCURRENT_BATCHES, DEFAULT_PRIORITY_LIMIT,
        DEFAULT_PROBE_BUDGET, DEFAULT_REORDER_COALESCE_MS, DEFAULT_STREAM_INITIAL_DELAY_MS,
        DEFAULT_STREAM_INTERVAL_MS, DEFAULT_STREAM_MIN_ENTRIES, DEFAULT_WATCH_DEBOUNCE_MS,
        DEFAULT_WATCH_INCREMENTAL_LIMIT, StreamOptions,
    };
}
