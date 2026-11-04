use gds_uim::el;
use gpui_kit::component::{ActiveTheme, switch::Switch};
use gpui_kit::*;

use crate::widgets::settings::ui::{
    View,
    controls::{Field, PERCENTAGE_RANGE, SettingNote},
};
use gds::i18n::text;
use gds::ui::settings::SettingRow;

impl View {
    pub(super) fn render_appearance(&mut self, cx: &mut Context<Self>) -> Div {
        let corners = el! {
            "settings-rounded-corners".Switch::new {
                accessibility_label: $(text("appearance-rounded-corners", cx));
                checked: $(self.preferences.appearance.rounded_corners);
                on_change: $(cx.listener(Self::on_rounded_corners_changed));
            }
        };
        let corners_row = self.setting_row(Field::RoundedCorners, corners, cx);
        let animations = el! {
            "settings-animations".Switch::new {
                accessibility_label: $(text("appearance-animations", cx));
                checked: $(self.preferences.animations_enabled.unwrap_or(true));
                on_change: $(cx.listener(Self::on_animations_changed));
            }
        };
        let animations_row = self.setting_row(Field::Animations, animations, cx);
        let animations_note = SettingNote::new(text("appearance-animations-help", cx));
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
        el! {
            .div { flex; flex_col } [
                source_row,
                ..builtin_row,
                mode_row,
                corners_row,
                animations_row,
                animations_note,
                content_opacity,
                sidebar_opacity,
                $(self.font.clone()),
                sample,
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
