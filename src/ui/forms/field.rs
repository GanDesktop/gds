//! A labelled field with optional explanatory or validation content.
mod search_choice;
pub use search_choice::{SearchOption, search_choice};

use crate::ui::container::container_traits;
use anti_webui::prelude::*;
use gds_uim::el;
pub struct Field {
    base: Div,
}
impl Default for Field {
    fn default() -> Self {
        Self {
            base: el! { .div.{ flex; flex_col; gap: 6.0_f32; w_full } },
        }
    }
}
container_traits!(Field);
