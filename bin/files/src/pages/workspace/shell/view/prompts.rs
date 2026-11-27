//! Modal prompt host overlays (About, Confirm, NameForm, and PasteConflict).

use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    rc::Rc,
};

use gds::i18n;
use gds::ui::IconName;
use gds::ui::about::{AboutDialogProps, Application};
use gds::ui::feedback::dialog::{ConfirmDialogProps, DialogDecision, DialogSize, ModalDialogProps};
use zgui::prelude::*;
use zgui::reactive::{
    RenderEffect, RwSignal, Signal, UnsyncCallback, enter_non_reactive_zone, on_cleanup_local,
};
use zgui::view;
use zgui::view::Scoped;
use zgui_ui_primitives::Binding;

use crate::pages::workspace::{
    panes::{render_conflict_dialog, render_name_form},
    state::{context::Context, model::Prompt, name_form::NameForm},
};

const CONFIRM_PREVIEW_LIMIT: usize = 5;

pub(super) fn prompt_dialogs(ctx: &Context) -> AnyView {
    let ctx = ctx.clone();
    AnyView::new(Scoped::new(move || {
        let prompt_sig = ctx.0.prompt;
        let gen_sig = ctx.0.prompt_generation;

        // 1. `Prompt::About` -> `AboutDialog` (`DialogSize::Sm = 420px`)
        let about_open = Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::About));
        let about_cancel = ctx.clone();

        // 2. `Prompt::Confirm` -> `ConfirmDialog` (`DialogSize::Sm = 420px`)
        let confirm_open =
            Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::Confirm { .. }));
        let confirm_title = RwSignal::new_local(String::new());
        let confirm_message = RwSignal::new_local(String::new());
        let confirm_primary = RwSignal::new_local(String::new());
        let confirm_cancel_lbl = Signal::derive_local(|| i18n::text("operation-cancel"));
        let confirm_icon = RwSignal::new_local(Option::<IconName>::None);
        let confirm_primary_icon = RwSignal::new_local(IconName::Check);
        let confirm_items = RwSignal::new_local(Vec::<String>::new());
        let confirm_overflow = RwSignal::new_local(Option::<String>::None);
        let confirm_destructive = RwSignal::new_local(false);
        let confirm_cb_slot: Rc<RefCell<Option<UnsyncCallback<DialogDecision>>>> =
            Rc::new(RefCell::new(None));
        {
            let cb_ctx = ctx.clone();
            let slot = Rc::clone(&confirm_cb_slot);
            let sync_confirm_fx = RenderEffect::new(move |_| {
                let generation = gen_sig.get();
                let _z = enter_non_reactive_zone();
                match cb_ctx.0.prompt.get_untracked() {
                    Prompt::Confirm {
                        title,
                        message,
                        primary,
                        accept,
                    } => {
                        let options = cb_ctx.0.confirm_options.get_untracked();
                        let shown_items: Vec<String> = options
                            .items
                            .iter()
                            .take(CONFIRM_PREVIEW_LIMIT)
                            .cloned()
                            .collect();
                        let extra_from_slice =
                            options.items.len().saturating_sub(CONFIRM_PREVIEW_LIMIT);
                        let remaining = options.overflow_count.saturating_add(extra_from_slice);
                        let overflow_line =
                            (remaining > 0).then(|| i18n::count("operation-more-items", remaining));

                        confirm_title.set(title);
                        confirm_message.set(message);
                        confirm_primary.set(primary);
                        confirm_icon.set(options.icon);
                        confirm_primary_icon.set(options.primary_icon.unwrap_or(IconName::Check));
                        confirm_items.set(shown_items);
                        confirm_overflow.set(overflow_line);
                        confirm_destructive.set(options.destructive);

                        let weak = cb_ctx.downgrade();
                        let settled = Rc::new(Cell::new(false));
                        let decision_cb = UnsyncCallback::new(move |decision: DialogDecision| {
                            if settled.get() {
                                return;
                            }
                            let Some(c) = weak.upgrade() else {
                                return;
                            };
                            if c.0.prompt_generation.get_untracked() != generation
                                || !matches!(c.0.prompt.get_untracked(), Prompt::Confirm { .. })
                            {
                                return;
                            }
                            settled.set(true);
                            match decision {
                                DialogDecision::Confirm => {
                                    c.0.prompt_decisions.clear();
                                    c.0.prompt.set(Prompt::Closed);
                                    accept();
                                }
                                DialogDecision::Cancel | DialogDecision::Dismiss => {
                                    c.cancel_prompt();
                                }
                            }
                        });
                        *slot.borrow_mut() = Some(decision_cb);
                        cb_ctx.0.prompt_decisions.register(false, None, decision_cb);
                    }
                    Prompt::Closed => {
                        cb_ctx.0.prompt_decisions.clear();
                    }
                    _ => {}
                }
            });
            on_cleanup_local(move || drop(sync_confirm_fx));
        }
        let cb_slot_run = confirm_cb_slot;

        // 3. `Prompt::Name` -> `ModalDialog` (`DialogSize::Sm`)
        let name_open =
            Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::Name { .. }));
        let name_open_cancel = ctx.clone();
        let name_decision_cancel = ctx.clone();
        let name_decisions = ctx.0.prompt_decisions.clone();
        let name_body_ctx = ctx.clone();
        let retained_name: Rc<RefCell<Option<(NameForm, Rc<dyn Fn(OsString)>, u64)>>> =
            Rc::new(RefCell::new(None));

        // 4. `Prompt::Conflict` -> `ModalDialog` (`DialogSize::Md`)
        let conflict_open =
            Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::Conflict { .. }));
        let conflict_open_cancel = ctx.clone();
        let conflict_decision_cancel = ctx.clone();
        let conflict_decisions = ctx.0.prompt_decisions.clone();
        let conflict_body_ctx = ctx;
        type RetainedConflict = (
            crate::features::file_operations::transfer::Mode,
            crate::features::file_operations::paste::PasteConflict,
            Rc<dyn Fn(crate::features::file_operations::paste::ConflictResolution)>,
            u64,
        );
        let retained_conflict: Rc<RefCell<Option<RetainedConflict>>> = Rc::new(RefCell::new(None));

        view! {
            AboutDialog(
                open = Binding::controlled(
                    about_open,
                    move |next: bool| {
                        if !next && matches!(about_cancel.0.prompt.get_untracked(), Prompt::About) {
                            about_cancel.cancel_prompt();
                        }
                    },
                ),
                application = Application::cargo("gds-files"),
            )
            ConfirmDialog(
                open = Binding::controlled(confirm_open, |_| {}),
                size = DialogSize::Sm,
                title = confirm_title,
                description = confirm_message,
                icon = confirm_icon,
                primary_icon = confirm_primary_icon,
                items = confirm_items,
                overflow_text = confirm_overflow,
                confirm_label = confirm_primary,
                cancel_label = confirm_cancel_lbl,
                destructive = confirm_destructive,
                generation = gen_sig,
                on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                    let cb = *cb_slot_run.borrow();
                    if let Some(cb) = cb {
                        cb.run(decision);
                    }
                }),
            )
            ModalDialog(
                open = Binding::controlled(
                    name_open,
                    move |next: bool| {
                        if !next
                            && matches!(
                                name_open_cancel.0.prompt.get_untracked(),
                                Prompt::Name { .. }
                            )
                        {
                            name_open_cancel.cancel_prompt();
                        }
                    },
                ),
                size = DialogSize::Sm,
                close_on_confirm = false,
                on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                    match decision {
                        DialogDecision::Confirm => {
                            let _ = name_decisions.confirm();
                        }
                        DialogDecision::Cancel | DialogDecision::Dismiss => {
                            if matches!(
                                name_decision_cancel.0.prompt.get_untracked(),
                                Prompt::Name { .. }
                            ) {
                                name_decision_cancel.cancel_prompt();
                            }
                        }
                    }
                }),
            ) {
                {{
                    let pb = name_body_ctx.clone();
                    let retained = Rc::clone(&retained_name);
                    move || {
                        let generation = gen_sig.get();
                        let _z = enter_non_reactive_zone();
                        if let Prompt::Name { form, submit } = pb.0.prompt.get_untracked() {
                            *retained.borrow_mut() =
                                Some((form.clone(), Rc::clone(&submit), generation));
                            render_name_form(
                                form,
                                &pb,
                                generation,
                                submit,
                                &pb.0.prompt_decisions,
                            )
                        } else if let Some((form, submit, retained_gen)) =
                            retained.borrow().clone()
                        {
                            render_name_form(
                                form,
                                &pb,
                                retained_gen,
                                submit,
                                &pb.0.prompt_decisions,
                            )
                        } else {
                            AnyView::new(())
                        }
                    }
                }}
            }
            ModalDialog(
                open = Binding::controlled(
                    conflict_open,
                    move |next: bool| {
                        if !next
                            && matches!(
                                conflict_open_cancel.0.prompt.get_untracked(),
                                Prompt::Conflict { .. }
                            )
                        {
                            conflict_open_cancel.cancel_prompt();
                        }
                    },
                ),
                size = DialogSize::Md,
                class = "gds-files-conflict-modal",
                dismiss_on_outside_press = false,
                dismiss_on_escape = false,
                close_on_confirm = false,
                on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                    match decision {
                        DialogDecision::Confirm => {
                            let _ = conflict_decisions.confirm();
                        }
                        DialogDecision::Cancel | DialogDecision::Dismiss => {
                            if matches!(
                                conflict_decision_cancel.0.prompt.get_untracked(),
                                Prompt::Conflict { .. }
                            ) {
                                conflict_decision_cancel.cancel_prompt();
                            }
                        }
                    }
                }),
            ) {
                {{
                    let pb = conflict_body_ctx.clone();
                    let retained = Rc::clone(&retained_conflict);
                    move || {
                        let generation = gen_sig.get();
                        let _z = enter_non_reactive_zone();
                        if let Prompt::Conflict {
                            mode,
                            conflict,
                            resolve,
                            ..
                        } = pb.0.prompt.get_untracked()
                        {
                            *retained.borrow_mut() = Some((
                                mode,
                                conflict.clone(),
                                Rc::clone(&resolve),
                                generation,
                            ));
                            render_conflict_dialog(
                                &pb,
                                generation,
                                mode,
                                conflict,
                                resolve,
                                &pb.0.prompt_decisions,
                            )
                        } else if let Some((mode, conflict, resolve, retained_gen)) =
                            retained.borrow().clone()
                        {
                            render_conflict_dialog(
                                &pb,
                                retained_gen,
                                mode,
                                conflict,
                                resolve,
                                &pb.0.prompt_decisions,
                            )
                        } else {
                            AnyView::new(())
                        }
                    }
                }}
            }
        }
    }))
}
