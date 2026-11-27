use super::{
    NumberControlProps, NumberRange, SettingActionsProps, SettingControlProps,
    SettingDescriptionProps, SettingLabelProps, SettingResetProps, SettingRowProps,
};
use crate::{
    i18n,
    ui::{
        IconButtonProps, IconName, ToggleSwitchProps, field::FieldLabelProps,
        tooltip::HintTooltipProps,
    },
};
use zgui::prelude::*;
use zgui_ui_primitives::Binding;

pub fn reset_button(
    modified: Signal<bool, LocalStorage>,
    focus: NodeRef,
    testid: Option<&'static str>,
    reset: impl Fn() + Clone + 'static,
) -> AnyView {
    AnyView::new(view! {
        SettingReset {
            {move || {
                let reset = reset.clone();
                modified.get().then(||view! {
                    IconButton(icon=IconName::RotateCcw, label=Signal::derive_local(||i18n::text("settings-reset-default")),
                        attr:data-testid=testid.map(str::to_owned), on:click=move |_| {reset();focus.focus();})
                }.into_view())
            }}
        }
    })
}

fn field_row(
    label: &'static str,
    help: Option<&'static str>,
    title: NodeRef,
    reset: AnyView,
    control: AnyView,
) -> AnyView {
    let title = AnyView::new(view! {FieldLabel(node_ref=title) {{move || i18n::text(label)}}});
    let title = match help {
        Some(help) => AnyView::new(view! {box(class="gds-settings-label-trigger") {
            HintTooltip(text=Signal::derive_local(move || i18n::text(help))) {{title}}
        }}),
        None => title,
    };
    AnyView::new(view! {SettingRow {
        SettingLabel {
            {title}
            {help.map(|key| view! {SettingDescription {{move||i18n::text(key)}}}.into_view())}
        }
        SettingActions { {reset} SettingControl {{control}} }
    }})
}

pub fn toggle_with_reset(
    label: &'static str,
    help: Option<&'static str>,
    value: Binding<bool>,
    default: bool,
    focus: NodeRef,
    reset: impl Fn() + Clone + 'static,
) -> AnyView {
    let title = NodeRef::new();
    let modified = Signal::derive_local(move || value.get().is_some_and(|v| v != default));
    let control = AnyView::new(view! {ToggleSwitch(checked=value,labelled_by=Some(title))});
    field_row(
        label,
        help,
        title,
        reset_button(modified, focus, None, reset),
        control,
    )
}

pub fn toggle_setting(
    label: &'static str,
    help: Option<&'static str>,
    value: Binding<bool>,
    default: bool,
    focus: NodeRef,
) -> AnyView {
    toggle_with_reset(label, help, value, default, focus, move || {
        value.write(default)
    })
}

pub fn number_setting(
    label: &'static str,
    help: Option<&'static str>,
    value: Binding<f64>,
    range: NumberRange,
    unit: &'static str,
    default: f64,
    focus: NodeRef,
) -> AnyView {
    let title = NodeRef::new();
    let modified = Signal::derive_local(move || {
        value
            .get()
            .is_some_and(|v| !range.equivalent(v as f32, default as f32))
    });
    let reset = reset_button(modified, focus, None, move || value.write(default));
    let control = AnyView::new(view! {row(class="gds-setting-number-row"){
        NumberControl(value=value,range=range,label=i18n::text_untracked(label),labelled_by=Some(title))
        text(class="gds-setting-number-value") {{move||format!("{:.0}{unit}",value.get().unwrap_or(default))}}
    }});
    field_row(label, help, title, reset, control)
}
