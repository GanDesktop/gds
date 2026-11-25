//! Native modal dialog scope, footer actions, decisions, and prompt hosts over `zgui-ui::Dialog`.

mod anchor;
mod command;
mod decision;
mod footer;

pub use anchor::DialogAnchor;
pub use command::DialogCommand;
pub use decision::DialogDecision;
pub use footer::{DialogFooter, DialogFooterProps};
pub use zgui_ui::{
    Dialog, DialogClose, DialogCloseProps, DialogContent, DialogContentProps, DialogDescription,
    DialogDescriptionProps, DialogHeader, DialogHeaderProps, DialogProps, DialogTitle,
    DialogTitleProps,
};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, RenderEffect, Signal, UnsyncCallback, on_cleanup_local, provide_local_context,
};
use zgui::view::{ListenerGuard, current_host, events};
use zgui::vocab::{Key, NamedKey};
use zgui::{component, css, view};
use zgui_ui::overlay::OverlayState;
use zgui_ui_primitives::Binding;

use crate::app::Gds;
use crate::ui::button::{ButtonProps, ButtonVariant};
use crate::ui::icon::{IconName, IconProps};
use crate::ui::keytips::{
    DialogScopeId, KeytipRegistry, allocate_dialog_scope_id, push_modal_scope, remove_modal_scope,
};

/// Canonical modal width tiers (`Sm = 420px`, `Md = 500px`, `Lg = 640px`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialogSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl DialogSize {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
        }
    }
}

const SHEET: &str = css!(
    r#"
    .zui-dialog,
    .zui-dialog.gds-dialog-content {
        --zui-surface-fill: var(--gds-panel);
        --zui-surface-ink: var(--gds-panel-foreground);
        --zui-surface-border: 1px solid var(--gds-border);
        --zui-surface-radius: 8px;
        background-color: var(--gds-panel);
        color: var(--gds-panel-foreground);
        border: 1px solid var(--gds-border);
        border-radius: 8px;
        font-family: var(--gds-font-sans);
        font-size: var(--gds-text-sm, 14px);
    }

    .zui-dialog.gds-dialog-content {
        width: 500px;
        min-width: 0px;
        max-width: calc(100vw - 32px);
        box-sizing: border-box;
    }

    .zui-dialog.gds-dialog-content[data-size="sm"] {
        width: 420px;
    }

    .zui-dialog.gds-dialog-content[data-size="md"] {
        width: 500px;
    }

    .zui-dialog.gds-dialog-content[data-size="lg"] {
        width: 640px;
    }

    .zui-dialog .zui-dialog__header {
        padding-right: 32px;
    }

    .zui-dialog .zui-dialog__title {
        font-size: var(--gds-text-md, 16px);
    }

    .zui-dialog .zui-dialog__description {
        font-size: var(--gds-text-sm, 14px);
    }

    .zui-dialog .zui-dialog__dismiss {
        top: 16px;
        right: 16px;
        width: var(--gds-icon-button-size, 32px);
        height: var(--gds-icon-button-size, 32px);
        padding: 0;
        border-radius: var(--gds-radius-control, 6px);
        background-color: transparent;
        color: var(--gds-muted-foreground);
        opacity: 1;
        cursor: pointer;
        box-shadow: none;
        outline: none;
        transition:
            background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .zui-dialog .zui-dialog__dismiss:hover {
        background-color: var(--gds-hover);
        color: var(--gds-foreground);
        opacity: 1;
    }

    .zui-dialog .zui-dialog__dismiss:active {
        background-color: var(--gds-pressed);
        color: var(--gds-foreground);
    }

    .zui-dialog .zui-dialog__dismiss:focus-visible {
        box-shadow: none;
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-dialog-actions {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
    }

    .gds-confirm-dialog__body {
        display: flex;
        flex-direction: column;
        gap: 16px;
        outline: none;
    }

    .gds-confirm-dialog__title-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }

    .gds-confirm-dialog__title-icon[data-destructive="true"] {
        color: var(--gds-error);
    }

    .gds-confirm-dialog__items {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 8px 12px;
        border-radius: var(--gds-radius-control, 6px);
        background-color: var(--gds-surface);
        border: 1px solid var(--gds-border);
    }

    .gds-confirm-dialog__item {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-confirm-dialog__more {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }
    "#
);

fn install_dialog_stylesheet() {
    if current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-dialog", SHEET);
    }
}

fn default_label_signal(id: &'static str, fallback: &'static str) -> Signal<String, LocalStorage> {
    Signal::derive_local(move || {
        Gds::try_current().map_or_else(|| fallback.to_owned(), |gds| gds.i18n().text(id))
    })
}

/// Dispatches `Alt+A` (`DialogDecision::Confirm`, when `!disabled`) and `Alt+C`
/// (`DialogDecision::Cancel`) on a modal dialog key event.
pub fn handle_dialog_key_down(
    disabled: bool,
    on_decision: UnsyncCallback<DialogDecision>,
    ev: &mut EventCx<'_, events::KeyDown>,
) {
    if !disabled && DialogCommand::Confirm.matches_event(ev, ev.modifiers) {
        on_decision.run(DialogDecision::Confirm);
        ev.prevent_default();
        ev.stop_propagation();
    } else if DialogCommand::Cancel.matches_event(ev, ev.modifiers) {
        on_decision.run(DialogDecision::Cancel);
        ev.prevent_default();
        ev.stop_propagation();
    }
}

/// Active modal dialog scope guard that suppresses background workspace keytips/shortcuts while
/// keeping keytips inside the modal dialog active (`shortcuts.md` §4).
#[derive(Clone, Copy, Debug)]
pub struct DialogScope {
    id: DialogScopeId,
}

impl DialogScope {
    /// Enters a modal dialog scope for the lifetime of the current mounted reactive scope
    /// (such as inside `DialogContent` or `ModalDialog`).
    #[must_use]
    pub fn enter() -> Self {
        let raw_id = allocate_dialog_scope_id();
        let id = DialogScopeId(raw_id);
        let registry = KeytipRegistry::current();
        if zgui::reactive::is_ui_thread() {
            provide_local_context(id);
        }
        push_modal_scope(registry, raw_id);
        on_cleanup_local(move || remove_modal_scope(registry, raw_id));
        Self { id }
    }

    /// Binds a modal dialog scope whose active state tracks `open` reactively.
    #[must_use]
    pub fn use_open(open: Signal<bool, LocalStorage>) -> Self {
        let raw_id = allocate_dialog_scope_id();
        let id = DialogScopeId(raw_id);
        let registry = KeytipRegistry::current();
        if zgui::reactive::is_ui_thread() {
            provide_local_context(id);
        }
        let fx = RenderEffect::new(move |_| {
            if open.get() {
                push_modal_scope(registry, raw_id);
            } else {
                remove_modal_scope(registry, raw_id);
            }
        });
        on_cleanup_local(move || {
            drop(fx);
            remove_modal_scope(registry, raw_id);
        });
        Self { id }
    }

    /// Returns this modal dialog scope's identifier.
    #[must_use]
    pub const fn id(self) -> DialogScopeId {
        self.id
    }

    /// Builds a `KEY_DOWN` listener dispatching `Alt+A` (`Confirm`) and `Alt+C` (`Cancel`).
    pub fn key_handler(
        disabled: Signal<bool, LocalStorage>,
        on_decision: UnsyncCallback<DialogDecision>,
    ) -> impl Fn(&mut EventCx<'_, events::KeyDown>) {
        move |ev| {
            handle_dialog_key_down(disabled.get_untracked(), on_decision, ev);
        }
    }
}

/// Standard GDS modal footer rendering neutral `Outline` Cancel (`IconName::X`, `Alt+C` with
/// `140ms` inline keytip `C`) and `Default` / `Destructive` Primary (`primary_icon` default
/// `IconName::Check`, `Alt+A` with `140ms` inline keytip `A`).
#[allow(clippy::too_many_arguments)]
#[component]
pub fn DialogActions(
    #[prop(into, optional)] confirm_label: Option<Signal<String, LocalStorage>>,
    #[prop(into, optional)] primary_label: Option<Signal<String, LocalStorage>>,
    #[prop(default = IconName::Check)] primary_icon: IconName,
    #[prop(into, optional)] cancel_label: Option<Signal<String, LocalStorage>>,
    #[prop(default = true)] show_cancel: bool,
    #[prop(default = false)] destructive: bool,
    #[prop(default = false)] danger: bool,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(optional)] on_decision: Option<UnsyncCallback<DialogDecision>>,
) -> impl IntoView {
    install_dialog_stylesheet();

    // When mounted inside a raw `DialogContent` without an outer `ModalDialog`, automatically
    // establish a `DialogScope` and bind `Alt+A` / `Alt+C` on the enclosing modal surface.
    let inherited_scope = DialogScopeId::current();
    if inherited_scope.is_none() {
        let _scope = DialogScope::enter();
        if let Some(decision_cb) = on_decision
            && let Some(overlay) = OverlayState::current()
        {
            let content_node = overlay.content();
            let key_guard: Rc<RefCell<Option<ListenerGuard>>> = Rc::new(RefCell::new(None));
            let key_guard_watch = Rc::clone(&key_guard);
            let attach_fx = RenderEffect::new(move |_| {
                if content_node.get().is_none() || key_guard_watch.borrow().is_some() {
                    return;
                }
                let guard = content_node.listen(
                    events::KEY_DOWN,
                    ListenerOptions::DEFAULT,
                    move |ev: &mut EventCx<'_, events::KeyDown>| {
                        handle_dialog_key_down(disabled.get_untracked(), decision_cb, ev);
                    },
                );
                *key_guard_watch.borrow_mut() = guard;
            });
            on_cleanup_local(move || {
                drop(attach_fx);
                drop(key_guard.borrow_mut().take());
            });
        }
    }

    let resolved_confirm = primary_label
        .or(confirm_label)
        .unwrap_or_else(|| default_label_signal(DialogCommand::Confirm.label_id(), "Confirm"));
    let resolved_cancel = cancel_label
        .unwrap_or_else(|| default_label_signal(DialogCommand::Cancel.label_id(), "Cancel"));

    let confirm_variant = if destructive || danger {
        ButtonVariant::Destructive
    } else {
        ButtonVariant::Default
    };

    let cancel_cb = on_decision;
    let confirm_cb = on_decision;

    view! {
        row(class = "gds-dialog-actions") {
            {show_cancel.then(move || {
                view! {
                    Button(
                        variant = ButtonVariant::Outline,
                        icon = IconName::X,
                        tooltip = resolved_cancel,
                        shortcut = DialogCommand::Cancel.default_stroke(),
                        a11y:role = Role::Button,
                        a11y:label = move || resolved_cancel.get(),
                        on:click = move |_| {
                            if let Some(cb) = cancel_cb {
                                cb.run(DialogDecision::Cancel);
                            }
                        }
                    ) {{move || resolved_cancel.get()}}
                }
                .into_view()
            })}
            Button(
                variant = confirm_variant,
                icon = primary_icon,
                disabled = disabled,
                tooltip = resolved_confirm,
                shortcut = DialogCommand::Confirm.default_stroke(),
                a11y:role = Role::Button,
                a11y:label = move || resolved_confirm.get(),
                on:click = move |_| {
                    if let Some(cb) = confirm_cb {
                        cb.run(DialogDecision::Confirm);
                    }
                }
            ) {{move || resolved_confirm.get()}}
        }
    }
}

/// Reusable GDS modal dialog wrapper binding [`DialogScope`] (suppressing background workspace
/// keytips while open), `Alt+A` / `Alt+C` decision shortcuts, `Esc` / scrim dismissal, and
/// [`DialogAnchor`] focus restoration.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn ModalDialog(
    #[prop(into, optional)] open: Binding<bool>,
    #[prop(into, default = Signal::stored_local(DialogSize::Md))] size: Signal<
        DialogSize,
        LocalStorage,
    >,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(true))] dismiss_on_outside_press: Signal<
        bool,
        LocalStorage,
    >,
    #[prop(into, default = Signal::stored_local(true))] dismiss_on_escape: Signal<
        bool,
        LocalStorage,
    >,
    #[prop(default = true)] dismiss_control: bool,
    #[prop(default = true)] close_on_confirm: bool,
    #[prop(optional)] anchor: Option<DialogAnchor>,
    #[prop(optional)] on_decision: Option<UnsyncCallback<DialogDecision>>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: ChildrenFn,
) -> impl IntoView {
    install_dialog_stylesheet();

    let settled = Rc::new(Cell::new(false));
    {
        let settled = Rc::clone(&settled);
        let reset_fx = zgui::reactive::RenderEffect::new(move |prev: Option<bool>| {
            let now_open = open.get().unwrap_or(false);
            if now_open && prev != Some(true) {
                settled.set(false);
            } else if !now_open
                && prev == Some(true)
                && !settled.get()
                && let Some(anchor) = anchor
            {
                anchor.restore();
            }
            now_open
        });
        on_cleanup_local(move || drop(reset_fx));
    }
    let finish = {
        let settled = Rc::clone(&settled);
        UnsyncCallback::new(move |decision: DialogDecision| {
            if settled.get() {
                return;
            }
            if decision == DialogDecision::Confirm && !close_on_confirm {
                if let Some(cb) = on_decision {
                    cb.run(DialogDecision::Confirm);
                }
                return;
            }
            settled.set(true);
            open.write(false);
            if let Some(cb) = on_decision {
                cb.run(decision);
            }
            if let Some(anchor) = anchor {
                anchor.restore();
            }
        })
    };

    let on_open_change = {
        let settled = Rc::clone(&settled);
        UnsyncCallback::new(move |next_open: bool| {
            open.write(next_open);
            if next_open {
                settled.set(false);
            } else if !settled.replace(true) {
                if let Some(cb) = on_decision {
                    cb.run(DialogDecision::Dismiss);
                }
                if let Some(anchor) = anchor {
                    anchor.restore();
                }
            }
        })
    };

    view! {
        Dialog(open = open, on_open_change = on_open_change) {
            DialogContent(
                class = "gds-dialog-content",
                class = class,
                attr:data-size = move || Some(size.get().as_str().to_owned()),
                dismiss_on_outside_press = dismiss_on_outside_press,
                dismiss_on_escape = dismiss_on_escape,
                dismiss_control = dismiss_control,
                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                    handle_dialog_key_down(disabled.get_untracked(), finish, ev);
                },
                {..attrs}
            ) {
                {{
                    let _scope = DialogScope::enter();
                    children.view()
                }}
            }
        }
    }
}

/// Controlled confirmation modal built over [`ModalDialog`] with focus trap, `Escape`
/// dismissal, outside scrim dismissal, mnemonic `Alt+A` / `Alt+C` actions, optional header icon
/// and bounded preview list, and focus restoration to `anchor`.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn ConfirmDialog(
    #[prop(into, optional)] open: Binding<bool>,
    #[prop(into, default = Signal::stored_local(DialogSize::Sm))] size: Signal<
        DialogSize,
        LocalStorage,
    >,
    #[prop(into)] title: Signal<String, LocalStorage>,
    #[prop(into, optional)] description: Option<Signal<String, LocalStorage>>,
    #[prop(into, default = Signal::stored_local(None))] icon: Signal<
        Option<IconName>,
        LocalStorage,
    >,
    #[prop(into, default = Signal::stored_local(IconName::Check))] primary_icon: Signal<
        IconName,
        LocalStorage,
    >,
    #[prop(into, default = Signal::stored_local(Vec::new()))] items: Signal<
        Vec<String>,
        LocalStorage,
    >,
    #[prop(into, default = Signal::stored_local(None))] overflow_text: Signal<
        Option<String>,
        LocalStorage,
    >,
    #[prop(into, optional)] confirm_label: Option<Signal<String, LocalStorage>>,
    #[prop(into, optional)] cancel_label: Option<Signal<String, LocalStorage>>,
    #[prop(into, default = Signal::stored_local(false))] destructive: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(0_u64))] generation: Signal<u64, LocalStorage>,
    #[prop(optional)] anchor: Option<DialogAnchor>,
    on_decision: UnsyncCallback<DialogDecision>,
) -> impl IntoView {
    install_dialog_stylesheet();

    let confirm_label = confirm_label
        .unwrap_or_else(|| default_label_signal(DialogCommand::Confirm.label_id(), "Confirm"));
    let cancel_label = cancel_label
        .unwrap_or_else(|| default_label_signal(DialogCommand::Cancel.label_id(), "Cancel"));

    let settled = Rc::new(Cell::new(false));
    {
        let settled = Rc::clone(&settled);
        let reset_fx = zgui::reactive::RenderEffect::new(move |prev: Option<(bool, u64)>| {
            let now = (open.get().unwrap_or(false), generation.get());
            if now.0 && prev != Some(now) {
                settled.set(false);
            }
            now
        });
        on_cleanup_local(move || drop(reset_fx));
    }
    let finish = {
        let settled = Rc::clone(&settled);
        UnsyncCallback::new(move |decision: DialogDecision| {
            if settled.replace(true) {
                return;
            }
            open.write(false);
            on_decision.run(decision);
            if let Some(anchor) = anchor {
                anchor.restore();
            }
        })
    };

    let on_open_change = {
        let settled = Rc::clone(&settled);
        UnsyncCallback::new(move |next_open: bool| {
            open.write(next_open);
            if next_open {
                settled.set(false);
            } else if !settled.replace(true) {
                on_decision.run(DialogDecision::Dismiss);
                if let Some(anchor) = anchor {
                    anchor.restore();
                }
            }
        })
    };

    view! {
        Dialog(open = open, on_open_change = on_open_change) {
            DialogContent(
                class = "gds-dialog-content",
                attr:data-size = move || Some(size.get().as_str().to_owned()),
                dismiss_on_outside_press = true,
                dismiss_on_escape = true,
                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                    handle_dialog_key_down(disabled.get_untracked(), finish, ev);
                }
            ) {
                {{
                    let _scope = DialogScope::enter();
                    let confirm_body = NodeRef::new();
                    let focus_fx = RenderEffect::new(move |mounted_before: Option<bool>| {
                        let mounted = confirm_body.get().is_some();
                        if mounted && mounted_before != Some(true) {
                            confirm_body.focus();
                        }
                        mounted
                    });
                    on_cleanup_local(move || drop(focus_fx));

                    view! {
                        column(
                            node_ref = confirm_body,
                            class = "gds-confirm-dialog__body",
                            tabindex = Focus::Programmatic,
                            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                if !ev.repeat
                                    && ev.target == ev.current
                                    && ev.modifiers.held().is_empty()
                                    && ev.key == Key::Named(NamedKey::Enter)
                                    && !disabled.get_untracked()
                                {
                                    finish.run(DialogDecision::Confirm);
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                }
                            },
                        ) {
                            DialogHeader {
                                {{move || {
                                    if let Some(header_icon) = icon.get() {
                                        AnyView::new(view! {
                                            row(class = "gds-confirm-dialog__title-row") {
                                                Icon(
                                                    name = header_icon,
                                                    size = 18.0,
                                                    class = "gds-confirm-dialog__title-icon",
                                                    attr:data-destructive = move || {
                                                        destructive.get().then(|| "true".to_owned())
                                                    },
                                                )
                                                DialogTitle {{move || title.get()}}
                                            }
                                        })
                                    } else {
                                        AnyView::new(view! {
                                            DialogTitle {{move || title.get()}}
                                        })
                                    }
                                }}}
                                {description.map(|desc| {
                                    view! {
                                        DialogDescription {{move || desc.get()}}
                                    }
                                    .into_view()
                                })}
                            }
                            {{move || {
                                let shown_items = items.get();
                                let overflow_line = overflow_text.get();
                                (!shown_items.is_empty()).then(|| {
                                    let rows = shown_items
                                        .into_iter()
                                        .map(|item| {
                                            view! {
                                                text(class = "gds-confirm-dialog__item") {{item}}
                                            }
                                            .into_view()
                                        })
                                        .collect::<Vec<_>>();
                                    let more = overflow_line
                                        .filter(|line| !line.is_empty())
                                        .map(|line| {
                                            view! {
                                                text(class = "gds-confirm-dialog__more") {{line}}
                                            }
                                        });
                                    view! {
                                        column(class = "gds-confirm-dialog__items") {
                                            {rows}
                                            {more}
                                        }
                                    }
                                })
                            }}}
                            DialogFooter {
                                {{move || {
                                    let p_icon = primary_icon.get();
                                    let is_destructive = destructive.get();
                                    view! {
                                        DialogActions(
                                            primary_label = confirm_label,
                                            primary_icon = p_icon,
                                            cancel_label = cancel_label,
                                            destructive = is_destructive,
                                            disabled = disabled,
                                            on_decision = finish,
                                        )
                                    }
                                }}}
                            }
                        }
                    }
                }}
            }
        }
    }
}

/// Convenience helper returning an [`AnyView`] confirmation dialog prompt controlled by `open`.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn confirm(
    open: impl Into<Binding<bool>>,
    title: impl Into<String>,
    description: Option<String>,
    confirm_label: impl Into<String>,
    cancel_label: Option<String>,
    destructive: bool,
    anchor: Option<DialogAnchor>,
    on_decision: UnsyncCallback<DialogDecision>,
) -> AnyView {
    let open = open.into();
    let title = title.into();
    let confirm_label = confirm_label.into();
    let description = description.map(Signal::stored_local);
    let cancel_signal = cancel_label.map(Signal::stored_local);
    AnyView::new(view! {
        ConfirmDialog(
            open = open,
            title = title,
            description = description,
            confirm_label = confirm_label,
            cancel_label = cancel_signal,
            destructive = destructive,
            anchor = anchor,
            on_decision = on_decision,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{Mounted, RwSignal, UnsyncCallback, install};
    use zgui::runtime::AppError;
    use zgui::view;
    use zgui::vocab::{KeyEvent, KeyState, Modifiers, Timestamp};
    use zgui_platform_headless::Harness;

    use super::{
        ConfirmDialogProps, DialogActionsProps, DialogCommand, DialogDecision, DialogScope,
        ModalDialogProps,
    };
    use crate::ui::icon::IconName;
    use crate::ui::keytips::{KeytipHostProps, has_active_modal_scope};

    #[test]
    fn dialog_commands_scope_and_actions_wire_up_cleanly() {
        assert_eq!(DialogCommand::Confirm.label_id(), "dialog-confirm");
        assert_eq!(DialogCommand::Cancel.label_id(), "dialog-cancel");
        assert!(DialogCommand::Confirm.matches_event(&KeyEvent::character("a"), Modifiers::ALT));
        assert!(DialogCommand::Cancel.matches_event(&KeyEvent::character("c"), Modifiers::ALT));
        assert!(DialogDecision::Confirm.is_confirmed());
        assert!(DialogDecision::Cancel.is_cancelled());
        assert!(DialogDecision::Dismiss.is_cancelled());

        install().ok();
        let scope = Mounted::new();
        scope.with(|| {
            assert!(!has_active_modal_scope());
            let inner = Mounted::new();
            inner.with(|| {
                let _dialog_scope = DialogScope::enter();
                assert!(has_active_modal_scope());
            });
            inner.unmount();
            assert!(!has_active_modal_scope());

            let open = RwSignal::new_local(true);
            let _dialog = view! {
                ConfirmDialog(
                    open = open,
                    title = "Delete?",
                    primary_icon = IconName::Trash,
                    destructive = true,
                    on_decision = UnsyncCallback::new(|_| {}),
                )
            };
            assert!(open.get_untracked());
        });
        scope.unmount();
    }

    #[test]
    fn headless_modal_dialog_dispatches_alt_a_and_alt_c_and_suppresses_workspace_keytips() {
        #[derive(Debug, Default)]
        struct HeadlessSink {
            atlas: zgui::atlas::MemorySink,
            target: Option<zgui::render::RenderTarget>,
        }

        impl zgui::render::Renderer for HeadlessSink {
            fn capabilities(&self) -> zgui::render::RenderCapabilities {
                zgui::render::RenderCapabilities::MINIMAL
            }
            fn configure(&mut self, target: zgui::render::RenderTarget) {
                self.target = Some(target);
            }
            fn target(&self) -> Option<zgui::render::RenderTarget> {
                self.target
            }
            fn register_external(
                &mut self,
                _texture: zgui::render::ExternalTexture,
            ) -> zgui::render::TextureHandle {
                zgui::render::TextureHandle(0)
            }
            fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}
            fn memory(&self) -> zgui::render::MemoryReport {
                zgui::render::MemoryReport::default()
            }
            fn draw(
                &mut self,
                _scene: &zgui::scene::Scene,
                _damage: &zgui::bits::DamageSet,
            ) -> zgui::render::FrameOutcome {
                zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
            }
            fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
                &mut self.atlas
            }
        }

        let decisions: Rc<Cell<Option<DialogDecision>>> = Rc::new(Cell::new(None));
        let open_slot: Rc<Cell<Option<RwSignal<bool, zgui::reactive::LocalStorage>>>> =
            Rc::new(Cell::new(None));

        let decisions_view = Rc::clone(&decisions);
        let open_view = Rc::clone(&open_slot);

        let decisions_check = Rc::clone(&decisions);
        let open_check = Rc::clone(&open_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.settle(4);

            let open_sig = open_check.get().expect("open signal");
            assert!(open_sig.get_untracked());
            assert!(
                has_active_modal_scope(),
                "open ModalDialog must activate DialogScope"
            );

            // Pressing Alt+A confirms the modal dialog.
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: KeyEvent::character("a"),
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert_eq!(decisions_check.get(), Some(DialogDecision::Confirm));
            assert!(!open_sig.get_untracked());
            assert!(
                !has_active_modal_scope(),
                "closed ModalDialog must release DialogScope"
            );

            // Re-open and press Alt+C to cancel.
            decisions_check.set(None);
            open_sig.set(true);
            harness.settle(4);
            assert!(has_active_modal_scope());
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: KeyEvent::character("c"),
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert_eq!(decisions_check.get(), Some(DialogDecision::Cancel));
            assert!(
                !has_active_modal_scope(),
                "cancelled ModalDialog must release DialogScope"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_title("gds-dialog-scope-test")
            .with_size(480.0, 320.0)
            .with_renderer(Box::new(|_surface: &Arc<dyn Surface>, target| {
                let mut renderer = HeadlessSink::default();
                zgui::render::Renderer::configure(&mut renderer, target);
                Ok::<_, AppError>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                let open = RwSignal::new_local(true);
                open_view.set(Some(open));
                let cb = Rc::clone(&decisions_view);
                let cb_actions = Rc::clone(&decisions_view);
                view! {
                    KeytipHost {
                        ModalDialog(
                            open = open,
                            on_decision = UnsyncCallback::new(move |d| cb.set(Some(d))),
                        ) {
                            {{
                                let cb_actions = Rc::clone(&cb_actions);
                                view! {
                                    DialogActions(
                                        primary_label = "Delete",
                                        primary_icon = IconName::Trash,
                                        destructive = true,
                                        on_decision = UnsyncCallback::new(move |d| cb_actions.set(Some(d))),
                                    )
                                }
                            }}
                        }
                    }
                }
            })
            .expect("headless modal dialog test ran");
    }
}
