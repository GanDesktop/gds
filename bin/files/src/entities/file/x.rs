//! Explicit file-layout relationships exposed to other entity slices.
pub(crate) mod location {
    pub(crate) use super::super::ViewMode;
    pub(crate) use super::super::columns::Layout;
    pub(crate) use super::super::directory::{self, DirectorySnapshot, ReadOptions};
    pub(crate) use super::super::sorting::Sort;
}

pub(crate) mod preferences {
    pub(crate) use super::super::directory::DEFAULT_PROBE_BUDGET;
}
