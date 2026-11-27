//! Layer 2 workspace projection accessors (depends strictly on Layer 1 `state/`).

pub(in crate::pages::workspace) mod geometry;
pub(in crate::pages::workspace) mod rows;
pub(in crate::pages::workspace) mod ui_state;

pub(in crate::pages::workspace) use geometry::Geometry;
pub(in crate::pages::workspace) use rows::Rows;
pub(in crate::pages::workspace) use ui_state::UiState;
