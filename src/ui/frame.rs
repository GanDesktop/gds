//! Shared GDS outer CSD window frame, single-column split toolbar, workspace, and footer.

mod drag;
mod footer;
mod toolbar;
mod workspace;

pub use drag::{WindowDragArea, WindowDragAreaProps};
pub use footer::{Footer, Footer as FrameFooter, FooterProps, FooterProps as FrameFooterProps};
pub use toolbar::{
    Toolbar, Toolbar as FrameToolbar, ToolbarProps, ToolbarProps as FrameToolbarProps,
};
pub use workspace::{
    WorkspaceFrame, WorkspaceFrame as FrameWorkspace, WorkspaceFrame as Workspace,
    WorkspaceFrameProps, WorkspaceFrameProps as FrameWorkspaceProps,
    WorkspaceFrameProps as WorkspaceProps,
};

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, provide_local_context, use_local_context};
use zgui::{component, css, view};

use crate::app::Gds;
use crate::appearance::tokens;
use crate::ui::feedback::tooltip::GdsTooltipProviderProps;

pub const TOOLBAR_HEIGHT: f32 = tokens::TOOLBAR_HEIGHT;
pub const FOOTER_HEIGHT: f32 = tokens::FOOTER_HEIGHT;
pub(super) const MENU_PADDING: f32 = tokens::CONTENT_PADDING;
pub(super) const STATUS_PADDING: f32 = 18.0;
pub(super) const DIVIDER_WIDTH: f32 = 1.0;

/// Shared single-column boundary propagated from [`Frame`] to [`Toolbar`], [`WorkspaceFrame`], and [`Footer`].
#[derive(Clone, Copy)]
pub struct FrameContext {
    pub sidebar_width: Option<Signal<f32, LocalStorage>>,
}

impl FrameContext {
    #[must_use]
    pub fn current() -> Option<Self> {
        zgui::reactive::is_ui_thread()
            .then(use_local_context::<Self>)
            .flatten()
    }
}

const FRAME_SHEET: &str = "gds-ui-frame";

const FRAME_CSS: &str = css!(
    ".gds-frame {
        position: relative;
        background-color: transparent;
    }
    .gds-frame[data-maximized=\"true\"],
    .gds-frame[data-tiled=\"true\"],
    .gds-frame[data-server-decorated=\"true\"] {
        border-radius: 0px;
        border-width: 0px;
    }
    .gds-frame__grip {
        position: absolute;
        z-index: 30;
    }
    .gds-frame__grip--n { top: 0px; left: 6px; right: 6px; height: 5px; }
    .gds-frame__grip--s { bottom: 0px; left: 6px; right: 6px; height: 5px; }
    .gds-frame__grip--w { left: 0px; top: 6px; bottom: 6px; width: 5px; }
    .gds-frame__grip--e { right: 0px; top: 6px; bottom: 6px; width: 5px; }
    .gds-frame__grip--nw { top: 0px; left: 0px; width: 10px; height: 10px; }
    .gds-frame__grip--ne { top: 0px; right: 0px; width: 10px; height: 10px; }
    .gds-frame__grip--sw { bottom: 0px; left: 0px; width: 10px; height: 10px; }
    .gds-frame__grip--se { bottom: 0px; right: 0px; width: 10px; height: 10px; }
    .gds-frame__v-divider {
        position: absolute;
        top: 0px;
        bottom: 0px;
        width: 1px;
        background-color: var(--gds-border);
        pointer-events: none;
        z-index: 5;
    }
    .gds-window-drag-area {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0px;
        min-width: 0px;
        align-self: stretch;
    }
    .gds-toolbar {
        position: relative;
        width: 100%;
        min-width: 0px;
        background-color: transparent;
    }
    .gds-toolbar--legacy {
        padding: 0px 8px;
        gap: 6px;
        background-color: var(--gds-content-bg, var(--gds-content));
    }
    .gds-toolbar__drag {
        position: absolute;
        inset: 0px;
        pointer-events: none;
    }
    .gds-toolbar__left {
        display: flex;
        flex-direction: row;
        align-items: center;
        height: 100%;
        flex-shrink: 0;
        min-width: 0px;
        box-sizing: border-box;
    }
    .gds-toolbar__left--sidebar {
        background-color: var(--gds-sidebar-bg, var(--gds-sidebar));
        color: var(--gds-sidebar-foreground);
    }
    .gds-toolbar__left--content {
        background-color: var(--gds-content-bg, var(--gds-content));
    }
    .gds-toolbar__menu {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
    }
    .gds-toolbar__right {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0px;
        min-width: 0px;
        align-self: stretch;
        padding: 0px var(--gds-sidebar-padding, 10px);
        gap: 4px;
        background-color: var(--gds-content-bg, var(--gds-content));
        box-sizing: border-box;
    }
    .gds-toolbar__controls {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0px;
        min-width: 0px;
        align-self: stretch;
        overflow: hidden;
    }
    .gds-workspace-frame {
        position: relative;
        display: flex;
        flex-direction: row;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }
    .gds-workspace-frame--legacy[data-paint-surface=\"true\"] {
        background-color: var(--gds-content-bg, var(--gds-content));
    }
    .gds-workspace-frame__sidebar {
        display: flex;
        flex-direction: column;
        align-self: stretch;
        min-width: 0px;
        min-height: 0px;
        flex-shrink: 0;
        overflow: hidden;
        background-color: var(--gds-sidebar-bg, var(--gds-sidebar));
        color: var(--gds-sidebar-foreground);
    }
    .gds-workspace-frame__content {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        align-self: stretch;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
        background-color: transparent;
    }
    .gds-workspace-frame__content[data-paint-surface=\"true\"] {
        background-color: var(--gds-content-bg, var(--gds-content));
    }
    .gds-footer {
        position: relative;
        width: 100%;
        min-width: 0px;
        color: var(--gds-foreground);
        font-size: var(--gds-font-size-xs, 12px);
        background-color: transparent;
    }
    .gds-footer--legacy {
        padding: 0px 18px;
        gap: 8px;
        background-color: var(--gds-content-bg, var(--gds-content));
        border-top: 1px solid var(--gds-border);
    }
    .gds-footer__left {
        display: flex;
        flex-direction: row;
        align-items: center;
        height: 100%;
        flex-shrink: 0;
        min-width: 0px;
        padding: 0px var(--gds-sidebar-padding, 10px);
        box-sizing: border-box;
        background-color: var(--gds-sidebar-bg, var(--gds-sidebar));
        color: var(--gds-sidebar-foreground);
    }
    .gds-footer__right {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0px;
        min-width: 0px;
        align-self: stretch;
        padding: 0px 18px;
        gap: 8px;
        box-sizing: border-box;
        background-color: var(--gds-content-bg, var(--gds-content));
        border-top: 1px solid var(--gds-border);
    }
    .gds-footer__status {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0px;
        min-width: 0px;
        gap: 8px;
        overflow: hidden;
    }
    .gds-footer__controls {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        gap: 8px;
    }"
);

pub(super) fn install_frame_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(FRAME_SHEET, FRAME_CSS);
}

pub(super) fn vertical_divider(sidebar_width: Option<Signal<f32, LocalStorage>>) -> AnyView {
    let Some(width) = sidebar_width else {
        return AnyView::new(());
    };
    AnyView::new(view! {
        box(
            class = "gds-frame__v-divider",
            a11y:hidden = true,
            style:left = move || Some(format!("{:.2}px", (width.get() - DIVIDER_WIDTH).max(0.0)))
        ) {}
    })
}

const RESIZE_EDGES: [(ResizeEdge, &str); 8] = [
    (ResizeEdge::North, "gds-frame__grip gds-frame__grip--n"),
    (ResizeEdge::South, "gds-frame__grip gds-frame__grip--s"),
    (ResizeEdge::West, "gds-frame__grip gds-frame__grip--w"),
    (ResizeEdge::East, "gds-frame__grip gds-frame__grip--e"),
    (ResizeEdge::NorthWest, "gds-frame__grip gds-frame__grip--nw"),
    (ResizeEdge::NorthEast, "gds-frame__grip gds-frame__grip--ne"),
    (ResizeEdge::SouthWest, "gds-frame__grip gds-frame__grip--sw"),
    (ResizeEdge::SouthEast, "gds-frame__grip gds-frame__grip--se"),
];

fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::North | ResizeEdge::South => CursorStyle::ResizeNorthSouth,
        ResizeEdge::East | ResizeEdge::West => CursorStyle::ResizeEastWest,
        ResizeEdge::NorthWest | ResizeEdge::SouthEast => CursorStyle::ResizeNorthWestSouthEast,
        _ => CursorStyle::ResizeNorthEastSouthWest,
    }
}

#[component]
fn ResizeGrip(edge: ResizeEdge, class: &'static str) -> impl IntoView {
    let Some(window) = zgui::prelude::try_use_window() else {
        return AnyView::new(());
    };
    let cursor = resize_cursor(edge);
    let resize_handler = window.resize_drag_handler(edge);
    let enter_win = window.clone();
    let leave_win = window;
    AnyView::new(view! {
        box(
            class = class,
            a11y:hidden = true,
            on:pointer_down = resize_handler,
            on:pointer_enter = move |_| enter_win.set_cursor(cursor),
            on:pointer_leave = move |_| leave_win.set_cursor(CursorStyle::Default)
        ) {}
    })
}

/// Outer window shell installing `"gds-appearance"` + `"gds-ui-base"` and propagating single-column
/// `sidebar_width` geometry across [`Toolbar`], [`WorkspaceFrame`], and [`Footer`].
#[component]
pub fn Frame(
    #[prop(into, optional)] sidebar_width: Option<Signal<f32, LocalStorage>>,
    #[prop(default = false)] without_sidebar: bool,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    if let Some(gds) = Gds::try_current() {
        let _ = gds.appearance().install_window_sheet();
    }
    install_frame_stylesheets();

    let resolved_sidebar_width = if without_sidebar {
        None
    } else {
        Some(sidebar_width.unwrap_or_else(|| Signal::stored_local(tokens::SIDEBAR_WIDTH)))
    };
    provide_local_context(FrameContext {
        sidebar_width: resolved_sidebar_width,
    });

    let window = zgui::prelude::try_use_window();
    let is_maximized = Signal::derive_local({
        let window = window.clone();
        move || {
            window
                .as_ref()
                .is_some_and(|w| w.maximized().get() || w.fullscreen().get().is_some())
        }
    });
    let is_tiled = Signal::derive_local({
        let window = window.clone();
        move || window.as_ref().is_some_and(|w| w.tiled().get().any())
    });
    let is_server_decorated = Signal::derive_local({
        let window = window.clone();
        move || window.as_ref().is_some_and(|w| w.server_decorated().get())
    });
    let framed = Signal::derive_local(move || {
        !is_maximized.get() && !is_tiled.get() && !is_server_decorated.get()
    });

    let content = if zgui_ui::tooltip::TooltipDelays::current().is_none() {
        AnyView::new(view! {
            GdsTooltipProvider {{children.into_view_once()}}
        })
    } else {
        children.into_view_once()
    };

    let grips = window.is_some().then(|| {
        AnyView::new(view! {
            Show(when = move || framed.get()) {
                for item in move || RESIZE_EDGES, key = |item: &(ResizeEdge, &'static str)| item.1 {
                    ResizeGrip(edge = item.0, class = item.1)
                }
            }
        })
    });

    view! {
        column(
            class = "gds-frame",
            attr:data-maximized = move || is_maximized.get().then(|| "true".to_owned()),
            attr:data-tiled = move || is_tiled.get().then(|| "true".to_owned()),
            attr:data-server-decorated = move || is_server_decorated.get().then(|| "true".to_owned()),
            class = class,
            {..attrs}
        ) {
            box(class = "gds-frame-content") {
                {content}
            }
            {grips}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use zgui::app::Fonts;
    use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
    use zgui::geom::{CssPx, Device, DevicePx, Edges, Point, Rect, Size};
    use zgui::platform::{AppHandler, Decorations, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{Mounted, RwSignal, flush, install};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::view;
    use zgui::vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Timestamp};
    use zgui_platform_headless::Harness;

    use super::{
        FOOTER_HEIGHT, FooterProps, FrameProps, TOOLBAR_HEIGHT, ToolbarProps, WindowDragAreaProps,
        WorkspaceFrameProps,
    };
    use crate::app::Gds;
    use crate::appearance::{theme, tokens};
    use crate::service::task_manager::Scope;

    struct NoopSink;

    impl TextureSink for NoopSink {
        fn create_texture(
            &mut self,
            _: TextureId,
            _: zgui::geom::Size<i32, Device>,
            _: TextureFormat,
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn write_texture(
            &mut self,
            _: TextureId,
            _: Rect<i32, Device>,
            _: TextureFormat,
            _: &[u8],
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn destroy_texture(&mut self, _: TextureId) {}
    }

    struct NoopRenderer {
        target: Option<RenderTarget>,
        sink: NoopSink,
    }

    impl Renderer for NoopRenderer {
        fn capabilities(&self) -> RenderCapabilities {
            RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.target
        }

        fn draw(&mut self, _: &zgui::scene::Scene, _: &zgui::bits::DamageSet) -> FrameOutcome {
            FrameOutcome::Presented(FrameStats::default())
        }

        fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
            TextureHandle(1)
        }

        fn release_external(&mut self, _: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::ZERO
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            &mut self.sink
        }
    }

    fn noop_renderer(
        _: &Arc<dyn Surface>,
        target: RenderTarget,
    ) -> Result<Box<dyn Renderer>, AppError> {
        Ok(Box::new(NoopRenderer {
            target: Some(target),
            sink: NoopSink,
        }))
    }

    #[test]
    fn toolbar_and_footer_heights_match_design_tokens() {
        assert_eq!(TOOLBAR_HEIGHT, tokens::TOOLBAR_HEIGHT);
        assert_eq!(FOOTER_HEIGHT, tokens::FOOTER_HEIGHT);
    }

    #[derive(Default)]
    struct SheetRecordingHost {
        sheets: std::cell::RefCell<std::collections::HashMap<String, String>>,
    }

    impl SheetRecordingHost {
        fn stylesheet(&self, name: &str) -> Option<String> {
            self.sheets.borrow().get(name).cloned()
        }
    }

    impl zgui::view::ViewHost for SheetRecordingHost {
        fn border_box(&self, _: zgui::view::NodeId) -> Option<Rect<DevicePx, Device>> {
            None
        }
        fn window_box(&self, _: zgui::view::NodeId) -> Option<Rect<DevicePx, Device>> {
            None
        }
        fn scale(&self) -> f32 {
            1.0
        }
        fn scroll_position(&self, _: zgui::view::NodeId) -> zgui::view::ScrollPosition {
            zgui::view::ScrollPosition::default()
        }
        fn scroll_to(
            &self,
            _: zgui::view::NodeId,
            _: zgui::view::ScrollTarget,
            _: zgui::view::ScrollBehavior,
        ) {
        }
        fn freeze_scrolling(&self, _: bool) {}
        fn focus(&self, _: zgui::view::NodeId) {}
        fn focused(&self) -> Signal<Option<zgui::view::NodeId>, zgui::reactive::LocalStorage> {
            Signal::derive_local(|| None)
        }
        fn contains(&self, _: zgui::view::NodeId, _: zgui::view::NodeId) -> bool {
            false
        }
        fn focusables(&self, _: zgui::view::NodeId) -> Vec<zgui::view::NodeId> {
            Vec::new()
        }
        fn focus_move(
            &self,
            _: zgui::view::NodeId,
            _: zgui::view::FocusMove,
        ) -> Option<zgui::view::NodeId> {
            None
        }
        fn push_focus_trap(
            &self,
            _: zgui::view::NodeId,
            _: zgui::view::FocusTrapOptions,
        ) -> zgui::view::FocusTrapId {
            zgui::view::FocusTrapId::new(0)
        }
        fn pop_focus_trap(&self, _: zgui::view::FocusTrapId) {}
        fn add_window_shortcut(&self, _: zgui::view::NodeId) {}
        fn remove_window_shortcut(&self, _: zgui::view::NodeId) {}
        fn is_text_entry(&self, _: zgui::view::NodeId) -> bool {
            false
        }
        fn selection(&self, _: zgui::view::NodeId) -> Option<core::ops::Range<usize>> {
            None
        }
        fn set_selection(&self, _: zgui::view::NodeId, _: core::ops::Range<usize>) {}
        fn select_all(&self, _: zgui::view::NodeId) {}
        fn set_value(&self, _: zgui::view::NodeId, _: &str) {}
        fn running_animations(&self, _: zgui::view::NodeId) -> usize {
            0
        }
        fn schedule(
            &self,
            _: core::time::Duration,
            _: zgui::view::Repeat,
            _: Rc<dyn Fn()>,
        ) -> zgui::view::TimerId {
            zgui::view::TimerId::new(0)
        }
        fn cancel_timer(&self, _: zgui::view::TimerId) {}
        fn request_frame_callback(&self, _: Rc<dyn Fn(Timestamp)>) -> zgui::view::FrameRequestId {
            zgui::view::FrameRequestId::new(0)
        }
        fn cancel_frame_callback(&self, _: zgui::view::FrameRequestId) {}
        fn precedes(&self, _: zgui::view::NodeId, _: zgui::view::NodeId) -> bool {
            false
        }
        fn install_stylesheet(&self, name: &str, css: &str) {
            self.sheets
                .borrow_mut()
                .insert(name.to_owned(), css.to_owned());
        }
        fn remove_stylesheet(&self, name: &str) {
            self.sheets.borrow_mut().remove(name);
        }
    }

    #[test]
    fn swapping_overlapping_frames_preserves_gds_appearance_stylesheet() {
        // Regression test for #95: mounting Frame B before Frame A drops must NEVER uninstall
        // `"gds-appearance"` from the window document while Frame B remains mounted!
        install().ok();
        let stub = Rc::new(SheetRecordingHost::default());
        let window_root = Mounted::new();
        window_root.with(|| {
            zgui::view::provide_host(zgui::view::HostHandle::from_rc(Rc::clone(&stub) as Rc<_>));
            let gds = Gds::install(Scope::default());

            let frame_a = Mounted::new();
            frame_a.with(|| {
                assert!(gds.appearance().install_window_sheet());
            });
            assert!(
                stub.stylesheet(theme::APPEARANCE_SHEET).is_some(),
                "Frame A installs gds-appearance"
            );

            // Mount Frame B BEFORE dropping Frame A (exact sequence when switching views).
            let frame_b = Mounted::new();
            frame_b.with(|| {
                assert!(gds.appearance().install_window_sheet());
            });

            // Now unmount Frame A while Frame B stays mounted!
            frame_a.unmount();
            assert!(
                stub.stylesheet(theme::APPEARANCE_SHEET).is_some(),
                "gds-appearance MUST remain installed after Frame A unmounts while Frame B is mounted"
            );

            // Verify reactive updates STILL propagate to the window while Frame B is mounted!
            gds.appearance().theme().update_window_corners(true);
            flush();
            let active_css = stub
                .stylesheet(theme::APPEARANCE_SHEET)
                .expect("gds-appearance still present after theme update");
            assert!(
                active_css.contains("--gds-radius-window: 10px;"),
                "reactive RenderEffect must survive Frame A unmounting"
            );

            // Unmounting Frame B (last holder) removes the sheet cleanly.
            frame_b.unmount();
            assert!(
                stub.stylesheet(theme::APPEARANCE_SHEET).is_none(),
                "gds-appearance is removed once the last Frame in the window unmounts"
            );
        });
        window_root.unmount();
    }

    #[test]
    fn frame_mounts_unmaximized_grips_hides_when_tiled_and_splits_columns_cleanly() {
        let clicked = Rc::new(Cell::new(0usize));
        let clicked_ref = Rc::clone(&clicked);
        let menu_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
        let menu_driver = Arc::clone(&menu_slot);
        let width_sig_slot: Arc<Mutex<Option<RwSignal<f32, zgui::reactive::LocalStorage>>>> =
            Arc::new(Mutex::new(None));
        let width_sig_driver = Arc::clone(&width_sig_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(800.0),
                DevicePx(500.0),
            )));
            harness.settle(32);

            // Hover over north edge grip to exercise ResizeEdge cursor routing when floating.
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(Point::new(CssPx(320.0), CssPx(2.0))),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            let surface = harness
                .platform()
                .offscreens()
                .first()
                .cloned()
                .expect("surface");
            assert!(
                surface
                    .cursor_log()
                    .contains(&CursorStyle::ResizeNorthSouth),
                "hovering north edge grip sets vertical resize cursor when floating"
            );

            // Collapse sidebar_width to 58px -> floating Frame has 1px CSD border, so menu button
            // center sits at 1 + 29 = 30px in window coords (and exact 29px when tiled!).
            if let Some(sig) = *width_sig_driver.lock().expect("lock") {
                sig.set(tokens::COLLAPSED_SIDEBAR_WIDTH);
                flush();
            }
            harness.settle(32);
            let node = menu_driver.lock().expect("lock").expect("menu ref");
            let bounds = node.window_bounds().expect("menu bounds");
            let scale = node.scale().max(1.0);
            let floating_center_x = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
            assert!(
                (floating_center_x - 30.0).abs() < 0.5,
                "32px menu button inside 58px collapsed toolbar column (1px floating frame border) sits at x=30, got {floating_center_x}"
            );

            // Click the menu button inside FrameToolbar — must fire click without starting window drag!
            let button_pos = Point::new(CssPx(30.0), CssPx(26.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(button_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(button_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            // Mark surface tiled -> 1px frame border squares to 0px and ResizeGrips unmount,
            // placing menu button center at exact x = 29px!
            harness
                .platform()
                .set_surface_tiled(surface.id(), Edges::ALL);
            harness.settle(16);
            let tiled_bounds = node.window_bounds().expect("tiled menu bounds");
            let tiled_center_x =
                (tiled_bounds.origin.x.0 + tiled_bounds.size.width.0 * 0.5) / scale;
            assert!(
                (tiled_center_x - 29.0).abs() < 0.5,
                "when tiled (0px frame border), 32px menu button sits dead-center at x=29, got {tiled_center_x}"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_decorations(Decorations::None)
            .with_size(800.0, 500.0)
            .run_on(driver, move || {
                let _gds = Gds::install(Scope::default());
                let sidebar_w = RwSignal::new_local(tokens::SIDEBAR_WIDTH);
                *width_sig_slot.lock().expect("lock") = Some(sidebar_w);
                let clicked = Rc::clone(&clicked_ref);
                let menu_ref = NodeRef::new();
                *menu_slot.lock().expect("lock") = Some(menu_ref);

                let menu_btn = AnyView::new(view! {
                    control(
                        node_ref = menu_ref,
                        tabindex = Focus::Sequential,
                        style:width = "32px",
                        style:height = "32px",
                        on:click = move |_| clicked.set(clicked.get() + 1)
                    ) {"M"}
                });
                let toolbar_controls = AnyView::new(view! {
                    WindowDragArea { text {"Address"} }
                });
                let sidebar_body = AnyView::new(view! { box { text {"Nav"} } });
                let main_body = AnyView::new(view! { box { text {"Files"} } });
                let status_body = AnyView::new(view! { text {"5 items"} });

                view! {
                    Frame(sidebar_width = Signal::from(sidebar_w)) {
                        Toolbar(menu = menu_btn, controls = toolbar_controls)
                        WorkspaceFrame(
                            sidebar = sidebar_body,
                            content = main_body,
                            paint_content_surface = true
                        )
                        Footer(status = status_body)
                    }
                }
            })
            .expect("frame runs headlessly");

        assert_eq!(
            clicked.get(),
            1,
            "menu control inside FrameToolbar must receive primary click without drag interference"
        );
    }
}
