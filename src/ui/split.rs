//! Focusable vertical pane divider.
use anti_webui::prelude::*;
use gds_uim::el;

pub const DIVIDER_HEIGHT: f32 = 9.0;
pub const KEY_STEP: f32 = 12.0;

pub fn handle(
    label: &'static str,
    drag: impl Fn(&GrabEvent) + 'static,
    resize: impl Fn(f32) + 'static,
) -> Div {
    let divider = el! { .div.{
        relative; w_full; h: DIVIDER_HEIGHT; flex_none; focusable; cursor: CursorIcon::NsResize;
        access: move a => $(a.role(Role::Separator).label(crate::i18n::text(label)));
        on_grab: drag;
        on_key_down: move event => ${
            if event.modifiers != Modifiers::default() {
                return;
            }
            let step = match event.key {
                Key::Named(NamedKey::ArrowUp) => -KEY_STEP,
                Key::Named(NamedKey::ArrowDown) => KEY_STEP,
                _ => return,
            };
            resize(step);
            event.prevent_default();
            event.stop_propagation();
        };
        focused: s => $(s.outline(1.0_f32).outline_offset(-1.0_f32).outline_color(theme().focus_ring));
    } };
    tooltip_with(divider, TooltipOptions::new(), move |_| {
        move || {
            el! { .div.{px: 8.0_f32; py: 4.0_f32; rounded: 4.0_f32;
                dyn_style: s => $(s.bg(theme().tooltip).text_color(theme().on_tooltip));
            }.[(move () => label.crate::i18n::text).text] }
        }
    })
}
