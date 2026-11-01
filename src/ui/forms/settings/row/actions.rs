mod reset;

use gds_uim::el;
use gpui_kit::{prelude::*, *};
pub use reset::SettingReset;

use super::super::CONTROL_GAP;
use crate::ui::container::container_traits;

/// The row's control group, including optional reset actions.
#[derive(IntoElement)]
pub struct SettingActions {
    base: Div,
}

impl Default for SettingActions {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_shrink_0; items_center; gap: CONTROL_GAP.px; } },
        }
    }
}

container_traits!(SettingActions);

impl RenderOnce for SettingActions {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
