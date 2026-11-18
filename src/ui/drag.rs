//! Floating preview card for a drag carrying several objects.

use crate::{app::Gds, appearance::tokens::DataListStyle, ui::container::container_traits};
use anti_webui::prelude::*;
use gds_uim::el;

macro_rules! part {
    ($name:ident, $base:expr) => {
        pub struct $name {
            base: Div,
        }

        impl Default for $name {
            fn default() -> Self {
                Self { base: $base }
            }
        }

        container_traits!($name);
    };
}

part!(
    DragCard,
    el! { .div.{
        flex; flex_col; min_w: 140.0_f32; max_w: 248.0_f32; overflow_hidden;
        border: 1.0_f32; rounded: 8.0_f32; shadow_md;
        dyn_style: style => $(style.bg(theme().overlay).border_color(theme().border).text_color(theme().text));
    } }
);

part!(
    DragCardHeader,
    el! { .div.{
        flex; items_center; gap: 8.0_f32; min_w: 0.0_f32; px: 10.0_f32; py: 6.0_f32;
        border_b: 1.0_f32; font_weight: FontWeight::SEMIBOLD;
        dyn_style: style => $(style.border_color(theme().border)
            .text_size(Gds::current().appearance().typography().scaled(DataListStyle::META_SIZE)));
    } }
);

part!(
    DragCardContent,
    el! { .div.{
        flex; flex_col; gap: 3.0_f32; min_w: 0.0_f32; px: 10.0_f32; py: 6.0_f32;
        dyn_style: style => $(style.text_size(Gds::current().appearance().typography().scaled(DataListStyle::NAME_SIZE)));
    } }
);

part!(
    DragCardFooter,
    el! { .div.{
        flex; items_center; min_w: 0.0_f32; px: 10.0_f32; pb: 6.0_f32;
        dyn_style: style => $(style.text_color(theme().text_muted)
            .text_size(Gds::current().appearance().typography().scaled(DataListStyle::META_SIZE)));
    } }
);
