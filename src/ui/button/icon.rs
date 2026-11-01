//! Icon-only controls sharing suite sizing and shortcut presentation.

use crate::appearance::tokens::{ICON_BUTTON_SIZE, TOOLTIP_DELAY};
use crate::ui::shortcut::ShortcutHint as Hint;
use crate::ui::*;
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{Disableable as _, Selectable as _, button::ButtonVariants as _};
use gpui_kit::{prelude::*, *};

#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    button: button::Button,
    label: SharedString,
    command: Option<Shortcut>,
    /// A binding outside the workspace command set; it wins when both are set.
    shortcut: Option<Shortcut>,
    disabled: bool,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        let button = button::Button::icon_button(id.clone());
        Self {
            id,
            button,
            label: "".into(),
            command: None,
            shortcut: None,
            disabled: false,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.button = self.button.control_icon(icon);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self.button = self.button.accessibility_label(self.label.clone());
        self
    }

    pub fn command(mut self, command: impl Into<Shortcut>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// The binding this control runs outside the workspace command set.
    ///
    /// Its hint and tooltip then read that binding exactly like a command
    /// control, so a key is never advertised from the wrong scope.
    pub fn shortcut(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.button = self.button.disabled(disabled);
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        if active {
            self.button = self.button.primary();
        }
        self
    }

    /// Persistent on/off state uses shared selection and accessibility semantics.
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.button = self.button.selected(toggled).toggled(toggled);
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

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            button,
            label,
            command,
            shortcut,
            disabled,
        } = self;
        let shortcut = shortcut.or(command);
        // A disabled control cannot run its action, so it never advertises a key.
        let hint = match &shortcut {
            Some(shortcut) if !disabled => {
                Some(keytips::anchor(window, cx, id.clone(), shortcut.clone()))
            }
            _ => None,
        };
        let tooltip = match shortcut {
            Some(shortcut) => Hint::shortcut(label, shortcut),
            None => Hint::command(label, None),
        }
        .builder();
        // The hint's containing block is the button, not a stretched parent row.
        el! {
            .div { # id; relative; size: ICON_BUTTON_SIZE.px; flex_shrink_0; tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip } [
                button,
                @if (let Some(hint) = hint) hint, @end,
            ]
        }
    }
}
