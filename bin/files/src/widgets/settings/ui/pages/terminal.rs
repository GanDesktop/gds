use crate::widgets::settings::ui::View;
use gds::{i18n::text, ui::settings::*};
use gds_uim::el;
use gpui_kit::{component::switch::Switch, *};

impl View {
    pub(super) fn render_terminal(&self, cx: &mut Context<Self>) -> Div {
        let label = text("files-terminal-enabled", cx);
        let on_change = cx.listener(|view, enabled: &bool, window, cx| {
            view.change(window, cx, |preferences| {
                preferences.terminal_enabled = Some(*enabled);
                true
            });
        });
        el! { .div { flex; flex_col; gap_4; } [
            .SettingRow::default [
                .SettingLabel::default [$label.clone()],
                .SettingActions::default [
                    .SettingControl::default [
                        "settings-terminal-enabled".Switch::new { checked: $self.preferences.terminal_enabled(); accessibility_label: label; on_change: on_change; },
                    ],
                ],
            ],
            @if (self.preferences.terminal_enabled()) $(self.terminal.clone()), @end,
        ] }
    }
}
