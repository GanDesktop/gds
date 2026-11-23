//! Suite settings presentation: flat hairline rows, numeric ranges, and keyboard-accessible sliders.

pub mod form;
mod row;

pub use row::*;

use crate::appearance::typography;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback};
use zgui::{component, css, view};
use zgui_ui::SliderProps;
use zgui_ui_primitives::Binding;

pub const CONTROL_WIDTH: f32 = 260.0;
pub(crate) const RESET_WIDTH: f32 = 32.0;
pub(crate) const CONTROL_GAP: f32 = 8.0;
pub const NUMBER_LABEL_WIDTH_REMS: f32 = 4.5;

const SETTINGS_SHEET: &str = "gds-ui-settings";

const SETTINGS_CSS: &str = css!(
    ".gds-setting-form.gds-setting-form {
        display: flex;
        flex-direction: column;
        gap: 0px;
        width: 100%;
    }
    .gds-setting-form.gds-setting-form[data-disabled=\"true\"] {
        opacity: 0.6;
    }
    .gds-setting-section.gds-setting-section {
        display: flex;
        flex-direction: column;
        gap: 0px;
        width: 100%;
    }
    .gds-setting-group.gds-setting-group {
        font-size: var(--gds-text-sm, 14px);
        font-weight: 400;
        color: var(--gds-muted-foreground);
        padding-top: 16px;
    }
    .gds-setting-row.gds-setting-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 24px;
        padding: 16px 0px;
        background: transparent;
        border: none;
        border-bottom: 1px solid var(--gds-border);
        border-radius: 0px;
        width: 100%;
    }
    [data-width-tier=\"micro\"] .gds-setting-row.gds-setting-row,
    .gds-setting-row.gds-setting-row[data-micro=\"true\"] {
        flex-direction: column;
        align-items: stretch;
        gap: 10px;
    }
    .gds-setting-label.gds-setting-label {
        display: flex;
        flex-direction: column;
        gap: 4px;
        flex: 1 1 0%;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
    }
    .gds-setting-title.gds-setting-title {
        font-size: var(--gds-text-sm, 14px);
        font-weight: 400;
        color: var(--gds-foreground);
    }
    .gds-setting-description.gds-setting-description {
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-muted-foreground);
    }
    .gds-setting-error.gds-setting-error {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-error);
    }
    .gds-setting-actions.gds-setting-actions {
        display: flex;
        flex-direction: row;
        flex-shrink: 0;
        align-items: center;
        gap: var(--gds-setting-gap, 8px);
    }
    [data-width-tier=\"micro\"] .gds-setting-actions.gds-setting-actions,
    .gds-setting-row[data-micro=\"true\"] .gds-setting-actions.gds-setting-actions {
        align-self: flex-end;
        max-width: 100%;
    }
    .gds-setting-reset.gds-setting-reset {
        width: var(--gds-setting-reset-size, 32px);
        height: var(--gds-setting-reset-size, 32px);
        flex-shrink: 0;
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
    .gds-setting-control.gds-setting-control {
        width: var(--gds-setting-control-width, 260px);
        flex-shrink: 0;
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: flex-end;
    }
    [data-width-tier=\"micro\"] .gds-setting-control.gds-setting-control,
    .gds-setting-row[data-micro=\"true\"] .gds-setting-control.gds-setting-control {
        max-width: 100%;
        min-width: 0px;
    }
    .gds-setting-control.gds-setting-control > .zui-select,
    .gds-setting-control.gds-setting-control .zui-select {
        width: 100%;
        min-width: 0px;
        height: var(--gds-control-height, 36px);
        min-height: var(--gds-control-height, 36px);
        background: var(--gds-content);
        color: var(--gds-foreground);
        border: 1px solid var(--gds-outline);
        border-radius: var(--gds-radius-control, 6px);
        box-shadow: none;
        outline: none;
        box-sizing: border-box;
    }
    .gds-setting-control.gds-setting-control .zui-select:focus,
    .gds-setting-control.gds-setting-control .zui-select:focus-visible {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }
    .gds-setting-control.gds-setting-control > .zui-select__trigger,
    .gds-setting-control.gds-setting-control .zui-select__trigger,
    .gds-setting-control.gds-setting-control > .zui-combobox,
    .gds-setting-control.gds-setting-control .zui-input-group,
    .gds-setting-control.gds-setting-control > .gds-input-field,
    .gds-setting-control.gds-setting-control > .gds-textarea-field,
    .gds-setting-control.gds-setting-control > .gds-setting-number-row,
    .gds-setting-control.gds-setting-control > .gds-setting-number {
        width: 100%;
    }
    .gds-setting-control .zui-select__trigger {
        background: var(--gds-content);
        border: 1px solid var(--gds-outline);
        border-radius: var(--gds-radius-control, 6px);
        box-shadow: none;
        outline: none;
    }
    .gds-setting-control .zui-select__trigger:focus,
    .gds-setting-control .zui-select__trigger:focus-visible {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }
    .gds-setting-number.gds-setting-number {
        flex: 1 1 0%;
        min-width: 0px;
        width: 100%;
    }
    .gds-setting-number-row.gds-setting-number-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        width: 100%;
    }
    .gds-setting-number-value.gds-setting-number-value {
        width: 4.5rem;
        min-width: 4.5rem;
        flex-shrink: 0;
        text-align: right;
        white-space: nowrap;
        font-size: var(--gds-text-sm, 14px);
        font-variant-numeric: tabular-nums;
        color: var(--gds-foreground);
    }"
);

pub(crate) fn install_settings_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(SETTINGS_SHEET, SETTINGS_CSS);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberRange {
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

impl NumberRange {
    #[must_use]
    pub fn quantize(self, value: f32) -> f32 {
        if !value.is_finite() {
            return self.max;
        }
        let value = value.clamp(self.min, self.max);
        (((value - self.min) / self.step).round() * self.step + self.min).clamp(self.min, self.max)
    }

    #[must_use]
    pub fn equivalent(self, left: f32, right: f32) -> bool {
        left.is_finite() && right.is_finite() && self.quantize(left) == self.quantize(right)
    }
}

pub const PERCENTAGE_RANGE: NumberRange = NumberRange {
    min: 0.0,
    max: 100.0,
    step: 1.0,
};

pub const FONT_SIZE_RANGE: NumberRange = NumberRange {
    min: typography::MIN_SIZE,
    max: typography::MAX_SIZE,
    step: 1.0,
};

/// Keyboard-accessible quantized numeric slider paired with a live readout.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn NumberControl(
    #[prop(into, optional)] value: Binding<f64>,
    range: NumberRange,
    #[prop(optional)] on_change: Option<UnsyncCallback<f64>>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, optional)] label: Option<String>,
    #[prop(optional)] labelled_by: Option<NodeRef>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_settings_stylesheets();
    view! {
        box(class = "gds-setting-number", class = class, {..attrs}) {
            Slider(
                value = value,
                min = f64::from(range.min),
                max = f64::from(range.max),
                step = f64::from(range.step),
                disabled = disabled,
                label = label,
                labelled_by = labelled_by,
                node_ref = node_ref,
                on_change = on_change
            )
        }
    }
}
