//! Editable leaf-name field shared by the dialogs that create or rename entries.
//!
//! Typing only checks the name itself, so no keystroke touches the filesystem.
//! While the field still shows the suggestion's display text, the
//! filesystem-checked suggestion is used as a lossless [`OsString`]; a
//! suggestion derived from a non-UTF-8 leaf therefore keeps its exact bytes.

use std::ffi::OsString;

use gds::i18n;
use gds_uim::el;
use gpui_kit::{
    component::{
        ActiveTheme,
        input::{InputEvent, InputState},
    },
    prelude::*,
    *,
};

use crate::features::file_operations::names;

pub(super) const DIALOG_GAP: f32 = 8.;

pub(super) struct NameForm {
    input: Entity<InputState>,
    suggestion: OsString,
    display: String,
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl NameForm {
    pub(super) fn new(
        suggestion: OsString,
        placeholder: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let display = suggestion.to_string_lossy().into_owned();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
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

    /// The typed text, unless the user left the checked suggestion untouched.
    pub(super) fn chosen_name(&self, cx: &App) -> OsString {
        let value = self.input.read(cx).value().to_string();
        if value == self.display {
            self.suggestion.clone()
        } else {
            OsString::from(value)
        }
    }

    pub(super) fn focus_input(&self, window: &mut Window, cx: &mut App) {
        focus(&self.input, window, cx);
    }

    /// Check the typed leaf itself; the destination is only probed in the
    /// background after the user confirms.
    pub(super) fn refresh_error(&mut self, cx: &App) -> bool {
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

    pub(super) fn is_valid(&self) -> bool {
        self.error.is_none()
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

impl Render for NameForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        field(&self.input, self.error.as_ref(), cx)
    }
}

pub(super) fn focus(input: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

/// One name field with its validation message underneath.
pub(super) fn field(
    input: &Entity<InputState>,
    error: Option<&SharedString>,
    cx: &App,
) -> impl IntoElement {
    let input = input.clone();
    let error = error.cloned();
    el! {
        .div { flex; flex_col; w_full; min_w_0; gap: DIALOG_GAP.px } [
            .div { w_full; min_w_0 } [($&input, cx).gds::ui::field::input { w_full }],
            @if (let Some(error) = error)
                .div { text_xs; text_color: $cx.theme().danger } [error],
            @end,
        ]
    }
}
