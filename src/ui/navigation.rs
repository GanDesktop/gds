//! Composable suite navigation rows, independent of the sidebar's layout.

use crate::appearance::tokens::{
    ICON_BUTTON_SIZE, SIDEBAR_ICON_SIZE, SIDEBAR_ITEM_HEIGHT, SMALL, TOOLTIP_DELAY,
};
mod action;
mod actions;
mod content;
mod details;
mod header;
mod icon;
mod label;

pub use actions::NavItemActions;
pub use content::NavItemContent;
pub use details::NavItemDetails;
pub use header::NavItemHeader;
pub use label::NavItemLabel;

pub use action::hover_action;
pub use icon::{pinned_icon, with_pin};

use super::button::Button;
use super::shortcut::ShortcutHint as Hint;
use super::*;
use gds_uim::{component, el};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Disableable as _, Icon, Selectable as _, button::ButtonVariants as _,
};
use gpui_kit::{prelude::*, *};

const ITEM_GAP: f32 = 8.;
const ITEM_RADIUS: f32 = 6.;

/// Navigation rows supply their own full-width content to Kit's Button.
/// This avoids its independently centered label container.
#[component(gpui = gpui_kit)]
#[derive(IntoElement)]
pub struct NavItem {
    id: ElementId,
    button: Button,
    icon: IconName,
    label: SharedString,
    #[attr]
    compact: bool,
    #[attr(option)]
    presentation: Option<sidebar::SidebarPresentation>,
    command: Option<Shortcut>,
    shortcut: Option<Shortcut>,
    disabled: bool,
    /// Describe the compact row's state and properties on hover.
    #[attr(into, option)]
    tooltip_text: Option<SharedString>,
    #[attr]
    tooltip_enabled: bool,
    /// Secondary content shares the row's hover, selection and click target.
    #[attr(any_element, option)]
    details: Option<AnyElement>,
    /// Independent controls in the row's trailing slots.
    #[attr]
    trailing: Vec<AnyElement>,
}

impl NavItem {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        let id = id.into();
        let label = label.into();
        let button =
            el! { (~id).Button::new { ghost; text_size: SMALL; accessibility_label: ~label } };
        Self {
            id,
            button,
            icon,
            label,
            compact: false,
            presentation: None,
            command: None,
            shortcut: None,
            disabled: false,
            tooltip_text: None,
            tooltip_enabled: true,
            details: None,
            trailing: Vec::new(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.button = self.button.selected(selected);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.button = self.button.disabled(disabled);
        self
    }
    pub fn command(mut self, command: impl Into<Shortcut>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// The binding this row performs outside the file workspace, where the
    /// workspace command list does not apply.
    pub fn shortcut(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.button = self.button.on_click(handler);
        self
    }
}

impl RenderOnce for NavItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            button,
            icon,
            label,
            compact,
            presentation,
            command,
            shortcut,
            disabled,
            tooltip_text,
            tooltip_enabled,
            details,
            trailing,
        } = self;
        let compact = presentation.map_or(compact, |value| value.progress() == 0.0);
        let show_actions = presentation.is_none_or(|value| value.actions_visible());
        let icon_inset = presentation.map(|value| {
            value.interpolate(
                ITEM_GAP,
                (crate::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH
                    - 2.0 * crate::appearance::tokens::SIDEBAR_PADDING
                    - SIDEBAR_ICON_SIZE)
                    / 2.0,
            )
        });
        let trailing_width = trailing.len() as f32 * ICON_BUTTON_SIZE;
        let header_right_padding = px(ITEM_GAP + trailing_width);
        let trailing: Vec<_> = trailing.into_iter().map(|control| {
            el! { .div { size: ICON_BUTTON_SIZE.px; flex; items_center; justify_center } [control] }
        }).collect();
        let has_details = details.is_some();
        let selected = button.is_selected();
        let content = el! {
            .NavItemContent::default [
                .NavItemHeader::default {
                    overflow_hidden;
                    @if (let Some(inset) = icon_inset)
                        pl: inset.px; pr: header_right_padding;
                    @else
                        @if (compact) justify_center; @else pr: header_right_padding; @end;
                    @end;
                } [
                    icon.Icon::new { size: SIDEBAR_ICON_SIZE.px; flex_shrink_0 },
                    @if (!compact || presentation.is_some())
                        .NavItemLabel::default {
                            @if (let Some(value) = presentation) opacity: $(value.label_opacity()); @end;
                        } [~label],
                    @end,
                ],
                @if (let Some(details) = details) .NavItemDetails::default [details], @end,
            ]
        };
        let button = el! {
            button { w_full; p_0; rounded: ITEM_RADIUS.px;
                @if (has_details) h_auto; @else h: SIDEBAR_ITEM_HEIGHT.px; @end;
                text_color: $(cx.theme().sidebar_foreground);
                @if (selected)
                    bg: $(cx.theme().sidebar_accent); text_color: $(cx.theme().sidebar_accent_foreground);
                @end;
            } [content]
        };
        let shortcut = shortcut.or(command);
        // A full-width row shows its key in its own trailing space, where it
        // covers no other row; a collapsed row has no such space, so its label
        // becomes a corner badge on the row itself.
        let hint = match &shortcut {
            Some(shortcut) if !disabled => Some(if compact {
                keytips::anchor(window, cx, id.clone(), shortcut.clone())
            } else {
                keytips::row_anchor(window, cx, id.clone(), shortcut.clone())
            }),
            _ => None,
        };
        let tooltip_text = tooltip_text.unwrap_or_else(|| label.clone());
        let tooltip = match shortcut {
            Some(shortcut) => Hint::shortcut(tooltip_text, shortcut),
            None => Hint::command(tooltip_text, None),
        }
        .builder();
        el! {
            .div {
                # id; relative; w_full; flex_shrink_0;
                @if (!has_details) h: SIDEBAR_ITEM_HEIGHT.px; @end;
                @if (tooltip_enabled) tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip; @end;
            } [
                button,
                @if (show_actions && !trailing.is_empty())
                    .NavItemActions::default [
                        ..trailing,
                    ],
                @end,
                @if (let Some(hint) = hint) hint, @end,
            ]
        }
    }
}
