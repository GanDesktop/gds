//! Shared menu appearance, driven by the toolkit's native popup and menu state.
use super::Icon;
use anti_webui::{
    prelude::*,
    widgets::{MenuChoice, MenuState},
};
use gds_uim::el;
use std::rc::Rc;

pub use anti_webui::widgets::{MenuOptions, MenuParts, menu_with};

#[derive(Clone)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
    Submenu(String, Rc<Vec<MenuEntry>>),
}

#[derive(Clone)]
pub struct MenuItem {
    label: String,
    icon: Option<Icon>,
    shortcut: Option<String>,
    checked: Option<Rc<dyn Fn() -> bool>>,
    disabled: Option<Rc<dyn Fn() -> bool>>,
    action: Option<Rc<dyn Fn()>>,
}

pub fn menu_item(label: impl Into<String>) -> MenuItem {
    MenuItem {
        label: label.into(),
        icon: None,
        shortcut: None,
        checked: None,
        disabled: None,
        action: None,
    }
}

pub const fn menu_separator() -> MenuEntry {
    MenuEntry::Separator
}

impl MenuItem {
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }
    pub fn checked_when(mut self, checked: impl Fn() -> bool + 'static) -> Self {
        self.checked = Some(Rc::new(checked));
        self
    }
    pub fn disabled_when(mut self, disabled: impl Fn() -> bool + 'static) -> Self {
        self.disabled = Some(Rc::new(disabled));
        self
    }
    pub fn on_click(mut self, action: impl Fn() + 'static) -> Self {
        self.action = Some(Rc::new(action));
        self
    }
}

impl From<MenuItem> for MenuEntry {
    fn from(item: MenuItem) -> Self {
        Self::Item(item)
    }
}

fn frame(state: MenuState, index: usize, disabled: impl Fn() -> bool + 'static) -> Div {
    el! { .div.{flex; items_center; gap: 8.0_f32; px: 8.0_f32; min_h: 28.0_f32;
        rounded: 4.0_f32; select_none;
        dyn_style: move style => ${
            let colors = theme();
            if disabled() {
                style.text_color(colors.text_disabled)
            } else if state.is_lit(index) {
                style.bg(colors.hover).text_color(colors.text).cursor(CursorIcon::Pointer)
            } else {
                style.text_color(colors.text).cursor(CursorIcon::Pointer)
            }
        };
    } }
}

fn row(state: MenuState, index: usize, entry: &MenuEntry) -> View {
    match entry {
        MenuEntry::Item(item) => {
            let checked = item.checked.clone();
            let checked_view = checked.clone();
            let disabled = item.disabled.clone();
            let disabled_style = disabled.clone();
            let action = item.action.clone();
            let icon = item.icon;
            let shortcut = item.shortcut.clone();
            let label = item.label.clone();
            let choice = MenuChoice::new(move || {
                if let Some(action) = action.as_ref() {
                    action();
                }
            })
            .disabled_when(move || disabled.as_ref().is_some_and(|get| get()));
            let base = frame(state, index, move || {
                disabled_style.as_ref().is_some_and(|get| get())
            });
            let contents = el! { base.{
                access: move a => $(if let Some(checked) = &checked {
                    a.role(Role::MenuItemCheckbox).checked(checked())
                } else { a });
            }.[
                .div.{flex_none; w: 16.0_f32; access: a => $(a.hidden())}.[
                    if let Some(checked) = checked_view [
                        $(move || {
                            if checked() { Icon::Check.view(16.0_f32).into_view() } else { div().into_view() }
                        }).dyn_view,
                    ] else if let Some(icon) = icon [$(icon.view(16.0_f32))],
                ],
                label.text.{flex_1; min_w: 0.0_f32; ellipsis; whitespace_nowrap},
                if let Some(shortcut) = shortcut [
                    $(shortcut_badge(&shortcut)),
                ],
            ] };
            state.item(index, contents, choice).into_view()
        }
        MenuEntry::Separator => el! { .div.{h: 1.0_f32; my: 4.0_f32; mx: 4.0_f32; flex_none;
            dyn_style: style => $(style.bg(theme().border));
        } }
        .into_view(),
        MenuEntry::Submenu(label, entries) => {
            let base = frame(state, index, || false);
            let label = label.clone();
            let entries = entries.clone();
            let contents = el! { base.[
                .div.{w: 16.0_f32; flex_none},
                label.text.{flex_1; min_w: 0.0_f32; ellipsis; whitespace_nowrap},
                $(Icon::ChevronRight.view(14.0_f32)),
            ] };
            state
                .submenu(index, contents, move |state| surface(entries, state))
                .into_view()
        }
    }
}

fn shortcut_badge(shortcut: &str) -> Div {
    let keys = shortcut.split(if shortcut.contains('+') { '+' } else { '-' }).map(|key| {
        let label = match key.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".into(),
            "shift" => "Shift".into(),
            "alt" => "Alt".into(),
            "super" | "logo" => "Super".into(),
            "enter" => "Enter".into(),
            "escape" => "Esc".into(),
            "left" => "←".into(),
            "right" => "→".into(),
            "up" => "↑".into(),
            "down" => "↓".into(),
            _ => key.to_uppercase(),
        };
        el! { .div.{ flex_none; flex; items_center; justify_center; h: 22.0_f32; min_w: 22.0_f32;
            px: 6.0_f32; rounded: 4.0_f32; border: 1.0_f32;
            dyn_style: style => ${
                let colors = theme();
                style.border_color(colors.border).bg(colors.surface_muted).text_color(colors.text_muted)
                    .text_size(crate::app::Gds::current().appearance().typography().scaled(12.0_f32))
            };
        }.[ label.text ] }
    });
    el! { .div.{ flex; flex_none; ml: 16.0_f32; items_center; gap: 4.0_f32 }.[..keys] }
}

fn surface(entries: Rc<Vec<MenuEntry>>, state: MenuState) -> Box<dyn Fn() -> Div> {
    let opacity = follow(
        move || if state.open() { 1.0_f32 } else { 0.0_f32 },
        Transition::ms(140).ease_out(),
    );
    let min_width = if entries
        .iter()
        .any(|entry| matches!(entry, MenuEntry::Item(item) if item.shortcut.is_some()))
    {
        300.0_f32
    } else {
        220.0_f32
    };
    Box::new(move || {
        let rows = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| row(state, index, entry));
        el! { .div.{flex; flex_col; min_w: min_width; max_w: 500.0_f32; max_h: 480.0_f32;
            p: 4.0_f32; rounded: 8.0_f32; border: 1.0_f32; shadow_lg; overflow_y_scroll;
            dyn_style: move style => ${ let colors = theme();
                style.bg(colors.overlay).border_color(colors.border).text_color(colors.text)
                    .opacity(opacity.get().clamp(0.0_f32, 1.0_f32))
            };
        }.[..rows] }
    })
}

pub fn menu(
    open: impl Fn() -> bool + 'static,
    dismiss: impl Fn() + 'static,
    entries: Vec<MenuEntry>,
) -> Show {
    menu_with_options(open, dismiss, MenuOptions::new(), entries)
}

pub fn menu_with_options(
    open: impl Fn() -> bool + 'static,
    dismiss: impl Fn() + 'static,
    options: MenuOptions,
    entries: Vec<MenuEntry>,
) -> Show {
    menu_with(open, dismiss, options, |state| {
        surface(Rc::new(entries), state)
    })
}

pub fn context_menu(target: Div, entries: Vec<MenuEntry>) -> Div {
    let at = signal(None::<RectI>);
    el! { target.{
        on_pointer_down: move event => ${
            if event.button == Some(PointerButton::Secondary) {
                event.stop_propagation();
                at.set(Some(RectI::new(event.position.x as i32, event.position.y as i32, 1, 1)));
            }
        };
    }.[ $(menu_with_options(move || at.with(Option::is_some), move || at.set(None), MenuOptions::new().at(at), entries)) ] }
}

pub fn dropdown_button(label: impl IntoTextSource, entries: Vec<MenuEntry>) -> Div {
    let open = signal(false);
    el! { label.button.{
        on_click: move _ => $(open.update(|value| *value = !*value));
        on_key_down: move event => ${
            if event.key == Key::Named(NamedKey::ArrowDown) && !open.get_untracked() {
                event.prevent_default();
                open.set(true);
            }
        };
    }.[ $(Icon::ChevronDown.view(14.0_f32)), $(menu(move || open.get(), move || open.set(false), entries)) ] }
}
