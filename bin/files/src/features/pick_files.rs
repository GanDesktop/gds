//! File-chooser modes, filters and selection results.
mod model;
pub(crate) use model::{
    Filter, Options, emit_portal_result, emit_selections, exit_code, mark_cancelled,
    mark_confirmed, mark_failed,
};

mod session;
pub(crate) use session::{
    ActivatedPath, Projection, SaveDecision, Session, inspect_activated_path, prepare_save_targets,
};
pub(crate) mod targets;

#[cfg(test)]
mod tests;
