use gds_uim::el;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use crate::widgets::settings::ui::{
    View,
    controls::{Field, PERCENTAGE_RANGE, SettingNote},
};
use gds::i18n::text;
use gds::ui::settings::SettingRow;

impl View {
    pub(super) fn render_appearance(&mut self, cx: &mut Context<Self>) -> Div {
        let source = self.source.render(
            Field::Source,
            text("theme-custom", cx),
            text("language-search", cx),
            cx,
        );
        let builtin_row =
            if self.preferences.appearance.source == gds::appearance::theme::Source::Builtin {
                let builtin = self.builtin.render(
                    Field::Builtin,
                    text("theme-builtin-selection", cx),
                    text("language-search", cx),
                    cx,
                );
                Some(self.setting_row(Field::Builtin, builtin, cx))
            } else {
                None
            };
        let mode = self.mode.render(
            Field::Mode,
            text("theme-mode", cx),
            text("language-search", cx),
            cx,
        );
        let source_row = self.setting_row(Field::Source, source, cx);
        let mode_row = self.setting_row(Field::Mode, mode, cx);
        let content_opacity = self.percentage_row(Field::ContentOpacity, cx);
        let sidebar_opacity = self.percentage_row(Field::SidebarOpacity, cx);
        let sample_text = text("font-sample", cx);
        let sample = el! {
            .div {
                py_4; text_lg; truncate;
                text_color: $(cx.theme().muted_foreground);
                border_b_1; border_color: $(cx.theme().border);
            } [sample_text]
        };
        let font_note = SettingNote::new(text("font-fallback-note", cx));
        let blur_note = SettingNote::new(text("blur-status", cx));
        el! {
            .div { flex; flex_col } [
                source_row,
                ..builtin_row,
                mode_row,
                content_opacity,
                sidebar_opacity,
                $(self.font.clone()),
                sample,
                .div { pt_4 } [font_note],
                .div { pt_2 } [blur_note],
            ]
        }
    }

    fn percentage_row(&self, field: Field, cx: &mut Context<Self>) -> SettingRow {
        let value = field
            .number(&self.preferences)
            .expect("opacity rows use numeric fields");
        let value_label = format!("{:.0}%", PERCENTAGE_RANGE.quantize(value)).into();
        let control = self.number_control(field).render(field, value_label, cx);
        self.setting_row(field, control, cx)
    }
}
