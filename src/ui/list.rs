//! Anatomical list surface; virtualization belongs to the caller's projection.
use super::container::container_traits;
use anti_webui::prelude::*;
use gds_uim::el;
pub struct ListSurface {
    base: Div,
}
impl Default for ListSurface {
    fn default() -> Self {
        Self {
            base: el! { .div.{ flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32; overflow_y_scroll } },
        }
    }
}
container_traits!(ListSurface);
