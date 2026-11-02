//! Shared interface primitives. UI text uses the workspace's compact scale; message and code text
//! use the separate content font preferences.

mod content_row;
mod feedback;
pub(crate) mod input;
pub(crate) mod input_geometry;
pub(crate) mod time;
pub(crate) use content_row::{
    ContentRow, ContentRowActions, ContentRowContent, ContentRowLeading, ContentRowTitle,
};
pub(crate) use feedback::{Feedback, copy_button, message as feedback};
pub(crate) use gds::ui::button::Button;
pub(crate) use gds::ui::menu::popup as menu;
pub(crate) use gds::ui::tooltip;
pub(crate) use gds::ui::window as window_controls;

use gds_uim::el;
use gpui_kit::{
    App, Rems,
    assets::IconName,
    component::{ActiveTheme, Icon, Sizable, button::*},
    prelude::*,
    px,
};

pub(crate) const SMALL: Rems = gds::appearance::tokens::SMALL;
pub(crate) const HEADING: Rems = Rems(1.125);
pub(crate) const ICON: f32 = gds::appearance::tokens::ICON_SIZE;
pub(crate) const SMALL_ICON: f32 = gds::appearance::tokens::SMALL_ICON_SIZE;
pub(crate) const COMPACT_CONTROL: f32 = gds::appearance::tokens::ICON_BUTTON_SIZE;
pub(crate) const CONTROL: f32 = gds::appearance::tokens::ICON_BUTTON_SIZE;
pub(crate) const MENU_WIDTH: f32 = gds::ui::menu::WIDTH;
pub(crate) const MENU_MAX_HEIGHT: f32 = gds::ui::menu::MAX_HEIGHT;
pub(crate) const MENU_ROW_HEIGHT: f32 = 28.;

pub(crate) fn checkbox(
    id: impl Into<gpui_kit::ElementId>,
) -> gpui_kit::component::checkbox::Checkbox {
    gpui_kit::component::checkbox::Checkbox::new(id).text_size(SMALL)
}

pub(crate) fn icon_button(id: impl Into<gpui_kit::ElementId>, icon: IconName) -> Button {
    Button::icon_button(id).control_icon(icon)
}

pub(crate) fn delete_icon(cx: &App) -> Icon {
    Icon::new(IconName::Trash).text_color(cx.theme().danger)
}

pub(crate) fn action_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    el! {
        (id).Button::new {
            with_variant: ButtonVariant::Default;
            small;
            h: CONTROL.px;
        }
    }
}

pub(crate) fn icon_action(
    id: impl Into<gpui_kit::ElementId>,
    icon: IconName,
    hint: impl Into<gpui_kit::SharedString>,
) -> Button {
    let hint = hint.into();
    icon_button(id, icon)
        .accessibility_label(hint.clone())
        .tooltip(hint)
}

/// Reading and editing utilities stay quiet until the user targets them.
pub(crate) fn subtle_icon_button(
    id: impl Into<gpui_kit::ElementId>,
    icon: IconName,
    cx: &App,
) -> Button {
    el! {
        (id, icon).icon_button {
            text_color: $(secondary_foreground(cx));
        }
    }
}

pub(crate) fn secondary_foreground(cx: &App) -> gpui_kit::Hsla {
    cx.theme().muted_foreground
}

pub(crate) mod assets;
pub(crate) mod audio;
pub(crate) mod dialogs;
pub(crate) mod forms;
