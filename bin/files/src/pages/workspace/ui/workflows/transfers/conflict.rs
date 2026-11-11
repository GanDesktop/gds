//! Dialog that asks for a free target name when a paste would collide.

use std::{cell::RefCell, rc::Rc};

use gds::{i18n, ui::DialogActions};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, WindowExt, dialog::AlertDialog};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Paste;
use crate::{
    features::file_operations::{names, paste::PasteConflict},
    pages::workspace::ui::{Browser, name_form::NameForm},
    shared::i18n::t,
};

const CONFLICT_WIDTH: f32 = 520.;

/// One rename request of a paste flow. The flow state lives here while the
/// dialog is open, so closing this dialog only ever affects its own paste.
pub(in crate::pages::workspace) struct ConflictDialog {
    browser: WeakEntity<Browser>,
    origin_tab: u64,
    pending: RefCell<Option<Paste>>,
    conflict: names::Collision,
    form: Entity<NameForm>,
}

impl ConflictDialog {
    pub(in crate::pages::workspace) fn new(
        browser: WeakEntity<Browser>,
        paste: Paste,
        prompt: PasteConflict,
        window: &mut Window,
        cx: &mut Context<Browser>,
    ) -> Rc<Self> {
        let origin_tab = paste.origin_tab;
        let form = cx.new(|cx| {
            NameForm::new(
                prompt.suggestion,
                i18n::text("transfer-conflict-placeholder", cx),
                window,
                cx,
            )
        });
        Rc::new(Self {
            browser,
            origin_tab,
            pending: RefCell::new(Some(paste)),
            conflict: prompt.collision,
            form,
        })
    }

    pub(in crate::pages::workspace) fn open(this: Rc<Self>, window: &mut Window) {
        // Wait for the triggering menu to leave the focus tree; Kit then records
        // the browser, not the disappearing menu, as the dialog's return target.
        window.on_next_frame(move |window, cx| {
            let Some(browser) = this.browser.upgrade() else {
                return;
            };
            let focus = browser.read(cx).window.focus.clone();
            focus.focus(window, cx);
            let form = this.form.clone();
            window.open_alert_dialog(cx, Self::builder(this.clone()));
            window.on_next_frame(move |window, cx| {
                form.update(cx, |form, cx| form.focus_input(window, cx));
            });
        });
    }

    fn builder(this: Rc<Self>) -> impl Fn(AlertDialog, &mut Window, &mut App) -> AlertDialog {
        move |dialog, _, cx| {
            let on_ok = Self::confirm(this.clone());
            let on_close = Self::close(this.clone());
            let form = this.form.clone();
            let footer = el! { .DialogActions::default {
                primary_label: $(i18n::text("paste-files", cx));
                primary_icon: IconName::ClipboardPaste;
                cancel_label: $(i18n::text("operation-cancel", cx));
            } };
            let name = this.conflict.target_label();
            el! {
                dialog {
                    confirm; width: CONFLICT_WIDTH.px;
                    bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                    title: $(t!(@cx, "transfer-conflict-title", name));
                    description: $(i18n::text("transfer-conflict-message", cx));
                    icon: IconName::ClipboardPaste.Icon::new;
                    footer: footer;
                    on_ok: on_ok;
                    on_close: on_close;
                } [~form]
            }
        }
    }

    fn confirm(this: Rc<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, window, cx| {
            let Some(paste) = this.pending.borrow_mut().take() else {
                return true;
            };
            let valid = this.form.update(cx, |form, cx| form.refresh_error(cx));
            if !valid {
                this.pending.replace(Some(paste));
                return false;
            }
            let name = this.form.update(cx, |form, cx| {
                debug_assert!(form.is_valid());
                form.chosen_name(cx)
            });
            let Some(browser) = this.browser.upgrade() else {
                return true;
            };
            let mut paste = paste;
            paste
                .job
                .resolve_conflict(this.conflict.source.clone(), name);
            // Rechecking the destination happens off the UI thread; a target
            // that is still taken comes back as another prompt.
            browser.update(cx, |browser, cx| browser.inspect_paste(paste, window, cx));
            true
        }
    }

    fn close(this: Rc<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            // Confirmed work already took the flow out; only an actual close
            // cancels it.
            if let Some(paste) = this.pending.borrow_mut().take()
                && let Some(browser) = this.browser.upgrade()
            {
                browser.update(cx, |browser, cx| {
                    browser.cancel_paste(paste, window, cx);
                });
            }
            Browser::restore_file_focus(this.browser.clone(), this.origin_tab, window);
        }
    }
}
