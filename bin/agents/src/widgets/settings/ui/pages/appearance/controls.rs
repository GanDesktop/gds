//! Application preference mapping for the suite's settings rows and sliders.

use super::*;
use crate::shared::i18n::tr;
use crate::shared::ui;
use gds_uim::el;
use gpui_kit::{Entity, assets::IconName, component::slider::SliderValue};

impl Field {
    pub(super) const NUMBERS: [Self; 5] = [
        Self::UiSize,
        Self::ContentSize,
        Self::LineHeight,
        Self::Opacity,
        Self::SidebarOpacity,
    ];
    fn scale(self) -> f32 {
        if matches!(self, Self::Opacity | Self::SidebarOpacity) {
            100.
        } else {
            1.
        }
    }
}

impl AppearancePage {
    pub(super) fn setting_number(&self, field: Field) -> &form::Numeric {
        match field {
            Field::UiSize => &self.form.ui_size,
            Field::ContentSize => &self.form.content_size,
            Field::LineHeight => &self.form.line_height,
            Field::Opacity => &self.form.opacity,
            Field::SidebarOpacity => &self.form.sidebar_opacity,
            _ => unreachable!("only numeric preferences own sliders"),
        }
    }

    fn setting_number_mut(&mut self, field: Field) -> &mut form::Numeric {
        match field {
            Field::UiSize => &mut self.form.ui_size,
            Field::ContentSize => &mut self.form.content_size,
            Field::LineHeight => &mut self.form.line_height,
            Field::Opacity => &mut self.form.opacity,
            Field::SidebarOpacity => &mut self.form.sidebar_opacity,
            _ => unreachable!("only numeric preferences own sliders"),
        }
    }

    fn effective_number(field: Field, cx: &App) -> f32 {
        match field {
            Field::UiSize => f32::from(cx.theme().font_size),
            Field::ContentSize => f32::from(theme::content_font_size(cx)),
            Field::LineHeight => theme::content_line_height(cx),
            Field::Opacity => cx.global::<theme::Appearance>().content_opacity,
            Field::SidebarOpacity => cx.global::<theme::Appearance>().sidebar_opacity,
            _ => unreachable!("only numeric preferences own sliders"),
        }
    }

    pub(super) fn sync_numbers(&self, window: &mut Window, cx: &mut Context<Self>) {
        for field in Field::NUMBERS {
            let number = self.setting_number(field);
            let value = number
                .value
                .unwrap_or_else(|| Self::effective_number(field, cx));
            number.control.sync(value * field.scale(), window, cx);
        }
    }

    pub(super) fn apply_number(
        &mut self,
        field: Field,
        value: SliderValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document.is_none() {
            return;
        }
        let SliderValue::Single(value) = value else {
            return;
        };
        let number = self.setting_number(field);
        let value = number.control.range.quantize(value);
        let old = number
            .value
            .unwrap_or_else(|| Self::effective_number(field, cx))
            * field.scale();
        if number.control.range.equivalent(old, value) {
            return;
        }
        self.setting_number_mut(field).value = Some(value / field.scale());
        self.flush_pending(window, cx);
    }

    fn number_slider(&self, field: Field, cx: &App) -> gpui_kit::AnyElement {
        let number = self.setting_number(field);
        let value = number
            .value
            .unwrap_or_else(|| Self::effective_number(field, cx));
        let label = match field {
            Field::Opacity | Field::SidebarOpacity => format!("{:.0}%", value * field.scale()),
            Field::LineHeight => format!("{value:.1}"),
            _ => format!("{value:.0} px"),
        };
        number
            .control
            .element(
                field.id().into(),
                field.label().into(),
                label.into(),
                self.document.is_none(),
            )
            .into_any_element()
    }

    fn overridden(&self, field: Field, cx: &App) -> bool {
        let named = |select: &Entity<SelectState<SearchableVec<form::FontChoice>>>| {
            matches!(
                select.read(cx).selected_value(),
                Some(form::FontChoice::Named(_))
            )
        };
        let defaults = Preferences::default();
        match field {
            Field::UiFont => named(&self.form.ui_font),
            Field::MonoFont => named(&self.form.mono_font),
            Field::UiSize
            | Field::ContentSize
            | Field::LineHeight
            | Field::Opacity
            | Field::SidebarOpacity => self.setting_number(field).value.is_some(),
            Field::Fallbacks => !self.form.fallbacks.read(cx).value().is_empty(),
            Field::Source => self
                .form
                .source
                .read(cx)
                .selected_value()
                .is_some_and(|value| *value != defaults.theme_source),
            Field::Builtin => self
                .form
                .builtin
                .read(cx)
                .selected_value()
                .is_some_and(|value| *value != defaults.builtin_theme),
            Field::Mode => self
                .form
                .mode
                .read(cx)
                .selected_value()
                .is_some_and(|value| *value != defaults.theme_mode),
            Field::Language => self
                .form
                .language
                .read(cx)
                .selected_value()
                .is_some_and(|value| *value != defaults.language),
            Field::Background => self
                .form
                .background
                .read(cx)
                .selected_value()
                .is_some_and(|value| *value != defaults.background),
            Field::ComposerEnter => self
                .form
                .composer_enter
                .read(cx)
                .selected_value()
                .is_some_and(|value| *value != defaults.composer_enter_behavior),
        }
    }

    fn reset_field(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_none() {
            return;
        }
        let defaults = Preferences::default();
        match field {
            Field::UiFont => self.form.ui_font.update(cx, |select, cx| {
                select.set_selected_value(&form::FontChoice::Default, window, cx)
            }),
            Field::MonoFont => self.form.mono_font.update(cx, |select, cx| {
                select.set_selected_value(&form::FontChoice::Default, window, cx)
            }),
            Field::UiSize
            | Field::ContentSize
            | Field::LineHeight
            | Field::Opacity
            | Field::SidebarOpacity => self.setting_number_mut(field).value = None,
            Field::Fallbacks => self
                .form
                .fallbacks
                .update(cx, |input, cx| input.set_value("", window, cx)),
            Field::Source => self.form.source.update(cx, |select, cx| {
                select.set_selected_value(&defaults.theme_source, window, cx)
            }),
            Field::Builtin => self.form.builtin.update(cx, |select, cx| {
                select.set_selected_value(&defaults.builtin_theme, window, cx)
            }),
            Field::Mode => self.form.mode.update(cx, |select, cx| {
                select.set_selected_value(&defaults.theme_mode, window, cx)
            }),
            Field::Language => self.form.language.update(cx, |select, cx| {
                select.set_selected_value(&defaults.language, window, cx)
            }),
            Field::Background => self.form.background.update(cx, |select, cx| {
                select.set_selected_value(&defaults.background, window, cx)
            }),
            Field::ComposerEnter => self.form.composer_enter.update(cx, |select, cx| {
                select.set_selected_value(&defaults.composer_enter_behavior, window, cx)
            }),
        }
        self.flush_pending(window, cx);
    }

    pub(super) fn row(&self, field: Field, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let controls = match field {
            Field::UiSize
            | Field::ContentSize
            | Field::LineHeight
            | Field::Opacity
            | Field::SidebarOpacity => self.number_slider(field, cx),
            _ => el! { .div { flex_1; min_w_0; flex; justify_end } [$(self.control(field, cx))] }
                .into_any_element(),
        };
        let reset = self.overridden(field, cx).then(|| {
            ui::icon_action(
                gpui_kit::SharedString::from(format!("{}-default", field.id())),
                IconName::RotateCcw,
                tr!("appearance-reset-field", field = field.label()),
            )
            .disabled(self.document.is_none())
            .on_click(cx.listener(move |this, _, window, cx| this.reset_field(field, window, cx)))
            .into_any_element()
        });
        crate::widgets::settings::ui::form::Row {
            id: field.id().into(),
            label: field.label(),
            help: field.help(),
            control: controls,
            reset,
        }
        .into_any_element()
    }
}
