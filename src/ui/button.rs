//! Shared icon controls with native focus, disabled state, and hover hints.
use super::Icon;
use crate::appearance::tokens::{APPLICATION_MENU_ICON_SIZE, ICON_BUTTON_SIZE, ICON_SIZE};
use anti_webui::prelude::*;
use gds_uim::el;
use std::rc::Rc;

enum Artwork {
    Icon(Icon),
    Dynamic(Rc<dyn Fn() -> Icon>),
    Application(Svg),
}

enum ButtonSize {
    Default,
    Small,
    ExtraSmall,
    Compact,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ButtonState {
    Idle,
    Active,
    Toggled,
}

pub struct IconButton {
    base: Div,
    icon: Artwork,
    label: Rc<dyn Fn() -> String>,
    disabled: bool,
    state: ButtonState,
    size: ButtonSize,
    keytip: Option<&'static str>,
}
impl IconButton {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            base: div(),
            icon: Artwork::Icon(Icon::More),
            label: {
                let id = id.into();
                Rc::new(move || id.clone())
            },
            disabled: false,
            state: ButtonState::Idle,
            size: ButtonSize::Default,
            keytip: None,
        }
    }
    pub fn application(id: impl Into<String>, artwork: Svg) -> Self {
        Self {
            icon: Artwork::Application(artwork),
            ..Self::new(id)
        }
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Artwork::Icon(icon);
        self
    }
    pub fn icon_fn(mut self, icon: impl Fn() -> Icon + 'static) -> Self {
        self.icon = Artwork::Dynamic(Rc::new(icon));
        self
    }
    pub fn compact(mut self) -> Self {
        self.size = ButtonSize::Compact;
        self
    }
    pub fn small(mut self) -> Self {
        self.size = ButtonSize::Small;
        self
    }
    pub fn extra_small(mut self) -> Self {
        self.size = ButtonSize::ExtraSmall;
        self
    }
    pub fn label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.label = Rc::new(move || label.clone());
        self
    }
    pub fn label_fn(mut self, label: impl Fn() -> String + 'static) -> Self {
        self.label = Rc::new(label);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.state = if active {
            ButtonState::Active
        } else {
            ButtonState::Idle
        };
        self
    }
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.state = if toggled {
            ButtonState::Toggled
        } else {
            ButtonState::Idle
        };
        self
    }
    pub fn keytip(mut self, stroke: &'static str) -> Self {
        self.keytip = Some(stroke);
        self
    }
    pub fn on_click(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.base = self.base.on_click(callback);
        self
    }
    pub fn on_focus_change(mut self, callback: impl Fn(bool) + 'static) -> Self {
        self.base = self.base.on_focus_change(callback);
        self
    }
}
impl IntoView for IconButton {
    fn into_view(self) -> View {
        let Self {
            base,
            icon,
            label,
            disabled,
            state,
            size,
            keytip,
        } = self;
        let accessible_label = label.clone();
        let focus_visible = signal(false);
        let hovered = signal(false);
        let (size, icon_size) = match size {
            ButtonSize::Default => (ICON_BUTTON_SIZE, ICON_SIZE),
            ButtonSize::Small => (24.0, 14.0),
            ButtonSize::ExtraSmall => (20.0, 12.0),
            ButtonSize::Compact => (18.0, 12.0),
        };
        let artwork = match icon {
            Artwork::Icon(icon) => icon.view(icon_size).into_view(),
            Artwork::Dynamic(icon) => dyn_view(move || icon().view(icon_size)).into_view(),
            Artwork::Application(artwork) => el! {
                .div.{ relative; size: APPLICATION_MENU_ICON_SIZE }.[
                    artwork.{
                        size: APPLICATION_MENU_ICON_SIZE;
                        dyn_style: move s => $(s.opacity(if hovered.get() { 0.0_f32 } else { 1.0_f32 }));
                    },
                    .div.{
                        absolute; inset: 0.0_f32; flex; items_center; justify_center;
                        dyn_style: move s => $(s.opacity(if hovered.get() { 1.0_f32 } else { 0.0_f32 }));
                    }.[ $(Icon::Menu.view(ICON_SIZE)) ],
                ]
            }.into_view(),
        };
        let button = el! { base.{
            relative; flex; items_center; justify_center; flex_none; size: size; rounded: 4.0_f32;
            cursor: CursorIcon::Pointer; focusable; select_none;
            on_focus_visible: move visible => focus_visible.{ set: visible };
            on_hover: move inside => hovered.{ set: inside };
            disabled: move () => disabled;
            access: move a => $(a.role(Role::Button).label(accessible_label()).selected(state != ButtonState::Idle));
            dyn_style: move s => ${
                let theme = theme();
                let background = match state {
                    _ if disabled => Color::TRANSPARENT,
                    ButtonState::Active if hovered.get() => theme.accent_hover,
                    ButtonState::Active => theme.accent,
                    ButtonState::Toggled => theme.selected,
                    ButtonState::Idle if hovered.get() => theme.hover,
                    ButtonState::Idle => Color::TRANSPARENT,
                };
                let foreground = match state {
                    _ if disabled => theme.text_disabled,
                    ButtonState::Active => theme.on_accent,
                    ButtonState::Toggled => crate::app::Gds::current().appearance().theme().colors().selection_foreground,
                    ButtonState::Idle => theme.text,
                };
                s.bg(background)
                    .text_color(foreground)
                    .outline(if focus_visible.get() { 1.0_f32 } else { 0.0_f32 })
                    .outline_offset(-1.0_f32)
                    .outline_color(theme.focus_ring)
            };
        }.[
            artwork,
            if !disabled [
                if let Some(stroke) = keytip [stroke.super::keytips::Anchor::new],
            ],
        ] };
        tooltip_with(button, TooltipOptions::new(), move |_| move || {
            let label = label.clone();
            el! { .div.{ max_w: 280.0_f32; px: 8.0_f32; py: 4.0_f32; rounded: 4.0_f32; text_size: 12.0_f32;
                dyn_style: s => $(s.bg(theme().tooltip).text_color(theme().on_tooltip));
            }.[ (move () => .label).text ] }
        }).into_view()
    }
}
