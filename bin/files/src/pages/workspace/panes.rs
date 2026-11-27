//! Layer 5 workspace feature panes and overlays (depends strictly on Layers 1–4 and acyclic peers).

pub(in crate::pages::workspace) mod dialogs;
pub(in crate::pages::workspace) mod drag;
pub(in crate::pages::workspace) mod inspection;
pub(in crate::pages::workspace) mod marquee;
pub(in crate::pages::workspace) mod menus;
pub(in crate::pages::workspace) mod picker;
pub(in crate::pages::workspace) mod status;
pub(in crate::pages::workspace) mod terminals;

pub(in crate::pages::workspace) use dialogs::{render_conflict_dialog, render_name_form};
pub(in crate::pages::workspace) use drag::DragHost;
pub(in crate::pages::workspace) use inspection::Inspection;
pub(in crate::pages::workspace) use marquee::Marquee;
pub(in crate::pages::workspace) use menus::MenuHost;
pub(in crate::pages::workspace) use picker::PickerView;
pub(in crate::pages::workspace) use status::StatusBar;
pub(in crate::pages::workspace) use terminals::TerminalDock;
