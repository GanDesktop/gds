//! Rubber-band selection over the file list's blank space.
//!
//! A press on blank space below the rows, or in the list's side gutter, arms a
//! marquee; once the pointer travels past [`DRAG_THRESHOLD`] the rectangle is
//! drawn and every visible row it crosses is written back as the Browser's own
//! path selection. Row drags, header sorting and column resizing, scrollbars and
//! context menus keep their own handlers: only blank space starts a marquee, and
//! a press that never crosses the threshold keeps its plain blank-click meaning.

use gds_uim::el;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gds::appearance::tokens::DataListStyle;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{prelude::*, *};

use super::view::{SCROLLBAR_LANE, TABLE_INSET};
use super::*;

/// Pointer travel that turns a blank press into a marquee. A shorter press is
/// still a plain blank click.
const DRAG_THRESHOLD: f32 = 4.;

/// The page a marquee started on. Selection is written only while that page is
/// still the visible one, so a marquee never edits another tab's selection.
struct Origin {
    tab_id: u64,
    path: PathBuf,
}

/// Selection and cursor snapshot captured before the press cleared them.
#[derive(Clone)]
struct Baseline {
    selection: Selection<PathBuf>,
    cursor: Option<PathBuf>,
    anchor: Option<PathBuf>,
    /// Ctrl/Shift was held at the press: keep the starting selection and add.
    merge: bool,
}

/// One armed or running marquee, with the selection it must restore on cancel.
struct Drag {
    origin: Origin,
    /// Press point in window coordinates.
    start: Point<Pixels>,
    /// Latest clamped pointer point in window coordinates.
    current: Point<Pixels>,
    baseline: Baseline,
    /// Set once the pointer passes the threshold; until then the press keeps its
    /// plain blank-click behavior.
    started: bool,
    header_height: f32,
}

impl Drag {
    fn rect(&self) -> Bounds<Pixels> {
        Bounds::from_corners(
            point(
                self.start.x.min(self.current.x),
                self.start.y.min(self.current.y),
            ),
            point(
                self.start.x.max(self.current.x),
                self.start.y.max(self.current.y),
            ),
        )
    }

    /// Paint the rectangle, clipped to the rows viewport and the scrollbar lanes.
    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        if !self.started {
            return;
        }
        let Some(viewport) = State::viewport(bounds, self.header_height) else {
            return;
        };
        let rect = self.rect();
        let left = rect.origin.x.max(viewport.origin.x);
        let top = rect.origin.y.max(viewport.origin.y);
        let right = (rect.origin.x + rect.size.width).min(viewport.origin.x + viewport.size.width);
        let bottom =
            (rect.origin.y + rect.size.height).min(viewport.origin.y + viewport.size.height);
        if right <= left || bottom <= top {
            return;
        }
        let theme = cx.theme();
        window.paint_quad(quad(
            Bounds::new(point(left, top), size(right - left, bottom - top)),
            px(0.),
            theme.drop_target,
            px(1.),
            theme.drag_border,
            BorderStyle::default(),
        ));
    }
}

/// Marquee state owned by the Browser for the lifetime of the window.
pub(in crate::pages::workspace) struct State {
    /// Window-space bounds of the wrapped list surface, written by the overlay's
    /// prepaint on every frame.
    bounds: Rc<RefCell<Bounds<Pixels>>>,
    drag: Rc<RefCell<Option<Drag>>>,
}

impl State {
    /// The surface region that rows occupy: below the fixed header, inside the
    /// table's horizontal bounds, above the bottom scrollbar lane.
    fn viewport(bounds: Bounds<Pixels>, header_height: f32) -> Option<Bounds<Pixels>> {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let left = TABLE_INSET;
        let right = width - TABLE_INSET;
        let top = header_height;
        let bottom = height - SCROLLBAR_LANE;
        if right <= left || bottom <= top {
            return None;
        }
        Some(Bounds::new(
            point(bounds.origin.x + px(left), bounds.origin.y + px(top)),
            size(px(right - left), px(bottom - top)),
        ))
    }

    /// Whether `local` (surface coordinates) is blank space a marquee may start
    /// from: the side gutter, or below the last row.
    fn blank_at(
        bounds: Bounds<Pixels>,
        local: Point<Pixels>,
        rows: usize,
        scroll_offset: Pixels,
    ) -> bool {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let x = f32::from(local.x);
        let y = f32::from(local.y);
        // The header keeps sorting and column resizing, and both scrollbar lanes
        // keep their own dragging.
        if y < DataListStyle::HEADER_HEIGHT
            || y > height - SCROLLBAR_LANE
            || x > width - TABLE_INSET - SCROLLBAR_LANE
        {
            return false;
        }
        if x < TABLE_INSET {
            return true;
        }
        let rows_top = DataListStyle::HEADER_HEIGHT + f32::from(scroll_offset);
        let below = (y - rows_top).max(0.);
        (below / DataListStyle::ROW_HEIGHT).floor() as usize >= rows
    }

    fn clear(&self) {
        self.drag.borrow_mut().take();
    }

    pub(in crate::pages::workspace) fn is_active(&self) -> bool {
        self.drag.borrow().as_ref().is_some_and(|drag| drag.started)
    }
}

enum MarqueeHits {
    Indices(std::ops::Range<usize>),
    Paths(Vec<PathBuf>),
}

impl Default for State {
    fn default() -> Self {
        Self {
            bounds: Rc::new(RefCell::new(Bounds::default())),
            drag: Rc::new(RefCell::new(None)),
        }
    }
}

impl Browser {
    /// Remove a marquee when its file surface is hidden by a page or popup.
    pub(in crate::pages::workspace) fn discard_marquee(&self) {
        self.files.marquee.clear();
    }

    /// Wrap the file list area so blank-space presses can start a marquee.
    pub(in crate::pages::workspace) fn marquee_surface(
        &self,
        content: AnyElement,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.marquee_discard_stale();
        let bounds = Rc::clone(&self.files.marquee.bounds);
        let drag = Rc::clone(&self.files.marquee.drag);
        let overlay = canvas(
            move |canvas_bounds: Bounds<Pixels>, _: &mut Window, _: &mut App| {
                *bounds.borrow_mut() = canvas_bounds;
            },
            move |canvas_bounds: Bounds<Pixels>, (), window: &mut Window, cx: &mut App| {
                if let Some(drag) = drag.borrow().as_ref() {
                    drag.paint(canvas_bounds, window, cx);
                }
            },
        )
        .absolute()
        .inset_0();
        el! {
            .div {
                # "marquee-surface"; relative; flex; flex_col; flex_1; min_h_0; min_w_0; overflow_hidden;
                capture_any_mouse_down: $(cx.listener(Self::marquee_press));
                on_mouse_move: $(cx.listener(Self::marquee_drag));
                on_mouse_up: MouseButton::Left, $(cx.listener(Self::marquee_release));
                on_mouse_up_out: MouseButton::Left, $(cx.listener(Self::marquee_release));
            } [content, overlay]
        }
        .into_any_element()
    }

    /// Cancel a running marquee and restore the selection it started from.
    ///
    /// Returns whether a marquee was running. An armed press that never crossed
    /// the threshold is dropped silently, so the key keeps its other meanings.
    pub(in crate::pages::workspace) fn cancel_marquee(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.files.marquee.drag.borrow_mut().take() else {
            return false;
        };
        if !drag.started {
            return false;
        }
        if self.marquee_origin_current(&drag.origin) {
            self.release_active_selection_mirrors(cx);
            self.marquee_restore(&drag.baseline);
            if let Some(slot) = self.active_mut() {
                slot.refresh_visible_if_dirty();
            }
        }
        cx.notify();
        true
    }

    /// Record the press before the blank-space handler clears the selection, so
    /// Ctrl/Shift marquees still know what they started from.
    fn marquee_press(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.files.marquee.clear();
        if event.button != MouseButton::Left
            || !self.pages.is_empty()
            || self.collection().is_some()
        {
            return;
        }
        let bounds = *self.files.marquee.bounds.borrow();
        if !bounds.contains(&event.position) {
            return;
        }
        let local = event.position - bounds.origin;
        let Some(slot) = self.active() else {
            return;
        };
        if slot.navigation.directory().is_none() || slot.table_rows.is_empty() {
            return;
        }
        let scroll_offset = slot.scroll.0.borrow().base_handle.offset().y;
        let blank = if self.grid_mode() {
            local.y < bounds.size.height - px(SCROLLBAR_LANE)
                && local.x < bounds.size.width - px(TABLE_INSET + SCROLLBAR_LANE)
                && slot
                    .grid
                    .hit(bounds, event.position, scroll_offset, slot.table_rows.len())
                    .is_none()
        } else {
            State::blank_at(bounds, local, slot.table_rows.len(), scroll_offset)
        };
        if !blank {
            return;
        }
        let tab = &slot.navigation;
        let drag = Drag {
            origin: Origin {
                tab_id: slot.id,
                path: match slot.path() {
                    Some(path) => path.to_path_buf(),
                    None => return,
                },
            },
            start: event.position,
            current: event.position,
            baseline: Baseline {
                selection: tab.view().selected.clone(),
                cursor: slot.cursor.clone(),
                anchor: slot.anchor.clone(),
                merge: event.modifiers.control || event.modifiers.platform || event.modifiers.shift,
            },
            started: false,
            header_height: if self.grid_mode() {
                0.
            } else {
                DataListStyle::HEADER_HEIGHT
            },
        };
        *self.files.marquee.drag.borrow_mut() = Some(drag);
        cx.notify();
    }

    fn marquee_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.files.marquee.drag.borrow_mut().take() else {
            return;
        };
        // A page switch mid-drag drops the marquee without touching any
        // selection; the render-time check does the same for other transitions.
        if !self.marquee_origin_current(&drag.origin) {
            cx.notify();
            return;
        }
        let bounds = *self.files.marquee.bounds.borrow();
        let max = point(
            bounds.origin.x + bounds.size.width,
            bounds.origin.y + bounds.size.height,
        );
        let current = event.position.clamp(&bounds.origin, &max);
        if !drag.started {
            let dx = f32::from(current.x - drag.start.x);
            let dy = f32::from(current.y - drag.start.y);
            if dx.hypot(dy) < DRAG_THRESHOLD {
                *self.files.marquee.drag.borrow_mut() = Some(drag);
                return;
            }
            drag.started = true;
            self.window.focus.focus(window, cx);
        }
        drag.current = current;
        let hits = self.marquee_hits(drag.rect(), bounds);
        let baseline = drag.baseline.clone();
        *self.files.marquee.drag.borrow_mut() = Some(drag);
        self.release_active_selection_mirrors(cx);
        self.marquee_write_selection(hits, &baseline);
        cx.notify();
    }

    fn marquee_release(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.files.marquee.drag.borrow_mut().take().is_some() {
            if self.active().is_some_and(|slot| slot.projection_dirty) {
                self.release_active_selection_mirrors(cx);
                if let Some(slot) = self.active_mut() {
                    slot.refresh_visible_if_dirty();
                }
            }
            cx.notify();
        }
    }

    /// The visible rows the rectangle crosses: contiguous index span in list mode,
    /// or tile-intersected paths in grid mode.
    fn marquee_hits(&self, rect: Bounds<Pixels>, bounds: Bounds<Pixels>) -> MarqueeHits {
        let Some(slot) = self.active() else {
            return MarqueeHits::Indices(0..0);
        };
        if slot.navigation.directory().is_none() {
            return MarqueeHits::Indices(0..0);
        }
        let Some(viewport) = State::viewport(
            bounds,
            if self.grid_mode() {
                0.
            } else {
                DataListStyle::HEADER_HEIGHT
            },
        ) else {
            return MarqueeHits::Indices(0..0);
        };
        let viewport_right = viewport.origin.x + viewport.size.width;
        let rect_right = rect.origin.x + rect.size.width;
        if rect.origin.x >= viewport_right || rect_right <= viewport.origin.x {
            return MarqueeHits::Indices(0..0);
        }
        let top = rect.origin.y.max(viewport.origin.y);
        let bottom =
            (rect.origin.y + rect.size.height).min(viewport.origin.y + viewport.size.height);
        if bottom <= top {
            return MarqueeHits::Indices(0..0);
        }
        // Rows scroll under the fixed header, so screen position must account
        // for the list's scroll offset instead of assuming row 0 is at the top.
        let scroll_offset = slot.scroll.0.borrow().base_handle.offset().y;
        if self.grid_mode() {
            let clipped = Bounds::new(
                point(rect.left().max(viewport.left()), top),
                size(
                    (rect.right().min(viewport.right()) - rect.left().max(viewport.left()))
                        .max(px(0.)),
                    bottom - top,
                ),
            );
            let first = ((f32::from(top - bounds.top() - scroll_offset) / slot.grid.row_height)
                .floor()
                .max(0.) as usize)
                * slot.grid.columns;
            let last = ((f32::from(bottom - bounds.top() - scroll_offset) / slot.grid.row_height)
                .ceil()
                .max(0.) as usize)
                * slot.grid.columns;
            return MarqueeHits::Paths(
                (first..last.min(slot.table_rows.len()))
                    .filter_map(|index| {
                        slot.grid
                            .tile(index, bounds.origin, scroll_offset)
                            .intersects(&clipped)
                            .then(|| slot.table_rows[index].path.clone())
                    })
                    .collect(),
            );
        }
        let rows_top = viewport.origin.y + scroll_offset;
        let first = ((f32::from(top - rows_top) / DataListStyle::ROW_HEIGHT)
            .floor()
            .max(0.) as usize)
            .min(slot.table_rows.len());
        let last = ((f32::from(bottom - rows_top) / DataListStyle::ROW_HEIGHT)
            .ceil()
            .max(0.) as usize)
            .min(slot.table_rows.len());
        MarqueeHits::Indices(first..last)
    }

    /// Write `hits` into the tab's own selection, the same paths copy, cut and
    /// delete read.
    fn marquee_write_selection(&mut self, hits: MarqueeHits, baseline: &Baseline) {
        let Some(slot) = self.active_mut() else {
            return;
        };
        match hits {
            MarqueeHits::Indices(range) if !baseline.merge => {
                if range.is_empty() {
                    slot.navigation.view_mut().selected.clear();
                    slot.cursor = None;
                    slot.cursor_index = None;
                    slot.anchor = None;
                    slot.anchor_index = None;
                } else {
                    let end = range.end - 1;
                    slot.select_index_range(range.start, end);
                    slot.anchor = Some(slot.table_rows[range.start].path.clone());
                    slot.anchor_index = Some(range.start);
                    slot.cursor = Some(slot.table_rows[end].path.clone());
                    slot.cursor_index = Some(end);
                }
            }
            MarqueeHits::Indices(range) => {
                let mut next = baseline.selection.clone();
                for entry in &slot.table_rows[range.clone()] {
                    next.insert(entry.path.clone());
                }
                if slot.navigation.view().selected != next {
                    slot.navigation.view_mut().selected = next;
                }
                if !range.is_empty() {
                    let end = range.end - 1;
                    slot.cursor = Some(slot.table_rows[end].path.clone());
                    slot.cursor_index = Some(end);
                    slot.anchor = baseline
                        .anchor
                        .clone()
                        .or_else(|| Some(slot.table_rows[range.start].path.clone()));
                    slot.anchor_index = None;
                } else {
                    slot.cursor = baseline.cursor.clone();
                    slot.cursor_index = None;
                    slot.anchor = baseline.anchor.clone();
                    slot.anchor_index = None;
                }
            }
            MarqueeHits::Paths(paths) => {
                if baseline.merge {
                    let mut next = baseline.selection.clone();
                    for path in &paths {
                        next.insert(path.clone());
                    }
                    if slot.navigation.view().selected != next {
                        slot.navigation.view_mut().selected = next;
                    }
                } else if slot.navigation.view().selected != paths {
                    let mut next = Selection::new();
                    next.replace(paths.iter().cloned());
                    slot.navigation.view_mut().selected = next;
                }
                if let Some(last) = paths.last() {
                    slot.cursor = Some(last.clone());
                    slot.cursor_index = None;
                    slot.anchor = if baseline.merge {
                        baseline.anchor.clone().or_else(|| paths.first().cloned())
                    } else {
                        paths.first().cloned()
                    };
                    slot.anchor_index = None;
                } else if baseline.merge {
                    // The blank press cleared these; a merge must keep its starting point.
                    slot.cursor = baseline.cursor.clone();
                    slot.cursor_index = None;
                    slot.anchor = baseline.anchor.clone();
                    slot.anchor_index = None;
                } else {
                    slot.cursor = None;
                    slot.cursor_index = None;
                    slot.anchor = None;
                    slot.anchor_index = None;
                }
            }
        }
    }

    fn marquee_restore(&mut self, baseline: &Baseline) {
        let Some(slot) = self.active_mut() else {
            return;
        };
        {
            let tab = &mut slot.navigation;
            tab.view_mut().selected = baseline.selection.clone();
        }
        slot.cursor = baseline.cursor.clone();
        slot.anchor = baseline.anchor.clone();
    }

    /// Whether `origin` is still the page the marquee may write to.
    fn marquee_origin_current(&self, origin: &Origin) -> bool {
        self.collection().is_none()
            && self.tabs.active_id == origin.tab_id
            && self.current_path().as_ref() == Some(&origin.path)
    }

    /// Drop a marquee whose page is gone, without editing another page.
    fn marquee_discard_stale(&self) {
        let stale = self
            .files
            .marquee
            .drag
            .borrow()
            .as_ref()
            .is_some_and(|drag| !self.marquee_origin_current(&drag.origin));
        if stale {
            self.files.marquee.clear();
        }
    }
}
