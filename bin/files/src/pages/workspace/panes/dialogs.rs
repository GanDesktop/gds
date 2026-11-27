//! Workspace modal dialog renderers for name forms and transfer conflicts.

mod conflict;
mod name_form;

pub(in crate::pages::workspace) use conflict::render_conflict_dialog;
pub(in crate::pages::workspace) use name_form::render_name_form;
