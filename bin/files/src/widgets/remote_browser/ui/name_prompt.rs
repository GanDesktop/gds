use gpui_kit::{component::input::InputState, *};

pub(super) struct Form {
    input: Entity<InputState>,
}

impl Form {
    pub(super) fn new(value: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            input: cx.new(|cx| InputState::new(window, cx).default_value(value)),
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
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        gds::ui::field::input(&self.input, cx)
    }
}
