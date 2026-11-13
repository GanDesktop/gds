use crate::widgets::settings::ui::{View, controls::Field};
use gds_uim::el;
use gpui_kit::*;

impl View {
    pub(super) fn render_terminal(&self, cx: &mut Context<Self>) -> Div {
        let enabled_row = self.toggle_row(Field::TerminalEnabled, false, cx);
        el! { .div { flex; flex_col; gap_4; } [
            enabled_row,
            @if (self.preferences.terminal_enabled()) $(self.terminal.clone()), @end,
        ] }
    }
}
