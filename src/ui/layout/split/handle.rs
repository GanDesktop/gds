use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::vocab::{Key, NamedKey, Orientation, PointerButton};
use zgui::{component, view};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipTriggerProps};

use super::{KEY_STEP, SplitContext, State};

/// Interactive divider handle for [`super::VerticalSplit`].
#[component]
pub fn SplitHandle(state: State, #[prop(into, optional)] class: Classes) -> impl IntoView {
    super::install_split_stylesheets();
    let ctx = use_local_context::<SplitContext>();
    let container = ctx.as_ref().map(|c| c.container);
    let label = ctx.as_ref().and_then(|c| c.label.clone());
    let on_commit = ctx.and_then(|c| c.on_commit);

    let drag_start: RwSignal<Option<(f32, f32)>, LocalStorage> = RwSignal::new_local(None);
    let window = zgui::prelude::try_use_window();

    let usable_height = move || -> Option<f32> {
        let node = container?;
        let bounds = node.bounds()?;
        let scale = node.scale();
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let css_h = bounds.size.height.0 / scale;
        (css_h > 0.0).then_some((css_h - 1.0).max(0.0))
    };

    let options = state.options();
    let width_sig = state.width();
    let collapsed_sig = state.collapsed();
    let a11y_label = label.clone().unwrap_or_else(|| "Resize split".to_owned());

    let semantics = A11yBinding::new(Role::Splitter)
        .orientation(Orientation::Vertical)
        .label(a11y_label)
        .numeric_value(move || {
            if collapsed_sig.get() {
                0.0
            } else {
                f64::from(width_sig.get())
            }
        })
        .step(move |a11y| {
            a11y.numeric_range(f64::from(options.min), f64::from(options.max))
                .numeric_step(f64::from(KEY_STEP))
        });

    let own = Attrs::new()
        .attribute(zgui::view::AttrName::new("data-dragging"), move || {
            drag_start.get().is_some().then(|| "true".to_owned())
        })
        .a11y_from(semantics);

    let enter_win = window.clone();
    let leave_win = window;
    let commit_drag = on_commit;
    let commit_cancel = on_commit;
    let commit_key = on_commit;

    let handle_control = view! {
        control(
            class = "gds-split__handle",
            class = class,
            tabindex = Focus::Sequential,
            on:pointer_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                if ev.button != Some(PointerButton::Primary) || !ev.primary {
                    return;
                }
                let start_size = if state.collapsed().get_untracked() {
                    0.0
                } else {
                    state.width().get_untracked()
                };
                drag_start.set(Some((ev.position.y.0, start_size)));
                ev.capture_pointer();
                ev.stop_propagation();
            },
            on:pointer_move = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerMove>| {
                let Some((start_y, start_size)) = drag_start.get_untracked() else {
                    return;
                };
                let delta_y = start_y - ev.position.y.0;
                state.resize_to(start_size + delta_y, usable_height());
            },
            on:pointer_up = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerUp>| {
                if drag_start.get_untracked().is_some() {
                    drag_start.set(None);
                    ev.release_pointer();
                    if let Some(ref cb) = commit_drag {
                        cb.run(state.commit_size());
                    }
                }
            },
            on:pointer_cancel = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerCancel>| {
                if drag_start.get_untracked().is_some() {
                    drag_start.set(None);
                    ev.release_pointer();
                    if let Some(ref cb) = commit_cancel {
                        cb.run(state.commit_size());
                    }
                }
            },
            on:pointer_enter = move |_| {
                if let Some(ref win) = enter_win {
                    win.set_cursor(CursorStyle::ResizeNorthSouth);
                }
            },
            on:pointer_leave = move |_| {
                if drag_start.get_untracked().is_none()
                    && let Some(ref win) = leave_win
                {
                    win.set_cursor(CursorStyle::Default);
                }
            },
            on:key_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::KeyDown>| {
                if !ev.modifiers.is_empty() {
                    return;
                }
                match ev.key {
                    Key::Named(NamedKey::ArrowUp) => state.step_by(KEY_STEP, usable_height()),
                    Key::Named(NamedKey::ArrowDown) => state.step_by(-KEY_STEP, usable_height()),
                    Key::Named(NamedKey::Home) => state.snap_min(usable_height()),
                    Key::Named(NamedKey::End) => state.snap_max(usable_height()),
                    _ => return,
                }
                ev.prevent_default();
                ev.stop_propagation();
                if let Some(ref cb) = commit_key {
                    cb.run(state.commit_size());
                }
            },
            {..own}
        ) {
            box(class = "gds-split__line", a11y:hidden = true) {}
        }
    };

    if let Some(tip) = label {
        let tip = Signal::stored_local(tip);
        AnyView::new(view! {
            Tooltip(delay = crate::appearance::tokens::TOOLTIP_DELAY) {
                TooltipTrigger(class = "gds-split__trigger") {{handle_control}}
                TooltipContent {{move || tip.get()}}
            }
        })
    } else {
        AnyView::new(view! {
            box(class = "gds-split__trigger") {{handle_control}}
        })
    }
}
