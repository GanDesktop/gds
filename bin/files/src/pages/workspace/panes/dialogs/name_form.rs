use std::{cell::Cell, ffi::OsString, rc::Rc};

use gds::ui::IconProps;
use gds::ui::feedback::dialog::{
    DialogActionsProps, DialogDecision, DialogFooterProps, DialogHeaderProps, DialogTitleProps,
    handle_dialog_key_down,
};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::install_stylesheet;
use zgui::vocab::{ImeEvent, Key, NamedKey, Role};
use zgui::{css, view};
use zgui_ui::InputProps;

use crate::pages::workspace::state::{
    context::Context,
    model::Prompt,
    name_form::{NameForm, PromptDecisionSlot},
};
use crate::shared::i18n;

const NAME_FORM_SHEET: &str = "gds-files-name-form";
const NAME_FORM_CSS: &str = css!(
    r#"
    .gds-files-name-form {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 100%;
        min-width: 0px;
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
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-files-name-form__error {
        margin: 0px;
        color: var(--gds-error);
        font-size: var(--gds-text-xs, 12px);
    }
    "#
);

pub(in crate::pages::workspace) fn render_name_form(
    form: NameForm,
    ctx: &Context,
    generation: u64,
    submit: Rc<dyn Fn(OsString)>,
    decision_slot: &PromptDecisionSlot,
) -> AnyView {
    if zgui::view::current_host().is_some() {
        install_stylesheet(NAME_FORM_SHEET, NAME_FORM_CSS);
    }

    let presentation = form.presentation();
    let value = RwSignal::new_local(presentation.initial_text.clone());
    let error = RwSignal::new_local(Option::<String>::None);
    let composing = Rc::new(Cell::new(false));
    let settled = Rc::new(Cell::new(false));
    let input_node = NodeRef::new();

    let initial_text = presentation.initial_text;
    let initial_selection = presentation.initial_selection;
    let focus_effect = RenderEffect::new(move |mounted_before: Option<bool>| {
        let mounted = input_node.get().is_some();
        if mounted && mounted_before != Some(true) {
            input_node.focus();
            input_node.set_value(&initial_text);
            input_node.set_selection(initial_selection.clone());
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

    let title = presentation.title;
    let header_icon = presentation.header_icon;
    let primary_icon = presentation.primary_icon;
    let confirm_key = presentation.confirm_key;
    let cancel_key = presentation.cancel_key;
    let message = presentation.message_key.map(i18n::text);
    let placeholder = presentation.placeholder_key.map(i18n::text);
    let confirm_label = Signal::derive_local(move || i18n::text(confirm_key));
    let cancel_label = Signal::derive_local(move || i18n::text(cancel_key));
    let invalid = Signal::derive_local(move || error.with(Option::is_some));

    let form = Rc::new(form);
    let ctx_submit = ctx.clone();
    let submit_form = form;
    let submit_settled = Rc::clone(&settled);
    let submit_action = Rc::new(move || {
        if submit_settled.get()
            || ctx_submit.0.prompt_generation.get_untracked() != generation
            || matches!(ctx_submit.0.prompt.get_untracked(), Prompt::Closed)
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
                ctx_submit.0.prompt.set(Prompt::Closed);
                submit(validated);
            }
            Err(label_id) => {
                error.set(Some(i18n::text_untracked(label_id)));
                input_node.focus();
            }
        }
    });

    let ctx_cancel = ctx.clone();
    let cancel_settled = settled;
    let cancel_action = Rc::new(move || {
        if cancel_settled.get()
            || ctx_cancel.0.prompt_generation.get_untracked() != generation
            || matches!(ctx_cancel.0.prompt.get_untracked(), Prompt::Closed)
        {
            return;
        }
        cancel_settled.set(true);
        ctx_cancel.cancel_prompt();
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
