//! File-chooser modes, filters and selection results.
mod model;
pub(crate) use model::{
    Filter, Options, emit_portal_result, emit_selections, exit_code, mark_cancelled,
    mark_confirmed, mark_failed,
};

mod session;
pub(crate) use session::{Projection, Session};
mod targets;
pub(crate) use targets::SaveTargets;
