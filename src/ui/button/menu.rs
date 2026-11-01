use super::Button;
use crate::appearance::tokens::{APPLICATION_MENU_ICON_SIZE, ICON_SIZE, TOOLTIP_DELAY};
use gds_uim::{el, m};
use gpui_kit::{assets::IconName, component::Icon, prelude::*, *};

const HOVER_GROUP: &str = "gds-application-menu";

impl Button {
    /// The application mark becomes the menu glyph while the pointer is over the button.
    pub fn application_menu(id: impl Into<ElementId>, icon: impl Into<SharedString>) -> Self {
        let icon = icon.into();
        let artwork = el! {
            .div { relative; size: APPLICATION_MENU_ICON_SIZE.px; flex_shrink_0; } [
                .div { absolute; inset_0; group_hover: HOVER_GROUP, m! { invisible; }; } [
                    .Icon::default { path: icon; size: APPLICATION_MENU_ICON_SIZE.px; },
                ],
                .div { absolute; inset_0; invisible; flex; items_center; justify_center;
                    group_hover: HOVER_GROUP, m! { visible; };
                } [IconName::Menu.Icon::new { size: ICON_SIZE.px; }],
            ]
        };
        Self::icon_button(id)
            .group(HOVER_GROUP)
            .tooltip_show_delay(TOOLTIP_DELAY)
            .child(artwork)
    }
}
