//! Interactive canvas editing: selection, hit-testing, manipulation, reversible commands,
//! explicit coalescing history, and pure-Rust tool FSM.

mod command;
mod history;
mod hit_test;
mod manipulate;
mod selection;
#[cfg(test)]
mod tests;
mod tool_fsm;

#[cfg(test)]
pub use command::EditCommand;
#[cfg(test)]
pub use history::CoalesceKey;
pub use history::EditHistory;
#[cfg(test)]
pub use history::MAX_UNDO_DEPTH;
pub use hit_test::{HandleKind, HoverTarget, handles_for_item, hover_test_canvas};
#[cfg(test)]
pub use hit_test::{HitTarget, hit_test_canvas, hit_test_cycle_stack, marquee_hits};
pub use manipulate::MARKER_DIAMETER_RANGE;
pub use manipulate::ReorderOp;
#[cfg(test)]
pub use manipulate::{
    ResizeModifiers, reorder_annotations, resize_item_with_handle, translate_group,
};
#[cfg(test)]
pub use selection::SelectionSet;
#[cfg(test)]
pub use tool_fsm::FinishOutcome;
pub use tool_fsm::{
    ActiveInteraction, CancelOutcome, PointerDownInput, PointerDownOutcome, PointerModifiers,
    PointerMoveOutcome, PropertyValue, StyleApplyOutcome, StyleTarget, TextEditTarget, ToolFsm,
    ToolMode, ToolSettings,
};
