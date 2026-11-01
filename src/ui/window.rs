//! Suite window defaults, titlebars, and platform-aware window controls.

use std::{cell::Cell, rc::Rc};

use gpui_kit::{
    AnyElement, App, ClickEvent, Decorations, IntoElement, MouseButton, ParentElement, RenderOnce,
    SharedString, Styled, TestSupportExt, Window, WindowControlArea, WindowDecorations,
    WindowOptions,
    assets::IconName,
    component::{ActiveTheme, Icon, InteractiveElementExt, Sizable, button::ButtonVariants},
    div,
    prelude::*,
    px,
};

pub const HEIGHT: f32 = crate::appearance::tokens::TOOLBAR_HEIGHT;
pub const MACOS_CONTROLS_WIDTH: f32 = 80.;
type Handler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Defaults for windows that draw the GDS toolbar or titlebar. macOS keeps its
/// native traffic lights; Linux and Windows share the custom control component.
pub fn window_options() -> WindowOptions {
    WindowOptions {
        titlebar: cfg!(target_os = "macos").then(gpui_kit::component::TitleBar::title_bar_options),
        app_owns_titlebar_drag: cfg!(target_os = "macos"),
        window_decorations: Some(WindowDecorations::Client),
        window_background: crate::appearance::tokens::WINDOW_BACKGROUND,
        ..Default::default()
    }
}

#[derive(Default, IntoElement)]
pub struct WindowControls {
    maximize: Option<Handler>,
    close: Option<Handler>,
}

impl WindowControls {
    pub fn on_maximize(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.maximize = Some(Box::new(handler));
        self
    }

    pub fn on_close_window(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.close = Some(Box::new(handler));
        self
    }

    fn session_hides_controls(desktop: Option<&str>, has_niri_socket: bool) -> bool {
        match desktop.map(str::trim).filter(|desktop| !desktop.is_empty()) {
            Some(desktop) => desktop
                .split(':')
                .any(|name| name.trim().eq_ignore_ascii_case("niri")),
            None => has_niri_socket,
        }
    }

    fn hidden_in_current_session() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
        let has_niri_socket =
            std::env::var_os("NIRI_SOCKET").is_some_and(|socket| !socket.is_empty());
        Self::session_hides_controls(desktop.as_deref(), has_niri_socket)
    }
}

fn control(
    id: &'static str,
    icon: IconName,
    hint: SharedString,
    area: WindowControlArea,
) -> super::button::Button {
    super::button::Button::new(id)
        .ghost()
        .xsmall()
        .w(px(crate::appearance::tokens::ICON_BUTTON_SIZE))
        .h(px(crate::appearance::tokens::ICON_BUTTON_SIZE))
        .p_0()
        .flex_shrink_0()
        .child(
            div()
                .id(gpui_kit::SharedString::from(format!("{id}-icon")))
                .test_support()
                .flex()
                .child(Icon::new(icon).with_size(px(crate::appearance::tokens::SMALL_ICON_SIZE))),
        )
        .accessibility_label(hint.clone())
        .tooltip(hint)
        .when(cfg!(target_os = "windows"), |button| {
            button.window_control_area(area)
        })
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
}

impl RenderOnce for WindowControls {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut controls = div()
            .id("window-controls")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0();
        if cfg!(target_os = "macos") || cfg!(target_family = "wasm") {
            return controls.hidden();
        }
        // A compositor may grant server decorations even when the application
        // requested client decorations. It already supplies min/max/close.
        if cfg!(target_os = "linux")
            && (Self::hidden_in_current_session()
                || !matches!(window.window_decorations(), Decorations::Client { .. }))
        {
            return controls.hidden();
        }
        let supported = window.window_controls();
        if supported.minimize {
            controls = controls.child(
                control(
                    "window-minimize",
                    IconName::WindowMinimize,
                    crate::i18n::text("window-minimize", cx),
                    WindowControlArea::Min,
                )
                .on_click(|_, window, cx| {
                    cx.stop_propagation();
                    window.minimize_window();
                }),
            );
        }
        if supported.maximize {
            let (icon, hint) = if window.is_maximized() {
                (
                    IconName::WindowRestore,
                    crate::i18n::text("window-restore", cx),
                )
            } else {
                (
                    IconName::WindowMaximize,
                    crate::i18n::text("window-maximize", cx),
                )
            };
            controls = controls.child(
                control("window-maximize", icon, hint.into(), WindowControlArea::Max).on_click(
                    move |event, window, cx| {
                        cx.stop_propagation();
                        if let Some(handler) = &self.maximize {
                            handler(event, window, cx);
                        } else {
                            window.zoom_window();
                        }
                    },
                ),
            );
        }
        controls.child(
            control(
                "window-close",
                IconName::WindowClose,
                crate::i18n::text("window-close", cx),
                WindowControlArea::Close,
            )
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                if let Some(handler) = &self.close {
                    handler(event, window, cx);
                } else {
                    window.remove_window();
                }
            }),
        )
    }
}

#[cfg(test)]
mod tests;

#[derive(Default, IntoElement)]
pub struct TitleBar {
    children: Vec<AnyElement>,
    controls: WindowControls,
}

impl TitleBar {
    pub fn window_options() -> WindowOptions {
        window_options()
    }

    pub fn on_close_window(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.controls = self.controls.on_close_window(handler);
        self
    }
}

impl ParentElement for TitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let dragging = window
            .use_state(cx, |_, _| Rc::new(Cell::new(false)))
            .read(cx)
            .clone();
        let down = dragging.clone();
        let up = dragging.clone();
        let outside = dragging.clone();
        let mut title = div()
            .id("title-bar-content")
            .flex()
            .items_center()
            .flex_1()
            .h_full()
            .text_size(crate::appearance::tokens::SMALL)
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, move |_, _, _| down.set(true))
            .on_mouse_up(MouseButton::Left, move |_, _, _| up.set(false))
            .on_mouse_down_out(move |_, _, _| outside.set(false))
            .on_mouse_move(move |_, window, _| {
                if dragging.replace(false) {
                    window.start_window_move();
                }
            })
            .on_double_click(|_, window, _| {
                if cfg!(target_os = "macos") {
                    window.titlebar_double_click();
                } else if window.window_controls().maximize {
                    window.zoom_window();
                }
            });
        if cfg!(target_os = "linux")
            && matches!(window.window_decorations(), Decorations::Client { .. })
        {
            title = title.on_mouse_down(MouseButton::Right, |event, window, _| {
                window.show_window_menu(event.position)
            });
        }
        div()
            .id("title-bar")
            .flex()
            .items_center()
            .gap_1()
            .h(px(HEIGHT))
            .flex_shrink_0()
            .px_3()
            .when(
                cfg!(target_os = "macos") && !window.is_fullscreen(),
                |bar| bar.pl(px(MACOS_CONTROLS_WIDTH)),
            )
            .bg(crate::appearance::theme::sidebar_background(cx))
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(title.children(self.children))
            .child(self.controls)
    }
}
