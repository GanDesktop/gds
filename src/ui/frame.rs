//! Shared geometry and materials for the window toolbar, workspace, and footer.
use super::container::container_traits;
use crate::appearance::tokens::{
    COLLAPSED_SIDEBAR_WIDTH, CONTENT_PADDING, FOOTER_HEIGHT, ICON_BUTTON_SIZE, SIDEBAR_PADDING,
    SIDEBAR_WIDTH, SMALL, TOOLBAR_HEIGHT, WINDOW_CORNER_RADIUS,
};
use anti_webui::prelude::*;
use gds_uim::el;

#[derive(Clone, Copy)]
enum SidebarWidth {
    Hidden,
    Fixed(f32),
    Reactive(Binding<f32>),
}
impl SidebarWidth {
    fn get(self) -> f32 {
        match self {
            Self::Hidden => 0.0,
            Self::Fixed(width) => width,
            Self::Reactive(width) => width.get(),
        }
    }
    fn visible(self) -> bool {
        !matches!(self, Self::Hidden)
    }
}
pub struct Frame {
    base: Div,
    sidebar: SidebarWidth,
    paint_workspace_content: bool,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            base: el! { .div.{ relative; flex; flex_col; size_full; min_w: 0.0_f32; min_h: 0.0_f32; overflow_hidden;
                dyn_style: s => ${
                    let gds = crate::app::Gds::current();
                    let rounded = gds.appearance().theme().rounded_corners()
                        && !window_maximized() && !window_fullscreen();
                    s.text_color(theme().text)
                        .font_family(gds.appearance().typography().family().as_str())
                        .text_size(gds.appearance().typography().scaled(SMALL))
                        .line_height(LineHeight::Relative(std::f32::consts::GOLDEN_RATIO))
                        .rounded(if rounded { WINDOW_CORNER_RADIUS } else { 0.0 })
                };
            } },
            sidebar: SidebarWidth::Fixed(SIDEBAR_WIDTH),
            paint_workspace_content: true,
        }
    }
}
container_traits!(Frame);
impl Frame {
    pub fn without_sidebar() -> Self {
        Self {
            sidebar: SidebarWidth::Hidden,
            ..Self::default()
        }
    }
    pub fn sidebar_width(mut self, width: f32) -> Self {
        self.sidebar = SidebarWidth::Fixed(width);
        self
    }
    pub fn sidebar_width_binding(mut self, width: impl Into<Binding<f32>>) -> Self {
        self.sidebar = SidebarWidth::Reactive(width.into());
        self
    }
    pub fn without_workspace_content_background(mut self) -> Self {
        self.paint_workspace_content = false;
        self
    }
    fn divider(&self) -> Option<Div> {
        let sidebar = self.sidebar;
        sidebar.visible().then(|| {
            el! {
                .div.{ absolute; top:0.0_f32; bottom:0.0_f32; w:1.0_f32;
                    dyn_style:move s=>$(s.left(sidebar.get() - 1.0).bg(theme().border));
                }
            }
        })
    }
    pub fn toolbar(&self, menu: impl IntoView, controls: impl IntoView) -> Div {
        let sidebar = self.sidebar;
        el! { .div.{ flex; items_center; flex_none; h: TOOLBAR_HEIGHT; w_full; relative }.[
            .div.{ flex; items_center; flex_none; h_full; pr: CONTENT_PADDING;
                dyn_style:move s=>${
                    let gds = crate::app::Gds::current();
                    let material = gds.appearance().theme();
                    let width = if sidebar.visible() { sidebar.get() } else { 2.0 * CONTENT_PADDING + ICON_BUTTON_SIZE };
                    let progress = ((width - COLLAPSED_SIDEBAR_WIDTH) / (SIDEBAR_WIDTH - COLLAPSED_SIDEBAR_WIDTH)).clamp(0.0, 1.0);
                    let inset = (COLLAPSED_SIDEBAR_WIDTH - ICON_BUTTON_SIZE) / 2.0;
                    s.w(width).pl(if sidebar.visible() { inset + (CONTENT_PADDING - inset) * progress } else { CONTENT_PADDING })
                        .bg(if sidebar.visible() { material.sidebar_background() } else { material.content_background() })
                };
            }.[.div.{size:ICON_BUTTON_SIZE;flex_none}.[menu], .WindowDragArea::default],
            .div.{ flex; flex_1; min_w: 0.0_f32; h_full; items_center; gap: 4.0_f32; px: SIDEBAR_PADDING;
                dyn_style: s => $(s.bg(crate::app::Gds::current().appearance().theme().content_background()));
            }.[ .div.{flex; flex_1; min_w:0.0_f32; h_full; items_center; overflow_hidden}.[controls], .WindowDragArea::default.{width:12.0_f32}, .super::window::WindowControls::default ],
            ..self.{ divider },
        ] }
    }
    pub fn workspace(&self, sidebar_view: impl IntoView, content: impl IntoView) -> Div {
        let sidebar = self.sidebar;
        el! { .div.{ relative; flex; flex_1; min_h: 0.0_f32; w_full }.[
            if sidebar.{ visible } [
                .div.{ h_full; flex_none; min_w: 0.0_f32; overflow_hidden;
                    dyn_style: move s => $(s.w(sidebar.get()).bg(crate::app::Gds::current().appearance().theme().sidebar_background()));
                }.[sidebar_view],
            ],
            .div.{ flex; flex_col; flex_1; min_w: 0.0_f32; min_h: 0.0_f32;
                if self->paint_workspace_content {
                    dyn_style: s => $(s.bg(crate::app::Gds::current().appearance().theme().content_background()));
                };
            }.[content],
            ..self.{ divider },
        ] }
    }
    pub fn footer(
        &self,
        sidebar_status: impl IntoView,
        status: impl IntoView,
        controls: impl IntoView,
    ) -> Div {
        let sidebar = self.sidebar;
        el! { .div.{ flex; items_center; flex_none; h: FOOTER_HEIGHT; w_full; relative;
            dyn_style: s => $(s.text_color(theme().text_muted));
        }.[
            if sidebar.{ visible } [
                .div.{ h_full; flex; items_center; flex_none; px: 12.0_f32;
                    dyn_style: move s => $(s.w(sidebar.get()).bg(crate::app::Gds::current().appearance().theme().sidebar_background()));
                }.[sidebar_status],
            ],
            .div.{ flex; flex_1; min_w: 0.0_f32; h_full; items_center; gap: 8.0_f32; px: 18.0_f32;
                dyn_style: s => $(s.bg(crate::app::Gds::current().appearance().theme().content_background()));
            }.[ .div.{ flex; items_center; gap:8.0_f32; flex_1; min_w: 0.0_f32 }.[status], controls ],
            ..self.{ divider },
            .div.{absolute; right:0.0_f32; top:0.0_f32; h:1.0_f32; dyn_style:move s=>$(s.left(sidebar.get()).bg(theme().border))},
        ] }
    }
}
#[derive(Default)]
pub struct WindowDragArea {
    width: Option<f32>,
}
impl WindowDragArea {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}
impl IntoView for WindowDragArea {
    fn into_view(self) -> View {
        let area = el! {
            .div.{
                min_w: 0.0_f32; h_full;
                on_pointer_down: event => ${
                    match event.button {
                        Some(PointerButton::Primary) if event.click_count == 2 => toggle_window_maximized(),
                        Some(PointerButton::Primary) => start_window_move(),
                        Some(PointerButton::Secondary) => show_window_menu(),
                        _ => {},
                    }
                };
            }
        };
        match self.width {
            Some(width) => area.w(width).flex_none(),
            None => area.flex_1(),
        }
        .into_view()
    }
}
