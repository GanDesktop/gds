//! Dialog that asks for a free target name when a paste would collide.

use std::{cell::RefCell, ffi::OsString, rc::Rc};

use fluent_bundle::FluentArgs;
use gds::{i18n, ui::DialogActions};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, WindowExt,
    dialog::AlertDialog,
    input::{InputEvent, InputState},
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{ConflictPrompt, Paste};
use crate::features::file_operations::names;
use crate::pages::workspace::ui::Browser;

const CONFLICT_WIDTH: f32 = 520.;
const DIALOG_GAP: f32 = 8.;

/// Input and validation state of one rename request.
///
/// Typing only checks the name itself, so no keystroke touches the filesystem.
/// The typed name is checked against the destination in the background once the
/// user confirms, and a remaining conflict is prompted again.
///
/// While the field still shows the suggestion's display text, the
/// filesystem-checked suggestion is used as a lossless [`OsString`]; a
/// suggestion derived from a non-UTF-8 leaf therefore keeps its exact bytes.
struct Form {
    input: Entity<InputState>,
    suggestion: OsString,
    display: String,
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl Form {
    fn new(suggestion: OsString, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let display = suggestion.to_string_lossy().into_owned();
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(i18n::text("transfer-conflict-placeholder", cx))
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        let mut form = Self {
            input,
            suggestion,
            display,
            error: None,
            _subscription: subscription,
        };
        let value = form.display.clone();
        form.input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.select_all(window, cx);
        });
        form.refresh_error(cx);
        form
    }

    fn is_valid(&self) -> bool {
        self.error.is_none()
    }

    fn chosen_name(&self, cx: &App) -> OsString {
        let value = self.input.read(cx).value().to_string();
        if value == self.display {
            self.suggestion.clone()
        } else {
            OsString::from(value)
        }
    }

    fn focus_input(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    fn refresh_error(&mut self, cx: &App) -> bool {
        match names::validate(&self.chosen_name(cx)) {
            Ok(()) => {
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(i18n::text(error.label_id(), cx));
                false
            }
        }
    }

    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.refresh_error(cx);
        cx.notify();
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let error = self.error.clone();
        el! {
            .div { flex; flex_col; w_full; min_w_0; gap: DIALOG_GAP.px } [
                .div { w_full; min_w_0 } [($&input, cx).gds::ui::field::input { w_full }],
                @if (let Some(error) = error)
                    .div { text_xs; text_color: $cx.theme().danger } [error],
                @end,
            ]
        }
    }
}

/// One rename request of a paste flow. The flow state lives here while the
/// dialog is open, so closing this dialog only ever affects its own paste.
pub(super) struct ConflictDialog {
    browser: WeakEntity<Browser>,
    origin_tab: u64,
    pending: RefCell<Option<Paste>>,
    conflict: names::Collision,
    form: Entity<Form>,
}

impl ConflictDialog {
    pub(super) fn new(
        browser: WeakEntity<Browser>,
        paste: Paste,
        prompt: ConflictPrompt,
        window: &mut Window,
        cx: &mut Context<Browser>,
    ) -> Rc<Self> {
        let origin_tab = paste.origin_tab;
        let form = cx.new(|cx| Form::new(prompt.suggestion, window, cx));
        Rc::new(Self {
            browser,
            origin_tab,
            pending: RefCell::new(Some(paste)),
            conflict: prompt.collision,
            form,
        })
    }

    pub(super) fn open(this: Rc<Self>, window: &mut Window) {
        // Wait for the triggering menu to leave the focus tree; Kit then records
        // the browser, not the disappearing menu, as the dialog's return target.
        window.on_next_frame(move |window, cx| {
            let Some(browser) = this.browser.upgrade() else {
                return;
            };
            let focus = browser.read(cx).focus.clone();
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
            let footer = DialogActions::default()
                .primary_label(i18n::text("paste-files", cx))
                .primary_icon(IconName::ClipboardPaste)
                .cancel_label(i18n::text("operation-cancel", cx));
            let mut arguments = FluentArgs::new();
            arguments.set("name", this.conflict.target_label());
            el! {
                dialog {
                    confirm; width: CONFLICT_WIDTH.px;
                    bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                    title: $(i18n::format("transfer-conflict-title", Some(&arguments), cx));
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
            paste.record_name(this.conflict.source.clone(), name);
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
