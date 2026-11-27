//! One style editor for the active tool or selected annotations.

use std::collections::HashMap;

use gds::{
    i18n::text_untracked as text,
    ui::{
        IconButtonProps, IconName, IconProps,
        field::{InputFieldProps, SearchChoiceButtonProps},
    },
};
use zgui::{prelude::*, reactive::RenderEffect, view::Scoped};
use zgui_ui::{PopoverContentProps, overlay::OverlayState};
use zgui_ui_primitives::{Align, Binding, Placement, Side};

use super::super::{ActivePopover, Page};
use crate::{
    entities::document::RgbaColor,
    features::edit_canvas::{
        ActiveInteraction, MARKER_DIAMETER_RANGE, PropertyValue, StyleApplyOutcome, StyleTarget,
        ToolMode, ToolSettings,
    },
    widgets::color_selector::ColorSelectorProps,
};

const SIZE_EPSILON: f32 = 1e-3;
const PRESET_LABEL_KEYS: [&str; 3] = ["size-small", "size-medium", "size-large"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::pages::workspace::ui) enum Dimension {
    Stroke,
    Marker,
    Font,
    Mosaic,
    Blur,
}

#[derive(Clone, Copy, PartialEq)]
struct CustomSize {
    value: f32,
    selected: bool,
}

#[derive(Default)]
pub(in crate::pages::workspace::ui) struct CustomSizes(
    HashMap<(StyleTarget, Dimension), CustomSize>,
);

#[derive(Clone, Copy)]
struct SizeBounds {
    min: f32,
    max: f32,
    integer_only: bool,
}

impl SizeBounds {
    fn parse(self, input: &str) -> Option<f32> {
        input.trim().parse::<f32>().ok().filter(|value| {
            (self.min..=self.max).contains(value) && (!self.integer_only || value.fract() == 0.0)
        })
    }
}

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < SIZE_EPSILON
}

fn format_editable_size(value: f32) -> String {
    let rounded = value.round();
    if approx_eq(value, rounded) {
        return format!("{}", rounded as i64);
    }
    let formatted = format!("{value:.3}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

impl Dimension {
    const ALL: [Self; 5] = [
        Self::Stroke,
        Self::Marker,
        Self::Font,
        Self::Mosaic,
        Self::Blur,
    ];

    fn default_size(self) -> f32 {
        let settings = ToolSettings::default();
        match self {
            Self::Stroke => settings.stroke_width,
            Self::Marker => settings.marker_diameter,
            Self::Font => settings.font_size,
            Self::Mosaic => settings.mosaic_block_size as f32,
            Self::Blur => settings.blur_radius as f32,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Stroke => "inspector-style-stroke",
            Self::Marker => "inspector-style-marker-diameter",
            Self::Font => "inspector-style-font-size",
            Self::Mosaic => "inspector-style-mosaic",
            Self::Blur => "inspector-style-blur",
        }
    }

    fn presets(self) -> [u32; 3] {
        match self {
            Self::Stroke => [2, 3, 6],
            Self::Marker => [24, 32, 48],
            Self::Font => [14, 20, 30],
            Self::Mosaic => [8, 14, 22],
            Self::Blur => [4, 8, 14],
        }
    }

    fn bounds(self) -> SizeBounds {
        match self {
            Self::Stroke => SizeBounds {
                min: 1.0,
                max: 32.0,
                integer_only: false,
            },
            Self::Marker => SizeBounds {
                min: *MARKER_DIAMETER_RANGE.start(),
                max: *MARKER_DIAMETER_RANGE.end(),
                integer_only: false,
            },
            Self::Font => SizeBounds {
                min: 8.0,
                max: 160.0,
                integer_only: false,
            },
            Self::Mosaic => SizeBounds {
                min: 2.0,
                max: 64.0,
                integer_only: true,
            },
            Self::Blur => SizeBounds {
                min: 1.0,
                max: 64.0,
                integer_only: true,
            },
        }
    }

    fn apply(self, page: Page, value: f32) {
        if page.window().busy.get_untracked() {
            return;
        }
        let outcome = page.change(|state| {
            let doc = state.doc.as_mut();
            let history = &mut state.history;
            match self {
                Self::Stroke => state.tool.apply_stroke_width(value, doc, history),
                Self::Font => state.tool.apply_font_size(value, doc, history),
                Self::Marker => state.tool.apply_marker_diameter(value, doc, history),
                Self::Mosaic => {
                    state
                        .tool
                        .apply_mosaic_block_size(value.round() as u32, doc, history)
                }
                Self::Blur => state
                    .tool
                    .apply_blur_radius(value.round() as u32, doc, history),
            }
        });
        if outcome != StyleApplyOutcome::None {
            page.render();
        }
        page.changed();
    }
}

fn pixels(value: PropertyValue<u32>) -> PropertyValue<f32> {
    match value {
        PropertyValue::Uniform(size) => PropertyValue::Uniform(size as f32),
        PropertyValue::Mixed => PropertyValue::Mixed,
        PropertyValue::Unsupported => PropertyValue::Unsupported,
    }
}

#[derive(Clone, PartialEq)]
struct Values {
    family: PropertyValue<String>,
    color: PropertyValue<RgbaColor>,
    stroke: PropertyValue<f32>,
    marker: PropertyValue<f32>,
    font: PropertyValue<f32>,
    mosaic: PropertyValue<f32>,
    blur: PropertyValue<f32>,
}

impl Values {
    fn read(page: Page) -> Self {
        use PropertyValue::{Uniform, Unsupported};
        use ToolMode::*;

        page.read(|state| {
            if let ActiveInteraction::EditingText(draft) = &state.tool.interaction {
                return Self {
                    family: Uniform(draft.family.clone()),
                    color: Uniform(draft.color),
                    font: Uniform(draft.font_size),
                    stroke: Unsupported,
                    marker: Unsupported,
                    mosaic: Unsupported,
                    blur: Unsupported,
                };
            }
            if state.tool.style_target() == Some(StyleTarget::Objects)
                && let Some(selection) = state.doc.as_ref().and_then(|doc| {
                    let items =
                        if matches!(state.tool.interaction, ActiveInteraction::Resizing { .. }) {
                            page.window().paint.track();
                            state.preview_objects.as_deref().unwrap_or(&doc.annotations)
                        } else {
                            &doc.annotations
                        };
                    state.tool.selection_style_summary(items)
                })
            {
                return Self {
                    family: selection.font_family,
                    color: selection.color,
                    stroke: selection.stroke_width,
                    marker: selection.marker_diameter,
                    font: selection.font_size,
                    mosaic: pixels(selection.mosaic_block_size),
                    blur: pixels(selection.blur_radius),
                };
            }
            let tool = state.tool.tool;
            let settings = &state.tool.settings;
            let is_shape = matches!(tool, Rect | Ellipse | Arrow | Line | Brush);
            Self {
                family: if tool == Text {
                    Uniform(settings.font_family.clone())
                } else {
                    Unsupported
                },
                color: if is_shape || matches!(tool, Text | Number) {
                    Uniform(settings.color)
                } else {
                    Unsupported
                },
                marker: if tool == Number {
                    Uniform(settings.marker_diameter)
                } else {
                    Unsupported
                },
                stroke: if is_shape {
                    Uniform(settings.stroke_width)
                } else {
                    Unsupported
                },
                font: if tool == Text {
                    Uniform(settings.font_size)
                } else {
                    Unsupported
                },
                mosaic: if tool == Mosaic {
                    Uniform(settings.mosaic_block_size as f32)
                } else {
                    Unsupported
                },
                blur: if tool == Blur {
                    Uniform(settings.blur_radius as f32)
                } else {
                    Unsupported
                },
            }
        })
    }

    fn size(&self, dimension: Dimension) -> PropertyValue<f32> {
        match dimension {
            Dimension::Stroke => self.stroke,
            Dimension::Marker => self.marker,
            Dimension::Font => self.font,
            Dimension::Mosaic => self.mosaic,
            Dimension::Blur => self.blur,
        }
    }
}

#[component]
fn SizeRow(
    page: Page,
    target: StyleTarget,
    dimension: Dimension,
    values: Memo<Values>,
) -> impl IntoView {
    let current = Memo::new(move |_| values.get().size(dimension));
    let key = dimension.key();
    let label = text(key);
    let presets = dimension.presets();
    let bounds = dimension.bounds();
    let memory = page.window().custom_sizes;
    let slot = (
        match target {
            StyleTarget::TextDraft => StyleTarget::Objects,
            other => other,
        },
        dimension,
    );
    let initial = CustomSize {
        value: dimension.default_size(),
        selected: false,
    };
    let saved =
        Memo::new(move |_| memory.with(|sizes| sizes.0.get(&slot).copied().unwrap_or(initial)));
    let remember = move |value, selected| {
        memory.update(|sizes| {
            sizes.0.insert(slot, CustomSize { value, selected });
        });
    };
    let is_preset = move |value| {
        presets
            .iter()
            .any(|&preset| approx_eq(preset as f32, value))
    };
    let custom_value = Memo::new(move |_| match current.get() {
        PropertyValue::Uniform(value) if !is_preset(value) => value,
        _ => saved.get().value,
    });
    let custom_selected = move || {
        matches!(
            current.get(),
            PropertyValue::Uniform(value)
                if !is_preset(value)
                    || (saved.get().selected && approx_eq(saved.get().value, value))
        )
    };
    let display = move || format!("{} px", custom_value.get().round() as i64);
    let open = page.popover_binding(ActivePopover::CustomSize(dimension));
    let is_open = move || open.get().unwrap_or(false);
    let draft = RwSignal::new_local(String::new());
    let anchor = NodeRef::new();
    let input = NodeRef::new();
    let focus = RenderEffect::new(move |previous: Option<bool>| {
        let editing = open.get().unwrap_or(false) && input.get().is_some();
        if editing && previous != Some(true) {
            input.focus();
            input.select_all();
        }
        editing
    });
    on_cleanup_local(move || drop(focus));

    let valid = Memo::new(move |_| bounds.parse(&draft.get()));
    let commit = move || {
        if let Some(value) = valid.get_untracked() {
            remember(value, true);
            dimension.apply(page, value);
            open.write(false);
        }
    };
    let trigger = move || {
        let currently_open = open.get_untracked().unwrap_or(false);
        if !currently_open {
            draft.set(format_editable_size(custom_value.get_untracked()));
        }
        open.write(!currently_open);
    };
    let popup = Scoped::new(move || {
        OverlayState::new(open, false, None)
            .anchored_to(anchor)
            .provide();
        view! {
            PopoverContent(
                placement = Placement::new(Side::Left, Align::Start),
                class = "images-size-popover",
            ) {
                text(class = "images-heading") {{text(key)}}
                row(class = "images-size-input") {
                    InputField(
                        compact = true,
                        node_ref = input,
                        label = text(key),
                        value = Binding::from(draft),
                        placeholder = format!("{}–{}", bounds.min, bounds.max),
                        invalid = Signal::derive_local(move || {
                            !draft.get().is_empty() && valid.get().is_none()
                        }),
                        on:key_down = move |event| {
                            if event.key == Key::Named(NamedKey::Enter) {
                                commit();
                                event.prevent_default();
                                event.stop_propagation();
                            }
                        },
                    )
                    IconButton(
                        icon = IconName::Check,
                        label = text("dialog-confirm"),
                        disabled = Signal::derive_local(move || valid.get().is_none()),
                        on:click = move |_| commit(),
                    )
                }
                text(class = "images-muted") {{format!("{}–{} px", bounds.min, bounds.max)}}
            }
        }
    });
    let preset_buttons: Vec<_> = presets
        .into_iter()
        .zip(PRESET_LABEL_KEYS)
        .map(|(preset, size_key)| {
            let selected = move || {
                !custom_selected()
                    && matches!(
                        current.get(),
                        PropertyValue::Uniform(value) if approx_eq(value, preset as f32)
                    )
            };
            view! {
                control(
                    class = "images-chip",
                    class:images-chip-selected = selected,
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:toggled_on = selected,
                    a11y:label = format!("{} {preset}", text(key)),
                    on:click = move |_| {
                        open.write(false);
                        remember(custom_value.get_untracked(), false);
                        dimension.apply(page, preset as f32);
                    },
                ) {{text(size_key)}}
            }
        })
        .collect();

    view! {
        row(class = "images-field-row") {
            text(
                class = "images-field-label",
                a11y:role = Role::Label,
                a11y:label = label.clone(),
            ) {{label}}
            row(class = "images-style-options") {
                {move || {
                    (current.get() == PropertyValue::Mixed).then(|| {
                        view! {
                            text(a11y:label = text("inspector-mixed")) {"—"}
                        }
                    })
                }}
                {preset_buttons}
                control(
                    node_ref = anchor,
                    class = "images-chip images-custom-size",
                    class:images-chip-selected = custom_selected,
                    a11y:toggled_on = custom_selected,
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:label = format!("{} · {}", text(key), text("size-custom")),
                    a11y:description = display,
                    a11y:expanded = is_open,
                    on:click = move |_| trigger(),
                ) {
                    text {{display}}
                    Icon(name = IconName::ChevronDown, size = 10.0, a11y:hidden = true)
                }
            }
            {popup}
        }
    }
}

#[component]
pub(super) fn StyleControls(page: Page, target: StyleTarget) -> impl IntoView {
    let values = Memo::new(move |_| {
        page.window().version.track();
        Values::read(page)
    });
    let initial = values.get_untracked();
    let color = Signal::derive_local(move || match values.get().color {
        PropertyValue::Uniform(value) => Some(value),
        _ => None,
    });
    let apply_color = UnsyncCallback::new(move |next_color| {
        if page.window().busy.get_untracked() {
            return;
        }
        let outcome = page.change(|state| {
            state
                .tool
                .apply_color(next_color, state.doc.as_mut(), &mut state.history)
        });
        if outcome != StyleApplyOutcome::None {
            page.render();
        }
        page.changed();
    });
    let font_family = Binding::controlled(
        Signal::derive_local(move || match values.get().family {
            PropertyValue::Uniform(name) => name,
            _ => String::new(),
        }),
        move |name: String| {
            if name.is_empty() || page.window().busy.get_untracked() {
                return;
            }
            let outcome = page.change(|state| {
                state
                    .tool
                    .apply_font_family(name, state.doc.as_mut(), &mut state.history)
            });
            if outcome != StyleApplyOutcome::None {
                page.render();
            }
            page.changed();
        },
    );
    let font_options = page.window().fonts;
    let show_color = initial.color != PropertyValue::Unsupported;
    let show_family = initial.family != PropertyValue::Unsupported;
    let size_rows: Vec<_> = Dimension::ALL
        .into_iter()
        .filter(|dimension| initial.size(*dimension) != PropertyValue::Unsupported)
        .map(|dimension| {
            view! {
                SizeRow(
                    page = page,
                    target = target,
                    dimension = dimension,
                    values = values,
                )
            }
            .into_view()
        })
        .collect();

    view! {
        column(class = "images-fields") {
            {show_color.then(|| {
                view! {
                    ColorSelector(
                        value = color,
                        open = page.popover_binding(ActivePopover::ColorSelector),
                        on_change = apply_color,
                    )
                }
                .into_view()
            })}
            {show_family.then(|| {
                let label = text("annotation-font");
                view! {
                    row(class = "images-field-row") {
                        text(
                            class = "images-field-label",
                            a11y:role = Role::Label,
                            a11y:label = label.clone(),
                        ) {{label}}
                        SearchChoiceButton(
                            value = font_family,
                            options = Signal::derive_local(move || font_options.get()),
                            label = text("annotation-font"),
                            search_label = text("font-search"),
                            empty_text = text("font-no-results"),
                            placeholder = text("inspector-mixed"),
                        )
                    }
                }
            })}
            {size_rows}
        }
    }
}
