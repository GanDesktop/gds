//! File mutations, immutable batches and operation history.
mod api;
pub(crate) mod history;
pub(crate) mod names;
pub(crate) mod paste;
pub(crate) mod recycle;
pub(crate) mod remote;
mod snapshot;
pub(crate) mod transfer;

#[cfg(unix)]
pub(crate) mod permissions;

#[cfg(unix)]
pub(crate) use api::symlink;
pub(crate) use api::{Batch, Error, Kind, new_folder, rename, templates};
