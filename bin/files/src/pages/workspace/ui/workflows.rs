//! User workflows, dialogs, and operations: file actions, transfers, undo/history, columns, permissions, tags, picker, and settings.

pub(in crate::pages::workspace) use super::*;

pub(in crate::pages::workspace) mod columns;
#[cfg(target_os = "linux")]
pub(in crate::pages::workspace) mod desktop_requests;
pub(in crate::pages::workspace) mod file_actions;
pub(in crate::pages::workspace) mod history;
pub(in crate::pages::workspace) mod name_form;
pub(in crate::pages::workspace) mod new_folder;
#[cfg(unix)]
pub(in crate::pages::workspace) mod permissions;
pub(in crate::pages::workspace) mod picker;
pub(in crate::pages::workspace) mod rename;
pub(in crate::pages::workspace) mod settings;
pub(in crate::pages::workspace) mod tag_editor;
pub(in crate::pages::workspace) mod transfers;
pub(in crate::pages::workspace) mod undo;
