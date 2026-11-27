//! Pure name-prompt form state, validation, and modal keyboard-decision slot.

use std::{
    cell::{Cell, RefCell},
    ffi::{OsStr, OsString},
    ops::Range,
    path::Path,
    rc::Rc,
};

use gds::ui::{
    IconName,
    feedback::dialog::{DialogCommand, DialogDecision},
};
use zgui::prelude::*;
use zgui::reactive::{UnsyncCallback, on_cleanup_local};

use crate::features::file_operations::{names, rename};

#[derive(Clone, Default)]
pub(in crate::pages::workspace) struct PromptDecisionSlot(Rc<RefCell<PromptDecisionState>>);

#[derive(Default)]
struct PromptDecisionState {
    next_token: u64,
    active: Option<PromptDecisionEntry>,
}

#[derive(Clone)]
struct PromptDecisionEntry {
    token: u64,
    confirm_disabled: bool,
    composing: Option<Rc<Cell<bool>>>,
    on_decision: UnsyncCallback<DialogDecision>,
}

impl PromptDecisionSlot {
    pub(in crate::pages::workspace) fn register(
        &self,
        confirm_disabled: bool,
        composing: Option<Rc<Cell<bool>>>,
        on_decision: UnsyncCallback<DialogDecision>,
    ) {
        let token = {
            let mut state = self.0.borrow_mut();
            state.next_token = state.next_token.wrapping_add(1);
            let token = state.next_token;
            state.active = Some(PromptDecisionEntry {
                token,
                confirm_disabled,
                composing,
                on_decision,
            });
            token
        };
        let slot = Rc::clone(&self.0);
        on_cleanup_local(move || {
            let mut state = slot.borrow_mut();
            if state
                .active
                .as_ref()
                .is_some_and(|entry| entry.token == token)
            {
                state.active = None;
            }
        });
    }

    pub(in crate::pages::workspace) fn clear(&self) {
        self.0.borrow_mut().active = None;
    }

    pub(in crate::pages::workspace) fn confirm(&self) -> bool {
        let entry = self.0.borrow().active.clone();
        let Some(entry) = entry else {
            return false;
        };
        if entry.confirm_disabled || entry.composing.as_ref().is_some_and(|c| c.get()) {
            return false;
        }
        entry.on_decision.run(DialogDecision::Confirm);
        true
    }

    pub(in crate::pages::workspace) fn handle_key_down(
        &self,
        ev: &mut EventCx<'_, zgui::view::events::KeyDown>,
    ) -> bool {
        let entry = self.0.borrow().active.clone();
        let Some(entry) = entry else {
            return false;
        };
        if entry.composing.as_ref().is_some_and(|c| c.get()) {
            return false;
        }
        if !entry.confirm_disabled && DialogCommand::Confirm.matches_event(ev, ev.modifiers) {
            entry.on_decision.run(DialogDecision::Confirm);
            ev.prevent_default();
            ev.stop_propagation();
            true
        } else if DialogCommand::Cancel.matches_event(ev, ev.modifiers) {
            entry.on_decision.run(DialogDecision::Cancel);
            ev.prevent_default();
            ev.stop_propagation();
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pages::workspace) enum NamePurpose {
    NewFolder,
    Rename,
    Generic,
}

pub(in crate::pages::workspace) struct NameFormPresentation {
    pub title: String,
    pub initial_text: String,
    pub initial_selection: Range<usize>,
    pub header_icon: Option<IconName>,
    pub primary_icon: IconName,
    pub message_key: Option<&'static str>,
    pub placeholder_key: Option<&'static str>,
    pub confirm_key: &'static str,
    pub cancel_key: &'static str,
}

#[derive(Clone)]
pub(in crate::pages::workspace) struct NameForm {
    purpose: NamePurpose,
    title: String,
    initial: OsString,
    display: String,
    non_utf8: bool,
    selection_end: usize,
}

impl NameForm {
    pub(in crate::pages::workspace) fn new(
        purpose: NamePurpose,
        title: String,
        suggestion: OsString,
    ) -> Self {
        let non_utf8 = suggestion.to_str().is_none();
        let display = suggestion.to_string_lossy().into_owned();
        let selection_end = initial_selection_end(purpose, &display);
        Self {
            purpose,
            title,
            initial: suggestion,
            display,
            non_utf8,
            selection_end,
        }
    }

    pub(in crate::pages::workspace) fn presentation(&self) -> NameFormPresentation {
        NameFormPresentation {
            title: self.title.clone(),
            initial_text: self.display.clone(),
            initial_selection: 0..self.selection_end,
            header_icon: self.header_icon(),
            primary_icon: self.primary_icon(),
            message_key: self.message_key(),
            placeholder_key: self.placeholder_key(),
            confirm_key: self.confirm_key(),
            cancel_key: self.cancel_key(),
        }
    }

    pub(in crate::pages::workspace) fn checked_name(
        &self,
        value: &str,
    ) -> Result<OsString, &'static str> {
        let candidate = if self.non_utf8 && value == self.display {
            self.initial.clone()
        } else {
            OsString::from(value)
        };
        if self.purpose == NamePurpose::Rename {
            rename::validate_name(&candidate, &self.initial).map_err(|error| error.label_id())?;
            return Ok(candidate);
        }
        names::validate(OsStr::new(&candidate)).map_err(|error| error.label_id())?;
        Ok(candidate)
    }

    const fn header_icon(&self) -> Option<IconName> {
        match self.purpose {
            NamePurpose::NewFolder => Some(IconName::FolderPlus),
            NamePurpose::Rename => Some(IconName::TextCursor),
            NamePurpose::Generic => None,
        }
    }

    const fn primary_icon(&self) -> IconName {
        match self.purpose {
            NamePurpose::NewFolder => IconName::FolderPlus,
            NamePurpose::Rename => IconName::Pencil,
            NamePurpose::Generic => IconName::Check,
        }
    }

    const fn message_key(&self) -> Option<&'static str> {
        match self.purpose {
            NamePurpose::NewFolder => Some("new-folder-message"),
            NamePurpose::Rename => Some("rename-message"),
            NamePurpose::Generic => None,
        }
    }

    const fn placeholder_key(&self) -> Option<&'static str> {
        match self.purpose {
            NamePurpose::NewFolder => Some("new-folder-placeholder"),
            NamePurpose::Rename => Some("rename-placeholder"),
            NamePurpose::Generic => None,
        }
    }

    const fn confirm_key(&self) -> &'static str {
        match self.purpose {
            NamePurpose::NewFolder => "new-folder-confirm",
            NamePurpose::Rename => "rename-confirm",
            NamePurpose::Generic => "dialog-confirm",
        }
    }

    const fn cancel_key(&self) -> &'static str {
        match self.purpose {
            NamePurpose::NewFolder | NamePurpose::Rename => "operation-cancel",
            NamePurpose::Generic => "dialog-cancel",
        }
    }
}

fn initial_selection_end(purpose: NamePurpose, display: &str) -> usize {
    if purpose == NamePurpose::Rename
        && let Some(stem) = Path::new(display)
            .file_stem()
            .and_then(|stem| stem.to_str())
        && !stem.is_empty()
        && display.starts_with(stem)
        && display.as_bytes().get(stem.len()) == Some(&b'.')
    {
        return stem.len();
    }
    display.len()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{NameForm, NamePurpose};

    #[test]
    fn purpose_icons_labels_and_rename_validation_match_gpui_parity() {
        let new_folder = NameForm::new(
            NamePurpose::NewFolder,
            "New Folder".into(),
            OsString::from("New Folder"),
        );
        assert_eq!(new_folder.confirm_key(), "new-folder-confirm");
        assert_eq!(new_folder.cancel_key(), "operation-cancel");
        assert!(new_folder.checked_name("New Folder").is_ok());

        let rename_form = NameForm::new(
            NamePurpose::Rename,
            "Rename".into(),
            OsString::from("report.txt"),
        );
        assert_eq!(rename_form.confirm_key(), "rename-confirm");
        assert_eq!(rename_form.cancel_key(), "operation-cancel");
        assert_eq!(rename_form.selection_end, "report".len());
        assert_eq!(
            rename_form.checked_name("report.txt"),
            Err("rename-error-unchanged")
        );
        assert_eq!(
            rename_form.checked_name("summary.txt"),
            Ok(OsString::from("summary.txt"))
        );
    }
}
