//! Properties, content inspection, and unified item editor dialog share existing services.

use crate::pages::workspace::state::context::Context;

mod artwork;
mod basic;
#[cfg(target_os = "linux")]
mod comment;
mod details;
mod dialogs;
pub(in crate::pages::workspace) mod editor;
mod info;
mod permissions;
mod tags;

#[cfg(test)]
pub(in crate::pages::workspace) use artwork::tests;
pub(crate) use details::Properties;
use permissions::PermissionEditor;

/// Layer 5 accessor for item inspection, artwork, details pane, and properties dialogs.
pub(in crate::pages::workspace) struct Inspection<'a>(pub &'a Context);
