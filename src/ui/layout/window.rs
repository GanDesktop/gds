//! Suite window defaults, titlebars, and platform-aware window controls.

mod drag;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use drag::{DragAction, record_drag_actions};
pub(crate) use drag::{attach_window_drag, inside_window_drag_scope, provide_window_drag_scope};

use zgui::geom::{Css, CssPx, Point, Size};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback};
use zgui::view::Scoped;
use zgui::{component, css, view};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipTriggerProps};

use crate::app::Gds;
use crate::appearance::tokens;
use crate::ui::button::IconButtonProps;
use crate::ui::icon::IconName;

pub const HEIGHT: f32 = tokens::TOOLBAR_HEIGHT;
pub const MACOS_CONTROLS_WIDTH: f32 = 80.;
pub const INITIAL_WIDTH: f32 = 1240.0;
pub const INITIAL_HEIGHT: f32 = 760.0;
const DISPLAY_FRACTION: f32 = 0.85;
pub const MIN_WIDTH: f32 = 460.0;
pub const MIN_HEIGHT: f32 = 360.0;

pub const MICRO_WIDTH: f32 = 600.0;
pub const WIDE_WIDTH: f32 = 1200.0;
pub const MICRO_HEIGHT: f32 = 520.0;

/// Orthogonal horizontal responsive tier for GDS workspace shells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidthTier {
    Micro,
    Compact,
    Wide,
}

impl WidthTier {
    #[must_use]
    pub fn from_width(width: f32) -> Self {
        if width < MICRO_WIDTH {
            Self::Micro
        } else if width < WIDE_WIDTH {
            Self::Compact
        } else {
            Self::Wide
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Micro => "micro",
            Self::Compact => "compact",
            Self::Wide => "wide",
        }
    }
}

/// Orthogonal vertical responsive tier for GDS workspace shells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightTier {
    Micro,
    Normal,
}

impl HeightTier {
    #[must_use]
    pub fn from_height(height: f32) -> Self {
        if height < MICRO_HEIGHT {
            Self::Micro
        } else {
            Self::Normal
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Micro => "micro",
            Self::Normal => "normal",
        }
    }
}

const WINDOW_SHEET: &str = "gds-ui-window";

const WINDOW_CSS: &str = css!(
    ".gds-window-controls {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        flex-shrink: 0;
    }
    .gds-window-controls__button .gds-icon {
        width: var(--gds-small-icon-size, 12px);
        height: var(--gds-small-icon-size, 12px);
    }
    .gds-window-controls__close:hover {
        background-color: var(--gds-error);
        color: var(--gds-error-foreground);
    }
    .gds-titlebar {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        height: var(--gds-toolbar-height, 48px);
        flex-shrink: 0;
        padding: 0 12px;
        background-color: var(--gds-sidebar-bg, var(--gds-sidebar));
        border-bottom: 1px solid var(--gds-border);
    }
    .gds-titlebar__slot {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        flex-shrink: 0;
    }
    .gds-titlebar__title {
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }"
);

fn install_window_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(WINDOW_SHEET, WINDOW_CSS);
}

#[must_use]
pub fn minimum_size() -> Size<CssPx, Css> {
    Size::new(CssPx(MIN_WIDTH), CssPx(MIN_HEIGHT))
}

#[must_use]
pub fn initial_size() -> Size<CssPx, Css> {
    Size::new(CssPx(INITIAL_WIDTH), CssPx(INITIAL_HEIGHT))
}

/// Clamp `requested` within `DISPLAY_FRACTION` of the primary monitor when available.
#[must_use]
pub fn fit_size(requested: Size<CssPx, Css>) -> Size<CssPx, Css> {
    if !zgui::reactive::is_ui_thread() {
        return requested;
    }
    let Some(monitors) = zgui::prelude::try_use_monitors() else {
        return requested;
    };
    let list = monitors.get_untracked();
    let Some(primary) = list.first() else {
        return requested;
    };
    let (avail_w, avail_h) = if let Some(logical) = primary.logical {
        (logical.size.width.0, logical.size.height.0)
    } else {
        let scale = primary
            .fractional_scale
            .unwrap_or(primary.scale_factor)
            .max(1.0) as f32;
        (primary.size.width.0 / scale, primary.size.height.0 / scale)
    };
    if avail_w <= 0.0 || avail_h <= 0.0 {
        return requested;
    }
    Size::new(
        CssPx(requested.width.0.min(avail_w * DISPLAY_FRACTION)),
        CssPx(requested.height.0.min(avail_h * DISPLAY_FRACTION)),
    )
}

/// Defaults for GDS client-side-decorated transparent workspace windows.
#[must_use]
pub fn workspace_options(title: impl Into<String>, app_id: Option<&str>) -> WindowOptions {
    let size = fit_size(initial_size());
    let min = minimum_size();
    let mut options = WindowOptions::new(title.into())
        .with_size(size.width.0, size.height.0)
        .with_min_size(min.width.0, min.height.0)
        .with_decorations(Decorations::None)
        .with_transparent(true);
    if let Some(id) = app_id {
        options = options.with_application_id(id);
    }
    options
}

/// Compatibility alias matching legacy `window_options()` callers.
#[must_use]
pub fn window_options() -> WindowOptions {
    workspace_options("", None)
}

/// Parents `window` under an external desktop handle (`"wayland:<token>"` or `"x11:<xid>"`).
pub fn set_external_parent(window: &WindowHandle, parent: &str, modal: bool) {
    if parent.is_empty() {
        return;
    }
    if let Err(error) = window.set_external_parent(parent, modal) {
        tracing::debug!(%parent, modal, %error, "failed to apply external window parent");
    }
}

/// Passes an external activation token (`xdg-activation-v1` / desktop startup ID) to `window`.
pub fn activate_with_token(window: &WindowHandle, token: &str) {
    if !token.is_empty() {
        window.activate_with_token(token);
    }
}

/// Opens the desktop's native window menu at window-local `pos`, in CSS pixels.
pub fn show_window_menu(window: &WindowHandle, pos: Point<CssPx, Css>) {
    window.show_window_menu(pos);
}

pub(crate) fn session_hides_controls(desktop: Option<&str>, has_niri_socket: bool) -> bool {
    match desktop.map(str::trim).filter(|name| !name.is_empty()) {
        Some(desktop) => desktop
            .split(':')
            .any(|name| name.trim().eq_ignore_ascii_case("niri")),
        None => has_niri_socket,
    }
}

fn hidden_in_current_session() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let has_niri_socket = std::env::var_os("NIRI_SOCKET").is_some_and(|socket| !socket.is_empty());
    session_hides_controls(desktop.as_deref(), has_niri_socket)
}

fn localized_label(key: &str, fallback: &str) -> String {
    Gds::try_current().map_or_else(|| fallback.to_owned(), |g| g.i18n().text(key))
}

fn control_button(
    icon: Signal<IconName, LocalStorage>,
    label: Signal<String, LocalStorage>,
    extra_class: &'static str,
    window: Option<WindowHandle>,
    on_activate: impl Fn(Option<&WindowHandle>) + 'static,
) -> AnyView {
    AnyView::new(view! {
        Tooltip(delay = crate::appearance::tokens::TOOLTIP_DELAY) {
            TooltipTrigger {
                IconButton(
                    icon = icon,
                    label = label,
                    class = extra_class,
                    a11y:role = Role::Button,
                    a11y:label = move || label.get(),
                    on:click = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                        ev.stop_propagation();
                        on_activate(window.as_ref());
                    }
                )
            }
            TooltipContent {{move || label.get()}}
        }
    })
}

fn render_window_controls(
    class: Classes,
    hide_session: bool,
    on_maximize: Option<UnsyncCallback<()>>,
    on_close_window: Option<UnsyncCallback<()>>,
) -> AnyView {
    install_window_stylesheets();
    if cfg!(target_os = "macos")
        || cfg!(target_family = "wasm")
        || (cfg!(target_os = "linux") && hide_session)
    {
        return AnyView::new(());
    }
    let window = zgui::prelude::try_use_window();
    let visible = Signal::derive_local({
        let window = window.clone();
        move || !window.as_ref().is_some_and(|w| w.server_decorated().get())
    });
    let can_minimize = Signal::derive_local({
        let window = window.clone();
        move || window.as_ref().is_none_or(|w| w.can_minimize().get())
    });
    let can_maximize = Signal::derive_local({
        let window = window.clone();
        move || window.as_ref().is_none_or(|w| w.can_maximize().get())
    });
    let max_icon = Signal::derive_local({
        let window = window.clone();
        move || {
            if window.as_ref().is_some_and(|w| w.maximized().get()) {
                IconName::WindowRestore
            } else {
                IconName::WindowMaximize
            }
        }
    });
    let max_label = Signal::derive_local({
        let window = window.clone();
        move || {
            if window.as_ref().is_some_and(|w| w.maximized().get()) {
                localized_label("window-restore", "Restore window")
            } else {
                localized_label("window-maximize", "Maximize window")
            }
        }
    });
    let min_label = Signal::derive_local(|| localized_label("window-minimize", "Minimize window"));
    let close_label = Signal::derive_local(|| localized_label("window-close", "Close window"));

    let min_window = window.clone();
    let max_window = window.clone();
    let close_window = window;

    AnyView::new(view! {
        Show(when = move || visible.get()) {
            {{
                let min_win = min_window.clone();
                let max_win = max_window.clone();
                let close_win = close_window.clone();
                view! {
                    row(class = "gds-window-controls", class = class.clone()) {
                        Show(when = move || can_minimize.get()) {
                            {{control_button(
                                Signal::stored_local(IconName::WindowMinimize),
                                min_label,
                                "gds-window-controls__button",
                                min_win.clone(),
                                |win| {
                                    if let Some(w) = win {
                                        w.minimize();
                                    }
                                },
                            )}}
                        }
                        Show(when = move || can_maximize.get()) {
                            {{
                                let on_max = on_maximize;
                                control_button(
                                    max_icon,
                                    max_label,
                                    "gds-window-controls__button",
                                    max_win.clone(),
                                    move |win| {
                                        if let Some(ref cb) = on_max {
                                            cb.run(());
                                        } else if let Some(w) = win {
                                            w.toggle_maximized();
                                        }
                                    },
                                )
                            }}
                        }
                        {{
                            let on_close = on_close_window;
                            control_button(
                                Signal::stored_local(IconName::WindowClose),
                                close_label,
                                "gds-window-controls__button gds-window-controls__close",
                                close_win,
                                move |win| {
                                    if let Some(ref cb) = on_close {
                                        cb.run(());
                                    } else if let Some(w) = win {
                                        w.close();
                                    }
                                },
                            )
                        }}
                    }
                }
            }}
        }
    })
}

/// Client-side window control buttons (minimize, maximize/restore, close), suppressed under Niri
/// and server-decorated windows.
#[component]
pub fn WindowControls(
    #[prop(optional)] on_maximize: Option<UnsyncCallback<()>>,
    #[prop(optional)] on_close_window: Option<UnsyncCallback<()>>,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    render_window_controls(
        class,
        hidden_in_current_session(),
        on_maximize,
        on_close_window,
    )
}

fn render_title_bar(
    title: Signal<String, LocalStorage>,
    leading: Option<Children>,
    trailing: Option<Children>,
    on_maximize: Option<UnsyncCallback<()>>,
    on_close_window: Option<UnsyncCallback<()>>,
    class: Classes,
    hide_controls: bool,
) -> impl IntoView {
    Scoped::new(move || {
        if let Some(gds) = Gds::try_current() {
            let _ = gds.appearance().install_window_sheet();
        }
        install_window_stylesheets();
        provide_window_drag_scope();
        let bar_ref = NodeRef::new();
        let window = zgui::prelude::try_use_window();
        let drag_handler = attach_window_drag(bar_ref, window);

        view! {
            row(
                node_ref = bar_ref,
                class = "gds-titlebar gds-toolbar",
                class = class,
                on:pointer_down = drag_handler
            ) {
                {leading.map(|slot| AnyView::new(view! {
                    row(class = "gds-titlebar__slot") {
                        {slot.into_view_once()}
                    }
                }))}
                text(class = "gds-titlebar__title") {{move || title.get()}}
                spacer() {}
                {trailing.map(|slot| AnyView::new(view! {
                    row(class = "gds-titlebar__slot") {
                        {slot.into_view_once()}
                    }
                }))}
                {render_window_controls(Classes::default(), hide_controls, on_maximize, on_close_window)}
            }
        }
    })
}

#[cfg(test)]
pub(crate) fn title_bar_with_forced_controls(
    title: &str,
    on_maximize: Option<UnsyncCallback<()>>,
    on_close_window: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    render_title_bar(
        Signal::stored_local(title.to_owned()),
        None,
        None,
        on_maximize,
        on_close_window,
        Classes::default(),
        false,
    )
}

/// Draggable titlebar strip with optional leading/trailing slots and window menu on secondary click.
#[component]
pub fn TitleBar(
    #[prop(into)] title: Signal<String, LocalStorage>,
    #[prop(optional)] leading: Option<Children>,
    #[prop(optional)] trailing: Option<Children>,
    #[prop(optional)] on_maximize: Option<UnsyncCallback<()>>,
    #[prop(optional)] on_close_window: Option<UnsyncCallback<()>>,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    render_title_bar(
        title,
        leading,
        trailing,
        on_maximize,
        on_close_window,
        class,
        hidden_in_current_session(),
    )
}
