//! Transfer conflict dialog with directional source→destination comparison, progressive inline
//! rename with live collision validation, batch "apply to all" with dynamic button labels, and
//! contextual actions (Replace / Merge / Rename / Skip / Abort).

use std::{
    cell::Cell,
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::Path,
    rc::Rc,
    time::UNIX_EPOCH,
};

use gds::ui::button::{ButtonProps, ButtonSize, ButtonVariant};
use gds::ui::feedback::dialog::{
    DialogDecision, DialogDescriptionProps, DialogFooterProps, DialogHeaderProps, DialogTitleProps,
    handle_dialog_key_down,
};
use gds::ui::time::{Format as TimeFormat, Presentation as TimePresentation, now as clock_now};
use gds::ui::{IconName, IconProps};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::install_stylesheet;
use zgui::vocab::{ImeEvent, Key, NamedKey, Role};
use zgui::{css, view};
use zgui_ui::{CheckboxProps, Checked, InputProps, LabelProps};
use zgui_ui_primitives::Binding;

use crate::pages::workspace::state::{
    context::Context, model::Prompt, name_form::PromptDecisionSlot,
};
use crate::{
    entities::file::ui::EntryPresentation,
    features::file_operations::{
        names,
        paste::{
            ConflictAction, ConflictKind, ConflictResolution, EntryInfo, EntryKind, PasteConflict,
        },
        transfer,
    },
    shared::i18n,
};

const CONFLICT_DIALOG_SHEET: &str = "gds-files-conflict-dialog";
const CONFLICT_DIALOG_CSS: &str = css!(
    r#"
    .zui-dialog.gds-dialog-content.gds-files-conflict-modal,
    .zui-dialog.gds-dialog-content.gds-files-conflict-modal[data-size="md"] {
        width: 540px;
        max-width: calc(100vw - 32px);
    }

    .gds-files-conflict {
        display: flex;
        flex-direction: column;
        gap: 14px;
        width: 100%;
        min-width: 0px;
        outline: none;
    }

    .gds-files-conflict__title-row {
        display: flex;
        flex-direction: row;
        align-items: flex-start;
        gap: 8px;
        min-width: 0px;
        padding-right: 28px;
    }

    .gds-files-conflict__title-icon {
        flex-shrink: 0;
        margin-top: 2px;
        color: var(--gds-foreground);
    }

    .gds-files-conflict__title-icon[data-tone="primary"] {
        color: var(--gds-primary);
    }

    .gds-files-conflict__title-icon[data-tone="danger"] {
        color: var(--gds-error);
    }

    .gds-files-conflict__title-text {
        flex: 1 1 0%;
        min-width: 0px;
        overflow-wrap: anywhere;
    }

    .gds-files-conflict__context {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 6px 10px;
        border-radius: var(--gds-radius-control, 6px);
        background: var(--gds-surface);
        border: 1px solid var(--gds-border);
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        min-width: 0px;
    }

    .gds-files-conflict__context-location {
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-conflict__context-remaining {
        flex-shrink: 0;
        font-weight: 500;
        color: var(--gds-foreground);
    }

    .gds-files-conflict__compare {
        display: flex;
        flex-direction: column;
        gap: 6px;
        min-width: 0px;
    }

    .gds-files-conflict__card {
        display: flex;
        flex-direction: row;
        align-items: flex-start;
        gap: 10px;
        padding: 10px 12px;
        border-radius: var(--gds-radius-control, 6px);
        border: 1px solid var(--gds-border);
        background: var(--gds-surface-container-low, var(--gds-surface));
        min-width: 0px;
    }

    .gds-files-conflict__card[data-at-risk="true"] {
        border-left: 3px solid var(--gds-error);
    }

    .gds-files-conflict__card[data-merge-target="true"] {
        border-left: 3px solid var(--gds-primary);
    }

    .gds-files-conflict__card-icon {
        flex-shrink: 0;
        margin-top: 2px;
        color: var(--gds-muted-foreground);
    }

    .gds-files-conflict__card-body {
        display: flex;
        flex-direction: column;
        gap: 2px;
        flex: 1 1 0%;
        min-width: 0px;
    }

    .gds-files-conflict__card-head {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        flex-wrap: wrap;
        min-width: 0px;
    }

    .gds-files-conflict__card-role {
        font-size: var(--gds-text-xs, 12px);
        font-weight: 600;
        color: var(--gds-muted-foreground);
    }

    .gds-files-conflict__badge {
        padding: 1px 6px;
        border-radius: 999px;
        border: 1px solid var(--gds-border);
        background: var(--gds-surface);
        color: var(--gds-primary);
        font-size: var(--gds-text-xs, 12px);
        font-weight: 500;
    }

    .gds-files-conflict__badge[data-tone="muted"] {
        color: var(--gds-muted-foreground);
    }

    .gds-files-conflict__card-name {
        font-size: var(--gds-text-sm, 14px);
        font-weight: 500;
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-conflict__card-meta {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-conflict__connector {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: center;
        gap: 6px;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-text-xs, 12px);
        padding: 0px 4px;
    }

    .gds-files-conflict__connector[data-tone="danger"] {
        color: var(--gds-error);
    }

    .gds-files-conflict__connector[data-tone="primary"] {
        color: var(--gds-primary);
    }

    .gds-files-conflict__rename {
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 10px 12px;
        border-radius: var(--gds-radius-control, 6px);
        border: 1px solid var(--gds-border);
        background: var(--gds-surface);
        min-width: 0px;
    }

    .gds-files-conflict__rename-head {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-conflict__rename-label {
        font-size: var(--gds-text-xs, 12px);
        font-weight: 600;
        color: var(--gds-foreground);
    }

    .gds-files-conflict__error {
        margin: 0px;
        color: var(--gds-error);
        font-size: var(--gds-text-xs, 12px);
    }

    .gds-files-conflict__batch {
        display: flex;
        flex-direction: column;
        gap: 4px;
        min-width: 0px;
    }

    .gds-files-conflict__apply-all {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
    }

    .gds-files-conflict__actions {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
        width: 100%;
    }

    .gds-files-conflict__spacer {
        flex: 1 1 0%;
    }
    "#
);

#[cfg(test)]
thread_local! {
    static LAST_CONFLICT_ACTIONS: std::cell::RefCell<Option<ConflictTestHooks>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct ConflictTestHooks {
    pub(super) kind: ConflictKind,
    pub(super) relative_dir: Option<std::path::PathBuf>,
    pub(super) remaining_after_current: usize,
    pub(super) has_apply_all: bool,
    pub(super) apply_all: RwSignal<bool, LocalStorage>,
    pub(super) rename_mode: RwSignal<bool, LocalStorage>,
    pub(super) rename_value: RwSignal<String, LocalStorage>,
    pub(super) rename_invalid: Signal<bool, LocalStorage>,
    #[cfg(feature = "files-e2e")]
    pub(super) body_node: NodeRef,
    #[cfg(feature = "files-e2e")]
    pub(super) batch_node: NodeRef,
    #[cfg(feature = "files-e2e")]
    pub(super) footer_row_node: NodeRef,
    pub(super) enter_or_auto_rename: Rc<dyn Fn()>,
    pub(super) exit_rename: Option<Rc<dyn Fn()>>,
    pub(super) confirm_rename: Rc<dyn Fn()>,
    pub(super) replace: Option<Rc<dyn Fn()>>,
    pub(super) merge: Option<Rc<dyn Fn()>>,
    pub(super) skip: Rc<dyn Fn()>,
    pub(super) abort: Rc<dyn Fn()>,
}

#[cfg(test)]
pub(super) fn take_conflict_test_hooks() -> Option<ConflictTestHooks> {
    LAST_CONFLICT_ACTIONS.with(|slot| slot.borrow_mut().take())
}

pub(in crate::pages::workspace) fn render_conflict_dialog(
    ctx: &Context,
    generation: u64,
    mode: transfer::Mode,
    conflict: PasteConflict,
    resolve: Rc<dyn Fn(ConflictResolution)>,
    decision_slot: &PromptDecisionSlot,
) -> AnyView {
    if zgui::view::current_host().is_some() {
        install_stylesheet(CONFLICT_DIALOG_SHEET, CONFLICT_DIALOG_CSS);
    }

    let kind = conflict.kind;
    let target_label = conflict.target_label();
    let initial_suggestion = conflict.suggestion.clone();
    let non_utf8 = initial_suggestion.to_str().is_none();
    let initial_display = initial_suggestion.to_string_lossy().into_owned();
    let selection_end = initial_selection_end(&initial_display, conflict.source.kind);
    let occupied_names: Rc<HashSet<OsString>> = Rc::new(conflict.occupied_names.clone());
    let can_return_from_rename = matches!(
        kind,
        ConflictKind::FileIntoFile | ConflictKind::DirectoryIntoDirectory
    );

    let rename_mode = RwSignal::new_local(!can_return_from_rename);
    let rename_value = RwSignal::new_local(initial_display.clone());
    let apply_all = RwSignal::new_local(false);
    let composing = Rc::new(Cell::new(false));
    let settled = Rc::new(Cell::new(false));
    let body_node = NodeRef::new();
    let input_node = NodeRef::new();
    let batch_node = NodeRef::new();
    let footer_row_node = NodeRef::new();

    // Live validation checks both name syntax and whether the candidate already exists in this
    // destination folder, preventing dialog bounce-back.
    let val_initial = initial_suggestion.clone();
    let val_initial_display = initial_display.clone();
    let val_occupied = Rc::clone(&occupied_names);
    let rename_validation: Signal<Result<OsString, String>, LocalStorage> =
        Signal::derive_local(move || {
            let current = rename_value.get();
            let candidate = if non_utf8 && current == val_initial_display {
                val_initial.clone()
            } else {
                OsString::from(current)
            };
            if let Err(error) = names::validate(OsStr::new(&candidate)) {
                return Err(i18n::text_untracked(error.label_id()));
            }
            if val_occupied.contains(&candidate) {
                let display = candidate.to_string_lossy().escape_debug().to_string();
                return Err(gds::t_untracked!(
                    "transfer-conflict-rename-exists",
                    name = display
                ));
            }
            Ok(candidate)
        });
    let rename_invalid = Signal::derive_local(move || rename_validation.with(Result::is_err));
    let rename_error = Signal::derive_local(move || rename_validation.get().err());
    let reset_reference = initial_display.clone();
    let rename_modified = Signal::derive_local(move || rename_value.get() != reset_reference);

    // Focus dialog body on initial mount when rename starts collapsed so Escape works immediately
    // without putting a distracting cursor in a hidden or secondary text field.
    let initial_body_focus = !can_return_from_rename;
    let body_focus_effect = RenderEffect::new(move |mounted_before: Option<bool>| {
        let mounted = body_node.get().is_some();
        if !initial_body_focus && mounted && mounted_before != Some(true) {
            body_node.focus();
        }
        mounted
    });
    on_cleanup_local(move || drop(body_focus_effect));

    // Whenever the inline rename input mounts (either on TypeMismatch open or after clicking
    // "Rename…"), focus it and select the suggested filename stem.
    let focus_text = initial_display.clone();
    let input_focus_effect = RenderEffect::new(move |mounted_before: Option<bool>| {
        let mounted = rename_mode.get() && !apply_all.get() && input_node.get().is_some();
        if mounted && mounted_before != Some(true) {
            input_node.focus();
            let current = rename_value.get_untracked();
            let text_to_select = if current == focus_text {
                &focus_text
            } else {
                &current
            };
            input_node.set_value(text_to_select);
            let end = if current == focus_text {
                selection_end
            } else {
                text_to_select.len()
            };
            input_node.set_selection(0..end);
        }
        mounted
    });
    on_cleanup_local(move || drop(input_focus_effect));

    let title = match kind {
        ConflictKind::DirectoryIntoDirectory => {
            gds::t_untracked!("transfer-conflict-merge-title", name = target_label)
        }
        ConflictKind::FileIntoFile => {
            gds::t_untracked!("transfer-conflict-replace-title", name = target_label)
        }
        ConflictKind::TypeMismatch | ConflictKind::SameEntry => {
            gds::t_untracked!("transfer-conflict-title", name = target_label)
        }
    };
    let description = i18n::text_untracked(match kind {
        ConflictKind::DirectoryIntoDirectory => "transfer-conflict-merge-message",
        ConflictKind::FileIntoFile => "transfer-conflict-replace-message",
        ConflictKind::TypeMismatch => {
            if conflict.source.kind == EntryKind::Directory {
                "transfer-conflict-mismatch-dir-over-file"
            } else {
                "transfer-conflict-mismatch-file-over-dir"
            }
        }
        ConflictKind::SameEntry => "transfer-conflict-same-message",
    });
    let (header_icon, header_tone) = match kind {
        ConflictKind::DirectoryIntoDirectory => (IconName::FolderInput, Some("primary")),
        ConflictKind::FileIntoFile => (IconName::Copy, Some("danger")),
        ConflictKind::TypeMismatch | ConflictKind::SameEntry => {
            (IconName::TriangleAlert, Some("danger"))
        }
    };

    let (incoming_parent, existing_parent) =
        distinguishing_parent_labels(&conflict.source.path, &conflict.existing.path);
    let (incoming_badges, existing_badges) =
        comparison_badges(&conflict.source, &conflict.existing);
    let incoming_role_key = match mode {
        transfer::Mode::Copy => "transfer-conflict-incoming-copy",
        transfer::Mode::Move => "transfer-conflict-incoming-move",
    };
    let incoming_card = render_entry_card(
        i18n::text_untracked(incoming_role_key),
        &conflict.source,
        incoming_parent,
        true,
        CardAccent::None,
        incoming_badges,
    );
    let existing_accent = match kind {
        ConflictKind::FileIntoFile => CardAccent::AtRisk,
        ConflictKind::DirectoryIntoDirectory => CardAccent::MergeTarget,
        ConflictKind::TypeMismatch | ConflictKind::SameEntry => CardAccent::None,
    };
    let existing_card = render_entry_card(
        i18n::text_untracked("transfer-conflict-existing"),
        &conflict.existing,
        existing_parent,
        false,
        existing_accent,
        existing_badges,
    );
    let (connector_label, connector_tone) = match kind {
        ConflictKind::FileIntoFile => (
            i18n::text_untracked("transfer-conflict-arrow-replace"),
            Some("danger"),
        ),
        ConflictKind::DirectoryIntoDirectory => (
            i18n::text_untracked("transfer-conflict-arrow-merge"),
            Some("primary"),
        ),
        ConflictKind::TypeMismatch | ConflictKind::SameEntry => {
            (i18n::text_untracked("transfer-conflict-arrow-clash"), None)
        }
    };

    let context_strip = build_context_strip(&conflict);

    let dispatch_ctx = ctx.clone();
    let dispatch_settled = Rc::clone(&settled);
    let dispatch_resolve = Rc::clone(&resolve);
    let dispatch_action: Rc<dyn Fn(ConflictAction)> = Rc::new(move |action: ConflictAction| {
        if dispatch_settled.get()
            || dispatch_ctx.0.prompt_generation.get_untracked() != generation
            || !matches!(
                dispatch_ctx.0.prompt.get_untracked(),
                Prompt::Conflict { .. }
            )
        {
            return;
        }
        dispatch_settled.set(true);
        dispatch_ctx.0.prompt_decisions.clear();
        dispatch_ctx.0.prompt.set(Prompt::Closed);
        dispatch_resolve(ConflictResolution {
            action,
            apply_to_all: apply_all.get_untracked(),
        });
    });

    let confirm_rename_dispatch = Rc::clone(&dispatch_action);
    let confirm_rename_action: Rc<dyn Fn()> =
        Rc::new(move || match rename_validation.get_untracked() {
            Ok(valid_name) => confirm_rename_dispatch(ConflictAction::Rename(valid_name)),
            Err(_) => input_node.focus(),
        });

    let auto_rename_dispatch = Rc::clone(&dispatch_action);
    let auto_rename_suggestion = initial_suggestion.clone();
    let enter_or_auto_rename_action: Rc<dyn Fn()> = Rc::new(move || {
        if apply_all.get_untracked() {
            auto_rename_dispatch(ConflictAction::Rename(auto_rename_suggestion.clone()));
        } else {
            rename_mode.set(true);
        }
    });

    let exit_rename_action: Option<Rc<dyn Fn()>> = can_return_from_rename.then(|| {
        Rc::new(move || {
            rename_mode.set(false);
            body_node.focus();
        }) as Rc<dyn Fn()>
    });

    let skip_dispatch = Rc::clone(&dispatch_action);
    let skip_action: Rc<dyn Fn()> = Rc::new(move || skip_dispatch(ConflictAction::Skip));

    let replace_action: Option<Rc<dyn Fn()>> = (kind == ConflictKind::FileIntoFile).then(|| {
        let replace_dispatch = Rc::clone(&dispatch_action);
        Rc::new(move || replace_dispatch(ConflictAction::Replace)) as Rc<dyn Fn()>
    });
    let merge_action: Option<Rc<dyn Fn()>> =
        (kind == ConflictKind::DirectoryIntoDirectory).then(|| {
            let merge_dispatch = Rc::clone(&dispatch_action);
            Rc::new(move || merge_dispatch(ConflictAction::Merge)) as Rc<dyn Fn()>
        });

    let cancel_ctx = ctx.clone();
    let cancel_settled = Rc::clone(&settled);
    let abort_action: Rc<dyn Fn()> = Rc::new(move || {
        if cancel_settled.get()
            || cancel_ctx.0.prompt_generation.get_untracked() != generation
            || !matches!(cancel_ctx.0.prompt.get_untracked(), Prompt::Conflict { .. })
        {
            return;
        }
        cancel_settled.set(true);
        cancel_ctx.cancel_prompt();
    });

    #[cfg(test)]
    LAST_CONFLICT_ACTIONS.with(|slot| {
        *slot.borrow_mut() = Some(ConflictTestHooks {
            kind,
            relative_dir: conflict.relative_dir.clone(),
            remaining_after_current: conflict.remaining_after_current,
            has_apply_all: conflict.has_more_conflicts_possible,
            apply_all,
            rename_mode,
            rename_value,
            rename_invalid,
            #[cfg(feature = "files-e2e")]
            body_node,
            #[cfg(feature = "files-e2e")]
            batch_node,
            #[cfg(feature = "files-e2e")]
            footer_row_node,
            enter_or_auto_rename: Rc::clone(&enter_or_auto_rename_action),
            exit_rename: exit_rename_action.clone(),
            confirm_rename: Rc::clone(&confirm_rename_action),
            replace: replace_action.clone(),
            merge: merge_action.clone(),
            skip: Rc::clone(&skip_action),
            abort: Rc::clone(&abort_action),
        });
    });

    // Escape in Rename mode cleanly returns to the main decision choices when applicable; a second
    // Escape (or Escape in State 1 / TypeMismatch) aborts the batch.
    let escape_exit = exit_rename_action.clone();
    let escape_abort = Rc::clone(&abort_action);
    let handle_escape: Rc<dyn Fn()> = Rc::new(move || {
        if rename_mode.get_untracked()
            && !apply_all.get_untracked()
            && let Some(exit) = &escape_exit
        {
            exit();
        } else {
            escape_abort();
        }
    });

    // Dialog-level Confirm (`Alt+A`) confirms Rename when editing a name, Merge on folder
    // conflicts, or opens/auto-runs Rename on file conflicts (never triggering destructive Replace
    // on a blind shortcut).
    let confirm_rename_cb = Rc::clone(&confirm_rename_action);
    let confirm_merge_cb = merge_action.clone();
    let confirm_enter_rename_cb = Rc::clone(&enter_or_auto_rename_action);
    let decision_escape = Rc::clone(&handle_escape);
    let decision_abort = Rc::clone(&abort_action);
    let decision_composing = Rc::clone(&composing);
    let decision_cb = UnsyncCallback::new(move |decision: DialogDecision| {
        if decision_composing.get() {
            return;
        }
        match decision {
            DialogDecision::Confirm => {
                if rename_mode.get_untracked() && !apply_all.get_untracked() {
                    confirm_rename_cb();
                } else if let Some(merge) = &confirm_merge_cb {
                    merge();
                } else {
                    confirm_enter_rename_cb();
                }
            }
            DialogDecision::Cancel => decision_escape(),
            DialogDecision::Dismiss => decision_abort(),
        }
    });
    decision_slot.register(false, Some(Rc::clone(&composing)), decision_cb);

    let dialog_escape = Rc::clone(&handle_escape);
    let dialog_composing = Rc::clone(&composing);
    let key_confirm_rename = Rc::clone(&confirm_rename_action);
    let key_escape = Rc::clone(&handle_escape);
    let key_composing = Rc::clone(&composing);
    let start_composing = Rc::clone(&composing);
    let preedit_composing = Rc::clone(&composing);
    let commit_composing = Rc::clone(&composing);
    let end_composing = Rc::clone(&composing);
    let blur_composing = composing;

    let reset_display_value = initial_display;
    let rename_panel = move || {
        let in_manual_rename = rename_mode.get() && !apply_all.get();
        if in_manual_rename {
            let reset_val = reset_display_value.clone();
            let key_confirm = Rc::clone(&key_confirm_rename);
            let key_esc = Rc::clone(&key_escape);
            let k_comp = Rc::clone(&key_composing);
            let s_comp = Rc::clone(&start_composing);
            let p_comp = Rc::clone(&preedit_composing);
            let c_comp = Rc::clone(&commit_composing);
            let e_comp = Rc::clone(&end_composing);
            let b_comp = Rc::clone(&blur_composing);
            Some(AnyView::new(view! {
                column(class = "gds-files-conflict__rename") {
                    row(class = "gds-files-conflict__rename-head") {
                        text(class = "gds-files-conflict__rename-label") {
                            {i18n::text("transfer-conflict-rename-label")}
                        }
                        {{
                            let reset_val = reset_val.clone();
                            move || {
                                rename_modified.get().then(|| {
                                    let reset_val = reset_val.clone();
                                    AnyView::new(view! {
                                        Button(
                                            variant = ButtonVariant::Ghost,
                                            size = ButtonSize::Xs,
                                            on:click = move |_| {
                                                rename_value.set(reset_val.clone());
                                                input_node.set_value(&reset_val);
                                                input_node.focus();
                                                input_node.set_selection(0..selection_end);
                                            },
                                        ) {
                                            {i18n::text("transfer-conflict-rename-reset")}
                                        }
                                    })
                                })
                            }
                        }}
                    }
                    Input(
                        value = rename_value,
                        placeholder = i18n::text("transfer-conflict-placeholder"),
                        invalid = rename_invalid,
                        node_ref = input_node,
                        on:ime_start = move |_| s_comp.set(true),
                        on:ime_preedit = move |event| {
                            let active = match &**event {
                                ImeEvent::Preedit { text, .. } => !text.is_empty(),
                                _ => event.is_composing(),
                            };
                            p_comp.set(active);
                        },
                        on:ime_commit = move |_| c_comp.set(false),
                        on:ime_end = move |_| e_comp.set(false),
                        on:focus_out = move |_| b_comp.set(false),
                        on:key_down = move |event| {
                            if k_comp.get() {
                                return;
                            }
                            if event.key == Key::Named(NamedKey::Enter)
                                && !event.modifiers.control()
                                && !event.modifiers.alt()
                                && !event.modifiers.meta()
                            {
                                event.prevent_default();
                                event.stop_propagation();
                                key_confirm();
                            } else if event.key == Key::Named(NamedKey::Escape) {
                                event.prevent_default();
                                event.stop_propagation();
                                key_esc();
                            } else {
                                handle_dialog_key_down(false, decision_cb, event);
                            }
                        },
                    ) {}
                    {move || rename_error.get().map(|text| view! {
                        text(
                            class = "gds-files-conflict__error",
                            a11y:role = Role::Alert,
                        ) {
                            {text}
                        }
                    })}
                }
            }))
        } else {
            None
        }
    };

    let apply_all_label_key = match kind {
        ConflictKind::FileIntoFile => "transfer-conflict-apply-all-files",
        ConflictKind::DirectoryIntoDirectory => "transfer-conflict-apply-all-folders",
        ConflictKind::TypeMismatch | ConflictKind::SameEntry => "transfer-conflict-apply-all-other",
    };
    let has_more_conflicts_possible = conflict.has_more_conflicts_possible;
    let apply_all_view = move || {
        if !has_more_conflicts_possible || (can_return_from_rename && rename_mode.get()) {
            return None;
        }
        let box_ref = NodeRef::new();
        let lbl_ref = NodeRef::new();
        let checked = Binding::controlled(
            Signal::derive_local(move || {
                if apply_all.get() {
                    Checked::Yes
                } else {
                    Checked::No
                }
            }),
            move |next: Checked| {
                let is_checked = matches!(next, Checked::Yes);
                apply_all.set(is_checked);
                if is_checked && can_return_from_rename {
                    rename_mode.set(false);
                }
            },
        );
        Some(AnyView::new(view! {
            column(node_ref = batch_node, class = "gds-files-conflict__batch") {
                row(class = "gds-files-conflict__apply-all") {
                    Checkbox(
                        node_ref = box_ref,
                        checked = checked,
                        labelled_by = lbl_ref,
                    )
                    Label(node_ref = lbl_ref, control = box_ref) {
                        {i18n::text(apply_all_label_key)}
                    }
                }
            }
        }))
    };

    let footer_abort = Rc::clone(&abort_action);
    let footer_skip = Rc::clone(&skip_action);
    let footer_enter_rename = Rc::clone(&enter_or_auto_rename_action);
    let footer_confirm_rename = Rc::clone(&confirm_rename_action);
    let footer_exit_rename = exit_rename_action;
    let footer_merge = merge_action;
    let footer_replace = replace_action;

    let footer_actions = move || {
        let is_all = apply_all.get();
        let in_manual_rename = rename_mode.get() && !is_all;

        if in_manual_rename {
            let back_btn = footer_exit_rename.clone().map(|exit| {
                AnyView::new(view! {
                    Button(
                        variant = ButtonVariant::Outline,
                        icon = IconName::ArrowLeft,
                        on:click = move |_| exit(),
                    ) {
                        {i18n::text("transfer-conflict-back")}
                    }
                })
            });
            let skip_btn = (!can_return_from_rename).then(|| {
                let skip = Rc::clone(&footer_skip);
                AnyView::new(view! {
                    Button(
                        variant = ButtonVariant::Outline,
                        on:click = move |_| skip(),
                    ) {
                        {i18n::text("transfer-conflict-skip")}
                    }
                })
            });
            let abort = Rc::clone(&footer_abort);
            let confirm = Rc::clone(&footer_confirm_rename);
            AnyView::new(view! {
                row(node_ref = footer_row_node, class = "gds-files-conflict__actions") {
                    {back_btn}
                    Button(
                        variant = ButtonVariant::Outline,
                        icon = IconName::X,
                        on:click = move |_| abort(),
                    ) {
                        {i18n::text("transfer-conflict-abort")}
                    }
                    box(class = "gds-files-conflict__spacer") {}
                    {skip_btn}
                    Button(
                        variant = ButtonVariant::Default,
                        icon = IconName::Check,
                        disabled = rename_invalid,
                        on:click = move |_| confirm(),
                    ) {
                        {i18n::text("transfer-conflict-rename-confirm")}
                    }
                }
            })
        } else {
            let abort = Rc::clone(&footer_abort);
            let skip = Rc::clone(&footer_skip);
            let rename_or_auto = Rc::clone(&footer_enter_rename);
            let skip_key = if is_all {
                "transfer-conflict-skip-all"
            } else {
                "transfer-conflict-skip"
            };
            let rename_key = if is_all {
                "transfer-conflict-keep-both"
            } else {
                "transfer-conflict-rename"
            };
            let rename_icon = if is_all {
                IconName::CopyPlus
            } else {
                IconName::Pencil
            };
            let rename_variant = if footer_merge.is_none() && footer_replace.is_none() {
                ButtonVariant::Default
            } else {
                ButtonVariant::Outline
            };
            let merge_btn = footer_merge.clone().map(|merge| {
                let key = if is_all {
                    "transfer-conflict-merge-all"
                } else {
                    "transfer-conflict-merge"
                };
                AnyView::new(view! {
                    Button(
                        variant = ButtonVariant::Default,
                        icon = IconName::FolderInput,
                        on:click = move |_| merge(),
                    ) {
                        {i18n::text(key)}
                    }
                })
            });
            let replace_btn = footer_replace.clone().map(|replace| {
                let key = if is_all {
                    "transfer-conflict-replace-all"
                } else {
                    "transfer-conflict-replace"
                };
                AnyView::new(view! {
                    Button(
                        variant = ButtonVariant::Destructive,
                        icon = IconName::RefreshCw,
                        on:click = move |_| replace(),
                    ) {
                        {i18n::text(key)}
                    }
                })
            });
            AnyView::new(view! {
                row(node_ref = footer_row_node, class = "gds-files-conflict__actions") {
                    Button(
                        variant = ButtonVariant::Outline,
                        icon = IconName::X,
                        on:click = move |_| abort(),
                    ) {
                        {i18n::text("transfer-conflict-abort")}
                    }
                    box(class = "gds-files-conflict__spacer") {}
                    Button(
                        variant = ButtonVariant::Outline,
                        on:click = move |_| skip(),
                    ) {
                        {i18n::text(skip_key)}
                    }
                    Button(
                        variant = rename_variant,
                        icon = rename_icon,
                        on:click = move |_| rename_or_auto(),
                    ) {
                        {i18n::text(rename_key)}
                    }
                    {merge_btn}
                    {replace_btn}
                }
            })
        }
    };

    AnyView::new(view! {
        column(
            node_ref = body_node,
            tabindex = Focus::Programmatic,
            class = "gds-files-conflict",
            attr:data-conflict-kind = match kind {
                ConflictKind::DirectoryIntoDirectory => "merge",
                ConflictKind::FileIntoFile => "replace",
                ConflictKind::TypeMismatch => "mismatch",
                ConflictKind::SameEntry => "same",
            },
            on:key_down = move |event| {
                if dialog_composing.get() {
                    return;
                }
                if event.key == Key::Named(NamedKey::Escape) {
                    event.prevent_default();
                    event.stop_propagation();
                    dialog_escape();
                    return;
                }
                handle_dialog_key_down(false, decision_cb, event);
            },
        ) {
            DialogHeader {
                row(class = "gds-files-conflict__title-row") {
                    Icon(
                        name = header_icon,
                        size = 18.0,
                        class = "gds-files-conflict__title-icon",
                        attr:data-tone = header_tone.map(str::to_owned),
                    )
                    DialogTitle(class = "gds-files-conflict__title-text") { {title} }
                }
                DialogDescription { {description} }
            }
            {context_strip}
            column(class = "gds-files-conflict__compare") {
                {incoming_card}
                row(
                    class = "gds-files-conflict__connector",
                    attr:data-tone = connector_tone.map(str::to_owned),
                ) {
                    Icon(name = IconName::ArrowDown, size = 14.0)
                    text { {connector_label} }
                }
                {existing_card}
            }
            {rename_panel}
            {apply_all_view}
            DialogFooter {
                {footer_actions}
            }
        }
    })
}

fn build_context_strip(conflict: &PasteConflict) -> Option<AnyView> {
    let show_location = conflict.relative_dir.is_some();
    let remaining = conflict.remaining_after_current;
    if !show_location && remaining == 0 {
        return None;
    }
    let location_text = format_destination_location(
        &conflict.task.destination_dir,
        conflict.relative_dir.as_deref(),
    );
    let location_label = gds::t_untracked!("transfer-conflict-location", location = location_text);
    let remaining_view = (remaining > 0).then(|| {
        let label = gds::i18n::count_untracked("transfer-conflict-remaining", remaining);
        view! {
            text(class = "gds-files-conflict__context-remaining") { {label} }
        }
    });
    Some(AnyView::new(view! {
        row(class = "gds-files-conflict__context") {
            text(class = "gds-files-conflict__context-location") { {location_label} }
            {remaining_view}
        }
    }))
}

fn format_destination_location(dest_dir: &Path, relative_dir: Option<&Path>) -> String {
    if let Some(rel) = relative_dir {
        let root_name = dest_dir
            .ancestors()
            .nth(rel.components().count())
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned());
        let rel_display = rel.to_string_lossy();
        match root_name {
            Some(root) if !root.is_empty() => format!("{root}/{rel_display}"),
            _ => rel_display.into_owned(),
        }
    } else {
        compact_tail_path(dest_dir, 3)
    }
}

fn compact_tail_path(path: &Path, max_components: usize) -> String {
    let segments: Vec<String> = path
        .components()
        .filter_map(|comp| match comp {
            std::path::Component::Normal(seg) => Some(seg.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if segments.is_empty() {
        return path.to_string_lossy().into_owned();
    }
    if segments.len() <= max_components {
        segments.join("/")
    } else {
        let tail = &segments[segments.len() - max_components..];
        format!("…/{}", tail.join("/"))
    }
}

/// Returns compact parent-directory labels for `source` and `existing` that include enough
/// trailing components to show where the two paths actually diverge (avoiding two cards that both
/// just say `sub`).
fn distinguishing_parent_labels(
    source: &Path,
    existing: &Path,
) -> (Option<String>, Option<String>) {
    let Some(src_parent) = source.parent() else {
        return (None, existing.parent().map(|p| compact_tail_path(p, 2)));
    };
    let Some(dst_parent) = existing.parent() else {
        return (Some(compact_tail_path(src_parent, 2)), None);
    };
    let to_segments = |dir: &Path| -> Vec<String> {
        dir.components()
            .filter_map(|comp| match comp {
                std::path::Component::Normal(seg) => Some(seg.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    let src_segs = to_segments(src_parent);
    let dst_segs = to_segments(dst_parent);
    if src_segs.is_empty() || dst_segs.is_empty() {
        return (
            Some(src_parent.to_string_lossy().into_owned()),
            Some(dst_parent.to_string_lossy().into_owned()),
        );
    }
    // Count matching trailing components (e.g. `.../Project/sub` vs `.../Project/sub`) and take
    // one extra component so the diverging parent (`src` vs `dst`) is always visible.
    let matching_tail = src_segs
        .iter()
        .rev()
        .zip(dst_segs.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let keep = (matching_tail + 1).clamp(1, 4);
    (
        Some(compact_tail_path(src_parent, keep)),
        Some(compact_tail_path(dst_parent, keep)),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CardAccent {
    None,
    AtRisk,
    MergeTarget,
}

struct Badge {
    label: String,
    muted: bool,
}

fn render_entry_card(
    role_label: String,
    info: &EntryInfo,
    parent_label: Option<String>,
    is_source: bool,
    accent: CardAccent,
    badges: Vec<Badge>,
) -> AnyView {
    let icon = match info.kind {
        EntryKind::Directory => IconName::Folder,
        EntryKind::Symlink => IconName::Link,
        EntryKind::File | EntryKind::Other => IconName::File,
    };
    let display_name = info.name.to_string_lossy().into_owned();
    let meta = format_entry_meta(info, parent_label, is_source);
    let badge_views = badges
        .into_iter()
        .map(|badge| {
            let tone = badge.muted.then(|| "muted".to_owned());
            view! {
                text(
                    class = "gds-files-conflict__badge",
                    attr:data-tone = tone,
                ) {
                    {badge.label}
                }
            }
            .into_view()
        })
        .collect::<Vec<_>>();
    AnyView::new(view! {
        row(
            class = "gds-files-conflict__card",
            attr:data-at-risk = (accent == CardAccent::AtRisk).then(|| "true".to_owned()),
            attr:data-merge-target = (accent == CardAccent::MergeTarget).then(|| "true".to_owned()),
        ) {
            Icon(
                name = icon,
                size = 18.0,
                class = "gds-files-conflict__card-icon",
            )
            column(class = "gds-files-conflict__card-body") {
                row(class = "gds-files-conflict__card-head") {
                    text(class = "gds-files-conflict__card-role") { {role_label} }
                    {badge_views}
                }
                text(class = "gds-files-conflict__card-name") { {display_name} }
                text(class = "gds-files-conflict__card-meta") { {meta} }
            }
        }
    })
}

fn format_entry_meta(info: &EntryInfo, parent_label: Option<String>, is_source: bool) -> String {
    let mut parts = Vec::with_capacity(4);
    if let Some(folder) = parent_label.filter(|f| !f.is_empty()) {
        let folder_part = if is_source {
            gds::t_untracked!("transfer-conflict-from-folder", folder = folder)
        } else {
            gds::t_untracked!("transfer-conflict-in-folder", folder = folder)
        };
        parts.push(folder_part);
    }
    match info.kind {
        EntryKind::Directory => {
            parts.push(i18n::text_untracked("kind-directory"));
            if let Some(items) = info.items {
                let key = if items.truncated {
                    "directory-item-count-at-least"
                } else {
                    "directory-item-count"
                };
                parts.push(gds::i18n::count_untracked(key, items.count));
            }
        }
        EntryKind::File => {
            if let Some(size) = info.size {
                parts.push(EntryPresentation::format_size(size));
            }
        }
        EntryKind::Symlink => {
            parts.push(i18n::text_untracked("kind-link"));
        }
        EntryKind::Other => {
            parts.push(i18n::text_untracked("kind-other"));
        }
    }
    if let TimePresentation::Absolute { text } =
        TimeFormat::Absolute.presentation(info.modified, clock_now())
    {
        parts.push(text);
    }
    if parts.is_empty() {
        "—".to_owned()
    } else {
        parts.join(" · ")
    }
}

fn comparison_badges(incoming: &EntryInfo, existing: &EntryInfo) -> (Vec<Badge>, Vec<Badge>) {
    let mut incoming_badges = Vec::new();
    let mut existing_badges = Vec::new();

    let same_second = match (incoming.modified, existing.modified) {
        (Some(a), Some(b)) => {
            let secs =
                |t: std::time::SystemTime| t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs());
            secs(a).is_some() && secs(a) == secs(b)
        }
        _ => false,
    };

    if incoming.kind == EntryKind::File
        && existing.kind == EntryKind::File
        && incoming.size.is_some()
        && incoming.size == existing.size
        && same_second
    {
        incoming_badges.push(Badge {
            label: i18n::text_untracked("transfer-conflict-badge-identical"),
            muted: true,
        });
        return (incoming_badges, existing_badges);
    }

    if let (Some(incoming_time), Some(existing_time)) = (incoming.modified, existing.modified)
        && !same_second
    {
        if incoming_time > existing_time {
            incoming_badges.push(Badge {
                label: i18n::text_untracked("transfer-conflict-badge-newer"),
                muted: false,
            });
        } else if existing_time > incoming_time {
            existing_badges.push(Badge {
                label: i18n::text_untracked("transfer-conflict-badge-newer"),
                muted: false,
            });
        }
    }
    if incoming.kind == EntryKind::File
        && existing.kind == EntryKind::File
        && let (Some(incoming_size), Some(existing_size)) = (incoming.size, existing.size)
    {
        if incoming_size > existing_size {
            incoming_badges.push(Badge {
                label: i18n::text_untracked("transfer-conflict-badge-larger"),
                muted: false,
            });
        } else if existing_size > incoming_size {
            existing_badges.push(Badge {
                label: i18n::text_untracked("transfer-conflict-badge-larger"),
                muted: false,
            });
        }
    }
    (incoming_badges, existing_badges)
}

fn initial_selection_end(display: &str, kind: EntryKind) -> usize {
    if kind != EntryKind::Directory
        && let Some(parts) = names::split_name(OsStr::new(display))
        && let Some(stem) = parts.stem.to_str()
        && display.starts_with(stem)
        && display.as_bytes().get(stem.len()) == Some(&b'.')
    {
        return stem.len();
    }
    display.len()
}

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd")))]
mod tests;
