//! File-chooser modes, filters and selection results.
mod model;
pub(crate) use model::{Filter, Options, Outcome};

mod session;
pub(crate) use session::{
    ActivatedPath, SaveDecision, Session, inspect_activated_path, prepare_save_targets,
};
pub(crate) mod targets;

#[cfg(test)]
mod tests;
