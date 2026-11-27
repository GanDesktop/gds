//! Hue ring, rotating saturation/value triangle, preset swatches and exact RGB input.

mod model;
mod wheel;

use gds::{i18n::text_untracked as text, ui::field::InputFieldProps};
use zgui::{
    elements::kurbo::Vec2,
    geom::{Css, CssPx, Point},
    prelude::*,
    reactive::RenderEffect,
    view::events,
    vocab::{Key, NamedKey, PointerButton, Role},
};
use zgui_ui::{PopoverContentProps, overlay::OverlayState};
use zgui_ui_primitives::{Align, Binding, Placement, Side};

use crate::entities::document::RgbaColor;
use model::{Hsv, Part};

const WHEEL_RADIUS_RATIO: f32 = 0.47;
const HUE_KEY_STEP: f64 = 1.0 / 360.0;
const SV_KEY_STEP: f64 = 0.01;

#[derive(Clone)]
struct DragSession {
    part: Part,
    original_hsv: Hsv,
    original_field: String,
}

fn local_point(node: NodeRef, at: Point<CssPx, Css>) -> Option<Vec2> {
    let bounds = node.window_bounds()?;
    let radius = WHEEL_RADIUS_RATIO * bounds.size.width.0.min(bounds.size.height.0);
    if radius <= 0.0 {
        return None;
    }
    let scale = node.scale().max(0.01);
    let center_x = bounds.origin.x.0 + bounds.size.width.0 * 0.5;
    let center_y = bounds.origin.y.0 + bounds.size.height.0 * 0.5;
    Some(Vec2::new(
        f64::from((at.x.0 * scale - center_x) / radius),
        f64::from((at.y.0 * scale - center_y) / radius),
    ))
}

#[component]
pub fn ColorSelector(
    value: Signal<Option<RgbaColor>, LocalStorage>,
    #[prop(into)] open: Binding<bool>,
    on_change: UnsyncCallback<RgbaColor>,
) -> impl IntoView {
    let overlay = OverlayState::new(open, false, None).provide();
    let anchor = overlay.trigger();
    let display = move || {
        value
            .get()
            .map(model::format_hex)
            .unwrap_or_else(|| text("inspector-mixed"))
    };
    let last_custom = RwSignal::new_local(None::<RgbaColor>);
    let custom_selected = Memo::new(move |_| {
        overlay.is_open()
            || value.get().is_some_and(|color| {
                Some(color) == last_custom.get() || !RgbaColor::PALETTE.contains(&color)
            })
    });
    let choose_custom = UnsyncCallback::new(move |color| {
        last_custom.set(Some(color));
        on_change.run(color);
    });
    let initial_custom = custom_selected
        .get_untracked()
        .then(|| value.get_untracked())
        .flatten();
    let (swatch_wheel, swatch_handle) =
        zgui::custom::custom(wheel::SwatchWheel::new(initial_custom));
    let sync_swatch = RenderEffect::new(move |_| {
        let active_custom = custom_selected.get().then(|| value.get()).flatten();
        swatch_handle.update(|widget| widget.custom = active_custom);
        swatch_handle.repaint();
    });
    on_cleanup_local(move || drop(sync_swatch));

    let swatches: Vec<_> = RgbaColor::PALETTE
        .into_iter()
        .map(|color| {
            let label = model::format_hex(color);
            let selected = move || !custom_selected.get() && value.get() == Some(color);
            view! {
                control(
                    class = "images-swatch-button",
                    class:images-swatch-selected = selected,
                    a11y:role = Role::Button,
                    a11y:label = label.clone(),
                    a11y:toggled_on = selected,
                    tabindex = Focus::Sequential,
                    on:click = move |_| {
                        overlay.close();
                        last_custom.set(None);
                        on_change.run(color);
                    },
                ) {
                    box(
                        class = "images-swatch",
                        a11y:hidden = true,
                        style:background-color = label,
                    )
                }
            }
        })
        .collect();

    view! {
        row(class = "images-field-row") {
            text(
                class = "images-field-label",
                a11y:role = Role::Label,
                a11y:label = text("inspector-style-color"),
            ) {
                {text("inspector-style-color")}
            }
            row(class = "images-style-options images-palette-row") {
                {swatches}
                control(
                    node_ref = anchor,
                    class = "images-swatch-button",
                    class:images-swatch-selected = move || custom_selected.get(),
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:label = text("color-choose"),
                    a11y:description = display,
                    a11y:toggled_on = move || custom_selected.get(),
                    a11y:expanded = move || overlay.is_open(),
                    on:click = move |_| overlay.toggle(),
                ) {
                    box(
                        class = "images-swatch images-swatch-wheel",
                        a11y:hidden = true,
                    ) {
                        {swatch_wheel.class("images-swatch-wheel-paint")}
                    }
                }
            }
            PopoverContent(
                placement = Placement::new(Side::Left, Align::Start),
                class = "images-picker-popover",
            ) {
                ColorPickerPopover(value = value, on_change = choose_custom)
            }
        }
    }
}

#[component]
fn ColorPickerPopover(
    value: Signal<Option<RgbaColor>, LocalStorage>,
    on_change: UnsyncCallback<RgbaColor>,
) -> impl IntoView {
    let initial = value.get_untracked();
    let hsv = RwSignal::new_local(Hsv::from_rgb(initial.unwrap_or(RgbaColor::RED), 0.0));
    let field = RwSignal::new_local(initial.map(model::format_hex).unwrap_or_default());
    let drag = RwSignal::new_local(None::<DragSession>);
    let dial = NodeRef::new();
    let (wheel, wheel_handle) = zgui::custom::custom(wheel::Wheel::new(hsv.get_untracked()));

    let paint = RenderEffect::new(move |_| {
        let next = hsv.get();
        wheel_handle.update(|widget| widget.hsv = next);
        wheel_handle.repaint();
    });

    let sync = RenderEffect::new(move |previous| {
        let selected = value.get();
        if previous == Some(selected) {
            return selected;
        }
        if let Some(color) = selected {
            let current = hsv.get_untracked();
            if current.rgb() != color {
                hsv.set(Hsv::from_rgb(color, current.hue));
            }
            // Keep the text input untouched when it already parses to `color` so editing
            // preserves cursor position and casing.
            if model::parse_hex_rgb(&field.get_untracked()) != Some(color) {
                field.set(model::format_hex(color));
            }
        } else if !field.with_untracked(String::is_empty) {
            field.set(String::new());
        }
        selected
    });
    on_cleanup_local(move || drop((paint, sync)));

    let preview = move |part: Part, point: Vec2| {
        let next = hsv.get_untracked().pick(part, point);
        hsv.set(next);
        field.set(model::format_hex(next.rgb()));
    };
    let active_part =
        move || drag.with_untracked(|session| session.as_ref().map(|active| active.part));

    let invalid = Signal::derive_local(move || {
        let input = field.get();
        !input.trim().is_empty() && model::parse_hex_rgb(&input).is_none()
    });

    let swatch = AnyView::new(view! {
        box(
            class = "images-current-color",
            a11y:hidden = true,
            style:background-color = move || {
                Some(
                    model::parse_hex_rgb(&field.get())
                        .map_or_else(|| "transparent".to_owned(), model::format_hex),
                )
            },
        )
    });

    let on_pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.primary
            && ev.button == Some(PointerButton::Primary)
            && let Some(point) = local_point(dial, ev.position)
            && let Some(part) = hsv.get_untracked().hit(point)
        {
            dial.focus();
            drag.set(Some(DragSession {
                part,
                original_hsv: hsv.get_untracked(),
                original_field: field.get_untracked(),
            }));
            ev.capture_pointer();
            preview(part, point);
            ev.prevent_default();
        }
    };

    let on_pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
        if ev.primary
            && let Some(part) = active_part()
            && let Some(point) = local_point(dial, ev.position)
        {
            preview(part, point);
        }
    };

    let on_pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
        if ev.primary
            && ev.button == Some(PointerButton::Primary)
            && let Some(part) = active_part()
        {
            if let Some(point) = local_point(dial, ev.position) {
                preview(part, point);
            }
            drag.set(None);
            ev.release_pointer();
            on_change.run(hsv.get_untracked().rgb());
        }
    };

    let on_pointer_cancel = move |_: &mut EventCx<'_, events::PointerCancel>| {
        if let Some(session) = drag.try_update(Option::take).flatten() {
            hsv.set(session.original_hsv);
            field.set(session.original_field);
        }
    };

    let on_key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
        let held = ev.modifiers.held();
        if held.control() || held.alt() || held.meta() {
            return;
        }
        let mut next = hsv.get_untracked();
        match ev.key {
            Key::Named(NamedKey::ArrowLeft) => {
                next.hue = (next.hue - HUE_KEY_STEP).rem_euclid(1.0);
            }
            Key::Named(NamedKey::ArrowRight) => {
                next.hue = (next.hue + HUE_KEY_STEP).rem_euclid(1.0);
            }
            Key::Named(key @ (NamedKey::ArrowUp | NamedKey::ArrowDown)) => {
                let delta = if key == NamedKey::ArrowUp {
                    SV_KEY_STEP
                } else {
                    -SV_KEY_STEP
                };
                let axis = if held.shift() {
                    &mut next.saturation
                } else {
                    &mut next.value
                };
                *axis = (*axis + delta).clamp(0.0, 1.0);
            }
            _ => return,
        }
        hsv.set(next);
        field.set(model::format_hex(next.rgb()));
        on_change.run(next.rgb());
        ev.prevent_default();
        ev.stop_propagation();
    };

    view! {
        column(class = "images-color-controls") {
            control(
                node_ref = dial,
                class = "images-wheel",
                tabindex = Focus::Sequential,
                a11y:role = Role::Group,
                a11y:label = text("color-selector"),
                a11y:description = text("color-selector-keys"),
                on:pointer_down = on_pointer_down,
                on:pointer_move = on_pointer_move,
                on:pointer_up = on_pointer_up,
                on:pointer_cancel = on_pointer_cancel,
                on:key_down = on_key_down,
            ) {
                {wheel.class("images-wheel-paint")}
            }
            InputField(
                label = text("tool-custom-color"),
                leading = swatch,
                value = Binding::from(field),
                placeholder = "#RRGGBB",
                invalid = invalid,
                on_change = UnsyncCallback::new(move |input: String| {
                    if let Some(color) = model::parse_hex_rgb(&input) {
                        on_change.run(color);
                    }
                }),
            )
        }
    }
}
