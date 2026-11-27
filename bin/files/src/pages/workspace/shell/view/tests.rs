use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use zgui::atlas::{MemorySink, TextureSink};
use zgui::bits::DamageSet;
use zgui::geom::{CssPx, Point};
use zgui::platform::{AppHandler, Surface, SurfaceEvent};
use zgui::prelude::{GetUntracked, NodeRef};
use zgui::render::{
    ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
    Renderer, TextureHandle,
};
use zgui::runtime::AppError;
use zgui::scene::Scene;
use zgui::vocab::{
    KeyEvent, KeyState, Modifiers, PointerAction, PointerButton, PointerEvent, Timestamp,
};
use zgui_platform_headless::Harness;

use crate::entities::file::columns::Column;
use crate::pages::workspace::shell::Browser;

mod interaction;
mod layout;

thread_local! {
    static HEADER_CELL_NODES: RefCell<HashMap<Column, NodeRef>> = RefCell::new(HashMap::new());
    static ROW_CELL_NODES: RefCell<HashMap<(usize, Column), NodeRef>> = RefCell::new(HashMap::new());
    static HEADER_RESIZE_NODES: RefCell<HashMap<Column, NodeRef>> = RefCell::new(HashMap::new());
    static FOOTER_TERMINAL_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
    static LAST_COLOR_SPRITES: RefCell<Vec<(f32, f32)>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn record_header_cell_node(column: Column, node: NodeRef) {
    HEADER_CELL_NODES.with(|slot| {
        slot.borrow_mut().insert(column, node);
    });
}

pub(super) fn record_row_cell_node(index: usize, column: Column, node: NodeRef) {
    ROW_CELL_NODES.with(|slot| {
        slot.borrow_mut().insert((index, column), node);
    });
}

pub(super) fn record_header_resize_node(column: Column, node: NodeRef) {
    HEADER_RESIZE_NODES.with(|slot| {
        slot.borrow_mut().insert(column, node);
    });
}

pub(super) fn record_footer_terminal_node(node: NodeRef) {
    FOOTER_TERMINAL_NODE.with(|slot| {
        *slot.borrow_mut() = Some(node);
    });
}

fn clear_view_probes() {
    HEADER_CELL_NODES.with(|slot| slot.borrow_mut().clear());
    ROW_CELL_NODES.with(|slot| slot.borrow_mut().clear());
    HEADER_RESIZE_NODES.with(|slot| slot.borrow_mut().clear());
    FOOTER_TERMINAL_NODE.with(|slot| *slot.borrow_mut() = None);
    LAST_COLOR_SPRITES.with(|slot| slot.borrow_mut().clear());
}

fn footer_terminal_node() -> Option<NodeRef> {
    FOOTER_TERMINAL_NODE.with(|slot| *slot.borrow())
}

fn assert_content_width_matches_layout(browser: &Browser) {
    let node = browser.0.content_focus;
    let bounds = node.bounds().expect("file content has completed layout");
    let measured = bounds.size.width.0;
    let expected = browser.0.content_width.get_untracked() * node.scale();
    assert!(
        (measured - expected).abs() <= 1.0,
        "file content width was {measured} device pixels, expected {expected}"
    );
}

fn last_color_sprites() -> Vec<(f32, f32)> {
    LAST_COLOR_SPRITES.with(|slot| slot.borrow().clone())
}

fn header_cell_node(column: Column) -> Option<NodeRef> {
    HEADER_CELL_NODES.with(|slot| slot.borrow().get(&column).copied())
}

fn row_cell_node(index: usize, column: Column) -> Option<NodeRef> {
    ROW_CELL_NODES.with(|slot| slot.borrow().get(&(index, column)).copied())
}

fn header_resize_node(column: Column) -> Option<NodeRef> {
    HEADER_RESIZE_NODES.with(|slot| slot.borrow().get(&column).copied())
}

fn assert_directory_header_and_row_cell_parity(browser: &Browser, row_indices: &[usize]) {
    let visible_columns = browser.read_untracked(|state| state.tab().columns.columns().to_vec());
    for column in visible_columns {
        let expected_w = super::list::column_width_untracked(browser, column);
        let header_node = header_cell_node(column)
            .unwrap_or_else(|| panic!("recorded header cell node for {column:?}"));
        let scale = header_node.scale().max(1.0);
        let header_bounds = header_node
            .window_bounds()
            .unwrap_or_else(|| panic!("measured header cell bounds for {column:?}"));
        let header_x = header_bounds.origin.x.0 / scale;
        let header_w = header_bounds.size.width.0 / scale;
        assert!(
            (header_bounds.size.width.0 - expected_w * scale).abs() <= 1.0,
            "expected {column:?} header cell width ~= {expected_w:.2}px, got {header_w:.2}px"
        );
        for &row_idx in row_indices {
            let cell_node = row_cell_node(row_idx, column)
                .unwrap_or_else(|| panic!("recorded row {row_idx} cell node for {column:?}"));
            let cell_bounds = cell_node
                .window_bounds()
                .unwrap_or_else(|| panic!("measured row {row_idx} cell bounds for {column:?}"));
            let cell_x = cell_bounds.origin.x.0 / scale;
            let cell_w = cell_bounds.size.width.0 / scale;
            assert!(
                (header_w - cell_w).abs() == 0.0 && (header_x - cell_x).abs() == 0.0,
                "expected exact 0.0px header/row parity for {column:?} on row {row_idx}: header=({header_x:.2}, {header_w:.2}) vs row=({cell_x:.2}, {cell_w:.2})"
            );
        }
    }
}

#[derive(Default)]
struct NoopRenderer {
    atlas: MemorySink,
    target: Option<RenderTarget>,
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

    fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
        TextureHandle(0)
    }

    fn release_external(&mut self, _: TextureHandle) {}

    fn memory(&self) -> MemoryReport {
        MemoryReport::default()
    }

    fn draw(&mut self, scene: &Scene, _: &DamageSet) -> FrameOutcome {
        if !scene.primitives.color_sprites.is_empty() {
            LAST_COLOR_SPRITES.with(|slot| {
                let mut out = slot.borrow_mut();
                out.clear();
                for sprite in &scene.primitives.color_sprites {
                    let [_, _, w, h] = sprite.bounds;
                    if w > 0.0 && h > 0.0 {
                        out.push((w, h));
                    }
                }
            });
        }
        FrameOutcome::Presented(FrameStats::default())
    }

    fn texture_sink(&mut self) -> &mut dyn TextureSink {
        &mut self.atlas
    }
}

fn noop_renderer(
    _: &Arc<dyn Surface>,
    target: RenderTarget,
) -> Result<Box<dyn Renderer>, AppError> {
    let mut renderer = NoopRenderer::default();
    renderer.configure(target);
    Ok(Box::new(renderer))
}

fn send_ctrl_b(harness: &mut Harness<Box<dyn AppHandler>>) {
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Pressed,
        event: KeyEvent::character("b"),
        modifiers: Modifiers::CONTROL,
        timestamp: Timestamp::ORIGIN,
    });
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Released,
        event: KeyEvent::character("b"),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::ORIGIN,
    });
    harness.pump();
    zgui::reactive::flush();
}

fn click_node_center(harness: &mut Harness<Box<dyn AppHandler>>, node: NodeRef) {
    let scale = node.scale().max(1.0);
    let bounds = node.window_bounds().expect("measured node window_bounds");
    let cx = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
    let cy = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;
    let pos = Point::new(CssPx(cx), CssPx(cy));
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Pressed,
        event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::ORIGIN,
    });
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Released,
        event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::ORIGIN,
    });
    harness.settle(8);
    zgui::reactive::flush();
}

pub(in crate::pages::workspace) fn send_key_chord(
    harness: &mut Harness<Box<dyn AppHandler>>,
    event: KeyEvent,
    modifiers: Modifiers,
) {
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Pressed,
        event: event.clone(),
        modifiers,
        timestamp: Timestamp::ORIGIN,
    });
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Released,
        event,
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::ORIGIN,
    });
    harness.settle(6);
    zgui::reactive::flush();
}
