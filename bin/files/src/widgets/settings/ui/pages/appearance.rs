use gds_uim::el;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use crate::widgets::settings::ui::{
    View,
    controls::{Field, PERCENTAGE_RANGE},
};
use gds::i18n::t;
use gds::ui::settings::{SettingDescription, SettingRow};

impl View {
    pub(super) fn render_appearance(&mut self, cx: &mut Context<Self>) -> Div {
        let corners_row = self.toggle_row(Field::RoundedCorners, false, cx);
        let animations_row = self.toggle_row(Field::Animations, false, cx);
        let animations_note =
            el! { .SettingDescription::default [$(t!(@cx, "appearance-animations-help"))] };
        let source = self.source.render(
            Field::Source,
            t!(@cx, "theme-custom"),
            t!(@cx, "theme-search"),
            cx,
        );
        let builtin_row = if self.source.state.read(cx).selected_value()
            == Some(&gds::appearance::theme::SourceKind::Builtin)
        {
            let builtin = self.builtin.render(
                Field::Builtin,
                t!(@cx, "theme-builtin-selection"),
                t!(@cx, "theme-search"),
                cx,
            );
            Some(self.setting_row(Field::Builtin, builtin, cx))
        } else {
            None
        };
        let custom_row = if self.source.state.read(cx).selected_value()
            == Some(&gds::appearance::theme::SourceKind::Custom)
        {
            let custom = self.custom.render(
                Field::Custom,
                t!(@cx, "theme-custom-selection"),
                t!(@cx, "theme-search"),
                cx,
            );
            Some(self.setting_row(Field::Custom, custom, cx))
        } else {
            None
        };
        let mode = self.mode.render(
            Field::Mode,
            t!(@cx, "theme-mode"),
            t!(@cx, "theme-search"),
            cx,
        );
        let source_row = self.setting_row(Field::Source, source, cx);
        let mode_row = self.setting_row(Field::Mode, mode, cx);
        let content_opacity = self.percentage_row(Field::ContentOpacity, cx);
        let sidebar_opacity = self.percentage_row(Field::SidebarOpacity, cx);
        let sample_text = t!(@cx, "font-sample");
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
                ..custom_row,
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
