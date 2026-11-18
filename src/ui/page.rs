//! Composable page anatomy; applications own state and each part's content.
use super::container::container_traits;
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
    ContentPage,
    el! { .div.{ flex; flex_col; flex_1; size_full; min_h: 0.0_f32; min_w: 0.0_f32 } }
);
part!(
    PageHeader,
    el! { .div.{ flex; items_center; flex_none; gap: 12.0_f32; px: 24.0_f32; py: 16.0_f32 } }
);
part!(
    PageTitle,
    el! { .div.{ flex; flex_col; flex_1; min_w: 0.0_f32; font_weight: FontWeight::SEMIBOLD; dyn_style:s=>$(s.text_size(crate::app::Gds::current().appearance().typography().scaled(22.0))) } }
);
part!(
    PageActions,
    el! { .div.{ flex; items_center; flex_none; gap: 8.0_f32 } }
);
part!(
    PageContent,
    el! { .div.{ flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32 } }
);
part!(
    PageTableArea,
    el! { .div.{ flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32; px: 18.0_f32 } }
);
part!(
    PageNotice,
    el! { .div.{ flex; flex_col; gap: 6.0_f32; p: 16.0_f32;
        dyn_style: s => $(s.bg(theme().surface_muted).text_color(theme().text_muted));
    } }
);
impl ContentPage {
    pub const TABLE_INSET: f32 = 18.0;
}
