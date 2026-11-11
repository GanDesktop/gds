//! Editable leaf-name field shared by the dialogs that create or rename entries.
//!
//! Typing only checks the name itself, so no keystroke touches the filesystem.
//! While the field still shows the suggestion's display text, the
//! filesystem-checked suggestion is used as a lossless [`OsString`]; a
//! suggestion derived from a non-UTF-8 leaf therefore keeps its exact bytes.

use std::ffi::OsString;

use gds::i18n;
use gds::ui::field::input as input_field;
use gds_uim::{component, el};
use gpui_kit::IntoElement;
use gpui_kit::{
    component::{
        ActiveTheme,
        input::{InputEvent, InputState},
    },
    prelude::*,
    *,
};

use crate::features::file_operations::{names, rename};

pub(super) const DIALOG_GAP: f32 = 8.;

enum Policy {
    Create,
    Rename { original: OsString },
}

pub(super) struct NameForm {
    input: Entity<InputState>,
    suggestion: OsString,
    display: String,
    policy: Policy,
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
        let mut form = Self::with_policy(suggestion, placeholder, Policy::Create, window, cx);
        form.refresh_error(cx);
        form
    }

    pub(super) fn for_rename(
        original: OsString,
        placeholder: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let policy = Policy::Rename {
            original: original.clone(),
        };
        Self::with_policy(original, placeholder, policy, window, cx)
    }

    fn with_policy(
        suggestion: OsString,
        placeholder: SharedString,
        policy: Policy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let display = suggestion.to_string_lossy().into_owned();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        let form = Self {
            input,
            suggestion,
            display,
            policy,
            error: None,
            _subscription: subscription,
        };
        let value = form.display.clone();
        form.input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.select_all(window, cx);
        });
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
        let chosen = self.chosen_name(cx);
        let label_id = match &self.policy {
            Policy::Create => names::validate(&chosen)
                .err()
                .map(names::NameError::label_id),
            Policy::Rename { original } => rename::validate_name(&chosen, original)
                .err()
                .map(rename::NameError::label_id),
        };
        match label_id {
            None => {
                self.error = None;
                true
            }
            Some(label_id) => {
                self.error = Some(i18n::text(label_id, cx));
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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        el! { .NameField::default { input: $(self.input.clone()); error: $(self.error.clone()); } }
    }
}

pub(super) fn focus(input: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

/// One name field with its validation message underneath.
#[component]
#[derive(Default, IntoElement)]
pub(super) struct NameField {
    #[attr(option)]
    input: Option<Entity<InputState>>,
    #[attr]
    error: Option<SharedString>,
}

impl RenderOnce for NameField {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! {
            .div { flex; flex_col; w_full; min_w_0; gap: DIALOG_GAP.px } [
                @if (let Some(input) = self.input)
                    .div { w_full; min_w_0 } [($&input, cx).input_field { w_full }],
                @end,
                @if (let Some(error) = self.error)
                    .div { text_xs; text_color: $cx.theme().danger } [error],
                @end,
            ]
        }
    }
}
