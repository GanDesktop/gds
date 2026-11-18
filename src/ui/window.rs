//! Native window controls, shown only when the client draws the decoration.
use super::{Icon, IconButton};
use anti_webui::prelude::*;
use gds_uim::el;

#[derive(Default)]
pub struct WindowControls;
impl WindowControls {
    fn hidden_in_current_session() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
        match desktop
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            Some(desktop) => desktop
                .split(':')
                .any(|name| name.trim().eq_ignore_ascii_case("niri")),
            None => std::env::var_os("NIRI_SOCKET").is_some_and(|socket| !socket.is_empty()),
        }
    }
}

/// Suite workspace dimensions. The compositor may allocate a different size.
pub fn workspace_options() -> WindowOptions {
    WindowOptions::new()
        .size(960, 640)
        .min_size(520, 360)
        .initial_size_fraction(0.85)
        .decorations(DecorationMode::Client)
        .background(crate::appearance::tokens::WINDOW_BACKGROUND)
}

impl IntoView for WindowControls {
    fn into_view(self) -> View {
        let hidden = Self::hidden_in_current_session();
        show(move || !hidden && window_decorations() == Decorations::Client, || el! {
            .div.{ flex; items_center; gap: 2.0_f32; pr: 8.0_f32 }.[
                "minimize".IconButton::new.{ icon: Icon::Minimize; label_fn: () => "window-minimize".crate::i18n::text; on_click: _ => $(minimize_window()) },
                "maximize".IconButton::new.{ icon: Icon::Maximize; label_fn: () => $(crate::i18n::text(if window_maximized() { "window-restore" } else { "window-maximize" })); on_click: _ => $(toggle_window_maximized()) },
                "close".IconButton::new.{ icon: Icon::Close; label_fn: () => "window-close".crate::i18n::text; on_click: _ => $(request_close_window()) },
            ]
        }).into_view()
    }
}
