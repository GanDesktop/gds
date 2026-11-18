//! Settings row parts keep label, explanation, control, and reset anatomy explicit.
use crate::ui::container::container_traits;
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
    SettingsRow,
    el! { .div.{ flex; items_center; gap: 24.0_f32; py: 16.0_f32; w_full; border_b:1.0_f32; dyn_style:s=>$(s.border_color(theme().border)) } }
);
part!(
    SettingsLabel,
    el! { .div.{ flex; flex_col; flex_1; min_w: 0.0_f32; gap: 4.0_f32 } }
);
part!(
    SettingsDescription,
    el! { .div.{ dyn_style: s => $(s.text_color(theme().text_muted).text_size(crate::app::Gds::current().appearance().typography().scaled(12.0))) } }
);
part!(
    SettingsControl,
    el! { .div.{ flex; items_center; justify_end; flex_none; w: super::CONTROL_WIDTH; gap: 4.0_f32 } }
);
part!(
    SettingsActions,
    el! { .div.{ flex; items_center; gap:8.0_f32; flex_none } }
);

part!(
    SettingsReset,
    el! { .div.{ flex; items_center; justify_center; flex_none; size:32.0_f32 } }
);
