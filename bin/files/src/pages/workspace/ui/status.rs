//! Footer control anatomy: summary trigger, live details, and removable rows.
mod control;
mod list;
mod row;
mod summary;

pub(super) use control::StatusControl;
pub(super) use list::StatusList;
pub(super) use row::StatusRow;
pub(super) use summary::StatusSummary;

const CONTROL_SIZE: f32 = 32.;
