use gds::{
    i18n::t,
    service::remote::{Address, Protocol},
    ui::field::input as input_field,
};
use gds_uim::el;
use gpui_kit::{
    component::{
        ActiveTheme,
        input::{InputEvent, InputState},
    },
    *,
};

const DIALOG_GAP: f32 = 8.0;

pub(super) struct Form {
    input: Entity<InputState>,
    protocol: Protocol,
    original: Option<String>,
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl Form {
    pub(super) fn new(
        value: String,
        protocol: Protocol,
        directory: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = if directory {
            t!(@cx, "new-folder-placeholder")
        } else {
            t!(@cx, "rename-placeholder")
        };
        let original = (!directory).then(|| value.clone());
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(value)
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        Self {
            input,
            protocol,
            original,
            error: None,
            _subscription: subscription,
        }
    }

    pub(super) fn value(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    pub(super) fn validate(&mut self, cx: &mut Context<Self>) -> bool {
        let name = self.value(cx);
        self.error = Self::validation_error(&name, self.protocol, self.original.as_deref(), cx);
        cx.notify();
        self.error.is_none()
    }

    pub(super) fn validation_error(
        name: &str,
        protocol: Protocol,
        original: Option<&str>,
        cx: &App,
    ) -> Option<SharedString> {
        if name.is_empty() {
            return Some(t!(@cx, "rename-error-empty"));
        }
        if matches!(name, "." | "..") {
            return Some(t!(@cx, "rename-error-reserved"));
        }
        if name.contains('/') || (protocol == Protocol::Smb && name.contains('\\')) {
            return Some(t!(@cx, "rename-error-separator"));
        }
        if name.contains('\0') || Address::validate_name(name, protocol).is_err() {
            return Some(t!(@cx, "rename-error-invalid"));
        }
        if original == Some(name) {
            return Some(t!(@cx, "rename-error-unchanged"));
        }
        None
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
        let _ = self.validate(cx);
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let error = self.error.clone();
        el! {
            .div { flex; flex_col; w_full; min_w_0; gap: DIALOG_GAP.px } [
                .div { w_full; min_w_0 } [($&input, cx).input_field { w_full }],
                @if (let Some(error) = error)
                    .div { text_xs; text_color: $(cx.theme().danger) } [error],
                @end,
            ]
        }
    }
}
