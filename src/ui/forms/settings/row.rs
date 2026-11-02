mod actions;
mod control;
mod description;
mod label;

pub use actions::{SettingActions, SettingReset};
pub use control::SettingControl;
pub use description::SettingDescription;
use gds_uim::el;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _};
use gpui_kit::{IntoElement, prelude::*, *};
pub use label::SettingLabel;

use crate::ui::container::container_traits;

/// A setting's description and controls, composed as ordinary children.
#[derive(IntoElement)]
pub struct SettingRow {
    base: Div,
}

impl Default for SettingRow {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; items_center; justify_between; gap_6; py_4; border_b_1; } },
        }
    }
}

container_traits!(SettingRow);

impl RenderOnce for SettingRow {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = std::mem::take(self.base.style());
        el! { $(self.base) { border_color: $(cx.theme().border); refine_style: $&style; } }
    }
}
