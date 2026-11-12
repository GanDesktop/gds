//! Footer control anatomy: summary trigger, live details, and removable rows.
mod control;
mod list;
mod row;
mod summary;

pub(in crate::pages::workspace) use control::StatusControl;
pub(in crate::pages::workspace) use list::{LIST_HEIGHT, LIST_PADDING, LIST_WIDTH, StatusList};
pub(in crate::pages::workspace) use row::StatusRow;
pub(in crate::pages::workspace) use summary::StatusSummary;

const CONTROL_SIZE: f32 = 32.;
