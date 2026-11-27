//! Layer 1 workspace state primitives and window-owned context (zero upward workspace dependencies).

pub(in crate::pages::workspace) mod context;
pub(in crate::pages::workspace) mod dock;
pub(in crate::pages::workspace) mod enrichment;
pub(in crate::pages::workspace) mod menus;
pub(in crate::pages::workspace) mod model;
pub(in crate::pages::workspace) mod name_form;
pub(in crate::pages::workspace) mod typeahead;
pub(in crate::pages::workspace) mod ui_flags;

pub(crate) use ui_flags::LaunchOptions;
