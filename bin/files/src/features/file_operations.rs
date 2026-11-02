//! File-operation commands and the immutable batches they execute.
mod api;
mod model;
pub(crate) use api::{Batch, Error, Kind, Report, names, transfer};
pub(crate) use model::commands;
pub(crate) mod remote;
