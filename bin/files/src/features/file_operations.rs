//! File mutations, immutable batches and operation history.
mod api;
pub(crate) use api::{Batch, Error, Kind, names, transfer};
pub(crate) mod paste;
pub(crate) mod remote;

pub(crate) mod history;
pub(crate) mod recycle;

#[cfg(unix)]
pub(crate) mod permissions;

#[cfg(unix)]
pub(crate) use api::symlink;
pub(crate) use api::{new_folder, rename, templates};
