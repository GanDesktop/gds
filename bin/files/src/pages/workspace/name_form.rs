use std::{
    cell::{Cell, RefCell},
    ffi::{OsStr, OsString},
    path::Path,
    rc::Rc,
};

use gds::ui::feedback::dialog::{
    DialogActionsProps, DialogCommand, DialogDecision, DialogFooterProps, DialogHeaderProps,
    DialogTitleProps, handle_dialog_key_down,
};
use gds::ui::{IconName, IconProps};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::install_stylesheet;
use zgui::vocab::{ImeEvent, Key, NamedKey, Role};
use zgui::{css, view};
use zgui_ui::InputProps;

use super::ui::Browser;
use crate::{
    features::file_operations::{names, rename},
    shared::i18n,
};

#[derive(Clone, Default)]
pub(super) struct PromptDecisionSlot(Rc<RefCell<PromptDecisionState>>);

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
    pub(super) fn register(
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

    pub(super) fn clear(&self) {
        self.0.borrow_mut().active = None;
    }

    pub(super) fn confirm(&self) -> bool {
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

    pub(super) fn handle_key_down(
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

const NAME_FORM_SHEET: &str = "gds-files-name-form";
const NAME_FORM_CSS: &str = css!(
    r#"
    .gds-files-name-form {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 340px;
        max-width: 440px;
    }

    .gds-files-name-form[data-conflict="true"] {
        width: 100%;
        max-width: 100%;
    }

    .gds-files-name-form__title-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-name-form__title-icon {
        flex-shrink: 0;
        color: var(--gds-foreground);
    }

    .gds-files-name-form__message {
        margin: 0px;
        color: var(--gds-muted-foreground);
        font-size: 13px;
    }

    .gds-files-name-form__error {
        margin: 0px;
        color: var(--gds-error);
        font-size: 12px;
    }
    "#
);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NamePurpose {
    NewFolder,
    Rename,
    Conflict,
    Generic,
}

#[derive(Clone)]
pub(super) struct NameForm {
    purpose: NamePurpose,
    title: String,
    initial: OsString,
    display: String,
    non_utf8: bool,
    selection_end: usize,
}

impl NameForm {
    pub(super) fn new(purpose: NamePurpose, title: String, suggestion: OsString) -> Self {
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

    pub(super) const fn is_conflict(&self) -> bool {
        matches!(self.purpose, NamePurpose::Conflict)
    }

    fn checked_name(&self, value: &str) -> Result<OsString, &'static str> {
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
            NamePurpose::Conflict => Some(IconName::ClipboardPaste),
            NamePurpose::Generic => None,
        }
    }

    const fn primary_icon(&self) -> IconName {
        match self.purpose {
            NamePurpose::NewFolder => IconName::FolderPlus,
            NamePurpose::Rename => IconName::Pencil,
            NamePurpose::Conflict => IconName::ClipboardPaste,
            NamePurpose::Generic => IconName::Check,
        }
    }

    const fn message_key(&self) -> Option<&'static str> {
        match self.purpose {
            NamePurpose::NewFolder => Some("new-folder-message"),
            NamePurpose::Rename => Some("rename-message"),
            NamePurpose::Conflict => Some("transfer-conflict-message"),
            NamePurpose::Generic => None,
        }
    }

    const fn placeholder_key(&self) -> Option<&'static str> {
        match self.purpose {
            NamePurpose::NewFolder => Some("new-folder-placeholder"),
            NamePurpose::Rename => Some("rename-placeholder"),
            NamePurpose::Conflict => Some("transfer-conflict-placeholder"),
            NamePurpose::Generic => None,
        }
    }

    const fn confirm_key(&self) -> &'static str {
        match self.purpose {
            NamePurpose::NewFolder => "new-folder-confirm",
            NamePurpose::Rename => "rename-confirm",
            NamePurpose::Conflict => "paste-files",
            NamePurpose::Generic => "dialog-confirm",
        }
    }

    const fn cancel_key(&self) -> &'static str {
        match self.purpose {
            NamePurpose::NewFolder | NamePurpose::Rename | NamePurpose::Conflict => {
                "operation-cancel"
            }
            NamePurpose::Generic => "dialog-cancel",
        }
    }

    pub(super) fn view(
        self,
        browser: &Browser,
        generation: u64,
        submit: Rc<dyn Fn(OsString)>,
        decision_slot: &PromptDecisionSlot,
    ) -> AnyView {
        if zgui::view::current_host().is_some() {
            install_stylesheet(NAME_FORM_SHEET, NAME_FORM_CSS);
        }

        let value = RwSignal::new_local(self.display.clone());
        let error = RwSignal::new_local(Option::<String>::None);
        let composing = Rc::new(Cell::new(false));
        let settled = Rc::new(Cell::new(false));
        let input_node = NodeRef::new();

        let initial_text = self.display.clone();
        let selection_end = self.selection_end;
        let focus_effect = RenderEffect::new(move |mounted_before: Option<bool>| {
            let mounted = input_node.get().is_some();
            if mounted && mounted_before != Some(true) {
                input_node.focus();
                input_node.set_value(&initial_text);
                input_node.set_selection(0..selection_end);
            }
            mounted
        });
        on_cleanup_local(move || drop(focus_effect));

        let clear_error_effect = RenderEffect::new(move |prev: Option<String>| {
            let current = value.get();
            if prev.as_deref() != Some(current.as_str()) && prev.is_some() {
                error.set(None);
            }
            current
        });
        on_cleanup_local(move || drop(clear_error_effect));

        let title = self.title.clone();
        let header_icon = self.header_icon();
        let primary_icon = self.primary_icon();
        let is_conflict = self.is_conflict();
        let confirm_key = self.confirm_key();
        let cancel_key = self.cancel_key();
        let message = self.message_key().map(i18n::text);
        let placeholder = self.placeholder_key().map(i18n::text);
        let confirm_label = Signal::derive_local(move || i18n::text(confirm_key));
        let cancel_label = Signal::derive_local(move || i18n::text(cancel_key));
        let invalid = Signal::derive_local(move || error.with(Option::is_some));

        let form = Rc::new(self);
        let browser_submit = browser.clone();
        let submit_form = form;
        let submit_settled = Rc::clone(&settled);
        let submit_action = Rc::new(move || {
            if submit_settled.get()
                || browser_submit.0.prompt_generation.get_untracked() != generation
                || matches!(
                    browser_submit.0.prompt.get_untracked(),
                    super::model::Prompt::Closed
                )
            {
                return;
            }
            let current = value.get_untracked();
            match submit_form.checked_name(&current) {
                Ok(validated) => {
                    submit_settled.set(true);
                    error.set(None);
                    // Close the active prompt before running `submit` so chained
                    // dialogs (for example, sequential conflict resolution) are not
                    // immediately overwritten with `Prompt::Closed`.
                    browser_submit.0.prompt.set(super::model::Prompt::Closed);
                    submit(validated);
                }
                Err(label_id) => {
                    error.set(Some(i18n::text_untracked(label_id)));
                    input_node.focus();
                }
            }
        });

        let browser_cancel = browser.clone();
        let cancel_settled = settled;
        let cancel_action = Rc::new(move || {
            if cancel_settled.get()
                || browser_cancel.0.prompt_generation.get_untracked() != generation
                || matches!(
                    browser_cancel.0.prompt.get_untracked(),
                    super::model::Prompt::Closed
                )
            {
                return;
            }
            cancel_settled.set(true);
            browser_cancel.cancel_prompt();
        });

        let decision_submit = Rc::clone(&submit_action);
        let decision_cancel = Rc::clone(&cancel_action);
        let decision_composing = Rc::clone(&composing);
        let decision_cb = UnsyncCallback::new(move |decision: DialogDecision| {
            if decision_composing.get() {
                return;
            }
            match decision {
                DialogDecision::Confirm => decision_submit(),
                DialogDecision::Cancel | DialogDecision::Dismiss => decision_cancel(),
            }
        });

        decision_slot.register(false, Some(Rc::clone(&composing)), decision_cb);

        let key_submit = submit_action;
        let key_cancel = Rc::clone(&cancel_action);
        let key_composing = Rc::clone(&composing);
        let start_composing = Rc::clone(&composing);
        let preedit_composing = Rc::clone(&composing);
        let commit_composing = Rc::clone(&composing);
        let end_composing = Rc::clone(&composing);
        let blur_composing = composing;

        let dialog_cancel = cancel_action;
        let dialog_composing = Rc::clone(&blur_composing);

        AnyView::new(view! {
            column(
                class = "gds-files-name-form",
                attr:data-conflict = is_conflict.then(|| "true".to_owned()),
                on:key_down = move |event| {
                    if dialog_composing.get() {
                        return;
                    }
                    if event.key == Key::Named(NamedKey::Escape) {
                        event.prevent_default();
                        event.stop_propagation();
                        dialog_cancel();
                        return;
                    }
                    handle_dialog_key_down(false, decision_cb, event);
                },
            ) {
                DialogHeader {
                    {if let Some(icon) = header_icon {
                        AnyView::new(view! {
                            row(class = "gds-files-name-form__title-row") {
                                Icon(
                                    name = icon,
                                    size = 18.0,
                                    class = "gds-files-name-form__title-icon",
                                )
                                DialogTitle { {title} }
                            }
                        })
                    } else {
                        AnyView::new(view! {
                            DialogTitle { {title} }
                        })
                    }}
                }
                {message.map(|text| view! {
                    text(class = "gds-files-name-form__message") { {text} }
                })}
                Input(
                    value = value,
                    placeholder = placeholder,
                    invalid = invalid,
                    node_ref = input_node,
                    on:ime_start = move |_| start_composing.set(true),
                    on:ime_preedit = move |event| {
                        let active = match &**event {
                            ImeEvent::Preedit { text, .. } => !text.is_empty(),
                            _ => event.is_composing(),
                        };
                        preedit_composing.set(active);
                    },
                    on:ime_commit = move |_| commit_composing.set(false),
                    on:ime_end = move |_| end_composing.set(false),
                    on:focus_out = move |_| blur_composing.set(false),
                    on:key_down = move |event| {
                        if key_composing.get() {
                            return;
                        }
                        if event.key == Key::Named(NamedKey::Enter)
                            && !event.modifiers.control()
                            && !event.modifiers.alt()
                            && !event.modifiers.meta()
                        {
                            event.prevent_default();
                            event.stop_propagation();
                            key_submit();
                        } else if event.key == Key::Named(NamedKey::Escape) {
                            event.prevent_default();
                            event.stop_propagation();
                            key_cancel();
                        } else {
                            handle_dialog_key_down(false, decision_cb, event);
                        }
                    },
                ) {}
                {move || error.get().map(|text| view! {
                    text(
                        class = "gds-files-name-form__error",
                        a11y:role = Role::Alert,
                    ) {
                        {text}
                    }
                })}
                DialogFooter {
                    DialogActions(
                        primary_label = confirm_label,
                        primary_icon = primary_icon,
                        cancel_label = cancel_label,
                        on_decision = decision_cb,
                    )
                }
            }
        })
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
        assert!(!new_folder.is_conflict());
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

        let conflict = NameForm::new(
            NamePurpose::Conflict,
            "Name Conflict".into(),
            OsString::from("report (2).txt"),
        );
        assert!(conflict.is_conflict());
        assert_eq!(conflict.confirm_key(), "paste-files");
        assert_eq!(conflict.cancel_key(), "operation-cancel");
    }
}
