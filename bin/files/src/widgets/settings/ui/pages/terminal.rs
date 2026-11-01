use gds_uim::el;
use gpui_kit::*;

use crate::widgets::settings::ui::{View, controls::SettingNote};
use gds::i18n::text;

impl View {
    pub(super) fn render_terminal(&self, cx: &App) -> Div {
        let shell_note = SettingNote::new(text("files-terminal-settings-note", cx));
        el! { .div { flex; flex_col; gap_4 } [$(self.terminal.clone()), shell_note] }
    }
}
