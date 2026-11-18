//! Shared keyboard focus outline and shortcut for a window region.
use super::keytips::Anchor;
use anti_webui::prelude::*;
use gds_uim::el;

pub fn surface(focus: NodeRef, stroke: &'static str) -> Div {
    let keyed = signal(false);
    el! { .div.{
        relative; min_w: 0.0_f32; min_h: 0.0_f32; node_ref: focus; focusable;
        access: a => $(a.role(Role::Group));
        on_focus_visible: move visible => keyed.{set: visible};
        dyn_style: move s => $(s.outline(if keyed.get() { 1.0_f32 } else { 0.0_f32 })
            .outline_offset(-1.0_f32).outline_color(theme().focus_ring));
    }.[ stroke.Anchor::new ] }
}
