//! Blank-space rectangle selection uses native pointer capture and tab-owned identities.

use std::{cell::RefCell, collections::HashSet, rc::Rc};

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, on_cleanup_local};
use zgui::view::{ListenerGuard, ListenerOptions, events, install_stylesheet};
use zgui::vocab::{Key, NamedKey, PointerButton};
use zgui::{css, view};

use super::{
    drag::take_row_pointer_down,
    geometry::virtual_row_height,
    model::{Row, RowKey},
    ui::Browser,
};
use crate::{entities::location::Location, shared::lib::selection::Selection};

const MARQUEE_THRESHOLD: f32 = 4.0;
const MARQUEE_SHEET: &str = "gds-files-marquee";
const MARQUEE_CSS: &str = css!(
    r#"
    .gds-files-marquee {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-marquee__box {
        position: absolute;
        pointer-events: none;
        border: 1px solid var(--gds-primary);
        background-color: var(--gds-drag-target, color-mix(in srgb, var(--gds-primary) 14%, transparent));
        border-radius: 0px;
        z-index: 5;
    }
    "#
);

#[derive(Clone, Copy)]
pub(super) enum Layout {
    List { viewport: NodeRef },
    Grid,
}

#[derive(Clone, Copy, PartialEq)]
struct RectOverlay {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

#[derive(Clone, PartialEq, Eq)]
struct Origin {
    tab_id: u64,
    location: Location,
}

#[derive(Clone)]
struct Snapshot {
    selection: Selection<RowKey>,
    cursor: Option<RowKey>,
    anchor: Option<RowKey>,
}

#[derive(Clone)]
struct MarqueeDrag {
    origin: Origin,
    start_win: (f32, f32),
    start_content: (f32, f32),
    active: bool,
    prior: Snapshot,
    baseline: Selection<RowKey>,
    additive: bool,
    toggle: bool,
}

impl Browser {
    pub(super) fn marquee_surface(&self, content: AnyView, layout: Layout) -> AnyView {
        if zgui::view::current_host().is_some() {
            install_stylesheet(MARQUEE_SHEET, MARQUEE_CSS);
        }

        let surface_node = NodeRef::new();
        let rect_signal: RwSignal<Option<RectOverlay>, LocalStorage> = RwSignal::new_local(None);
        let drag_state: Rc<RefCell<Option<MarqueeDrag>>> = Rc::new(RefCell::new(None));
        let escape_guard: Rc<RefCell<Option<ListenerGuard>>> = Rc::new(RefCell::new(None));

        {
            let cleanup_guard = Rc::clone(&escape_guard);
            let cleanup_state = Rc::clone(&drag_state);
            on_cleanup_local(move || {
                drop(cleanup_guard.borrow_mut().take());
                *cleanup_state.borrow_mut() = None;
            });
        }

        let down_browser = self.clone();
        let down_state = Rc::clone(&drag_state);
        let down_escape = Rc::clone(&escape_guard);

        let move_browser = self.clone();
        let move_state = Rc::clone(&drag_state);
        let move_escape = Rc::clone(&escape_guard);

        let up_browser = self.clone();
        let up_state = Rc::clone(&drag_state);
        let up_escape = Rc::clone(&escape_guard);

        let cancel_browser = self.clone();
        let cancel_state = drag_state;
        let cancel_escape = escape_guard;

        AnyView::new(view! {
            box(
                class = "gds-files-marquee",
                node_ref = surface_node,
                on:pointer_down = move |ev| {
                    if ev.button != Some(PointerButton::Primary) || !ev.primary {
                        return;
                    }
                    if take_row_pointer_down(&down_browser) {
                        return;
                    }
                    // Picker sessions and non-multi-selectable collections never open a rubber-band box.
                    if down_browser.read_untracked(|state| state.picker.is_some() || !state.allows_multiple()) {
                        return;
                    }
                    drop(down_escape.borrow_mut().take());
                    let win_pos = (ev.position.x.0, ev.position.y.0);
                    let content_pos = to_content_coords(&down_browser, layout, surface_node, win_pos);
                    let (origin, prior) = down_browser.read_untracked(|state| {
                        let tab = state.tab();
                        let view = tab.navigation.view();
                        (
                            Origin {
                                tab_id: tab.id,
                                location: tab.navigation.location().clone(),
                            },
                            Snapshot {
                                selection: view.selection.clone(),
                                cursor: view.cursor.clone(),
                                anchor: view.anchor.clone(),
                            },
                        )
                    });
                    let baseline = if ev.modifiers.shift() || ev.modifiers.primary() {
                        prior.selection.clone()
                    } else {
                        Selection::new()
                    };
                    *down_state.borrow_mut() = Some(MarqueeDrag {
                        origin,
                        start_win: win_pos,
                        start_content: content_pos,
                        active: false,
                        prior,
                        baseline,
                        additive: ev.modifiers.shift(),
                        toggle: ev.modifiers.primary(),
                    });
                },
                on:pointer_move = move |ev| {
                    let win_pos = (ev.position.x.0, ev.position.y.0);
                    let valid = move_browser.read_untracked(|state| {
                        state.picker.is_none() && state.allows_multiple()
                    });
                    if !valid {
                        *move_state.borrow_mut() = None;
                        drop(move_escape.borrow_mut().take());
                        rect_signal.set(None);
                        return;
                    }
                    let mut state_ref = move_state.borrow_mut();
                    let Some(drag) = state_ref.as_mut() else {
                        return;
                    };
                    if !origin_matches(&move_browser, &drag.origin) {
                        *state_ref = None;
                        drop(state_ref);
                        drop(move_escape.borrow_mut().take());
                        rect_signal.set(None);
                        return;
                    }
                    let dx = win_pos.0 - drag.start_win.0;
                    let dy = win_pos.1 - drag.start_win.1;
                    let just_activated = if !drag.active {
                        if (dx * dx + dy * dy).sqrt() < MARQUEE_THRESHOLD {
                            return;
                        }
                        drag.active = true;
                        ev.capture_pointer();
                        true
                    } else {
                        false
                    };
                    let cur_content = to_content_coords(&move_browser, layout, surface_node, win_pos);
                    let overlay = compute_overlay_rect(surface_node, drag.start_win, win_pos);
                    let x0 = drag.start_content.0.min(cur_content.0);
                    let y0 = drag.start_content.1.min(cur_content.1);
                    let x1 = drag.start_content.0.max(cur_content.0);
                    let y1 = drag.start_content.1.max(cur_content.1);
                    let drag_copy = drag.clone();
                    drop(state_ref);

                    if just_activated {
                        let esc_browser = move_browser.clone();
                        let esc_state = Rc::clone(&move_state);
                        let esc_guard_slot = Rc::clone(&move_escape);
                        let guard = move_browser.0.root_focus.listen(
                            events::KEY_DOWN,
                            ListenerOptions::CAPTURE,
                            move |key_ev: &mut EventCx<'_, events::KeyDown>| {
                                if key_ev.key != Key::Named(NamedKey::Escape) {
                                    return;
                                }
                                let active_drag = esc_state.borrow_mut().take();
                                drop(esc_guard_slot.borrow_mut().take());
                                rect_signal.set(None);
                                if let Some(cancelled) = active_drag
                                    && cancelled.active
                                {
                                    restore_snapshot(&esc_browser, &cancelled.origin, &cancelled.prior);
                                    key_ev.prevent_default();
                                    key_ev.stop_propagation();
                                }
                            },
                        );
                        *move_escape.borrow_mut() = guard;
                    }

                    rect_signal.set(overlay);
                    apply_marquee_selection(&move_browser, layout, (x0, y0, x1, y1), &drag_copy);
                },
                on:pointer_up = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    let finished = up_state.borrow_mut().take();
                    drop(up_escape.borrow_mut().take());
                    rect_signal.set(None);
                    let Some(drag) = finished else {
                        return;
                    };
                    if drag.active {
                        ev.release_pointer();
                        return;
                    }
                    if !ev.modifiers.shift()
                        && !ev.modifiers.primary()
                        && origin_matches(&up_browser, &drag.origin)
                    {
                        up_browser.reset_typeahead();
                        let cleared = up_browser.update_selection(|state| {
                            let view = state.tab_mut().navigation.view_mut();
                            let had_state = !view.selection.is_empty() || view.anchor.is_some();
                            view.selection.clear();
                            view.anchor = None;
                            had_state
                        });
                        let _ = cleared;
                    }
                },
                on:pointer_cancel = move |ev| {
                    let cancelled = cancel_state.borrow_mut().take();
                    drop(cancel_escape.borrow_mut().take());
                    rect_signal.set(None);
                    if let Some(drag) = cancelled
                        && drag.active
                    {
                        ev.release_pointer();
                        restore_snapshot(&cancel_browser, &drag.origin, &drag.prior);
                    }
                },
            ) {
                {content}
                {move || rect_signal.get().map(|rect| view! {
                    box(
                        class = "gds-files-marquee__box",
                        style:left = format!("{:.1}px", rect.left),
                        style:top = format!("{:.1}px", rect.top),
                        style:width = format!("{:.1}px", rect.width),
                        style:height = format!("{:.1}px", rect.height),
                    ) {}
                })}
            }
        })
    }
}

fn origin_matches(browser: &Browser, origin: &Origin) -> bool {
    browser.read_untracked(|state| {
        let tab = state.tab();
        tab.id == origin.tab_id && *tab.navigation.location() == origin.location
    })
}

fn restore_snapshot(browser: &Browser, origin: &Origin, prior: &Snapshot) {
    if !origin_matches(browser, origin) {
        return;
    }
    browser.update_selection(|state| {
        let view = state.tab_mut().navigation.view_mut();
        view.selection = prior.selection.clone();
        view.cursor = prior.cursor.clone();
        view.anchor = prior.anchor.clone();
    });
}

fn to_content_coords(
    browser: &Browser,
    layout: Layout,
    surface_node: NodeRef,
    win_pos: (f32, f32),
) -> (f32, f32) {
    match layout {
        Layout::List { viewport } => {
            let x_scale = viewport.scale().max(1.0);
            let x = viewport
                .window_bounds()
                .or_else(|| surface_node.window_bounds())
                .map_or(win_pos.0, |bounds| {
                    win_pos.0 - bounds.origin.x.0 / x_scale + viewport.scroll_offset().x.0 / x_scale
                });
            let list = browser.0.list;
            let y_scale = list.scale().max(1.0);
            let y = list
                .window_bounds()
                .or_else(|| surface_node.window_bounds())
                .map_or(win_pos.1, |bounds| {
                    win_pos.1 - bounds.origin.y.0 / y_scale + list.scroll_offset().y.0 / y_scale
                });
            (x, y)
        }
        Layout::Grid => {
            let scroll_node = browser.0.list;
            let bounds = scroll_node
                .window_bounds()
                .or_else(|| surface_node.window_bounds());
            let scale = scroll_node.scale().max(1.0);
            let offset = scroll_node.scroll_offset();
            if let Some(bounds) = bounds {
                (
                    win_pos.0 - bounds.origin.x.0 / scale + offset.x.0 / scale,
                    win_pos.1 - bounds.origin.y.0 / scale + offset.y.0 / scale,
                )
            } else {
                win_pos
            }
        }
    }
}

fn compute_overlay_rect(
    surface_node: NodeRef,
    a: (f32, f32),
    b: (f32, f32),
) -> Option<RectOverlay> {
    let bounds = surface_node.window_bounds()?;
    let scale = surface_node.scale().max(1.0);
    let origin_x = bounds.origin.x.0 / scale;
    let origin_y = bounds.origin.y.0 / scale;
    let max_w = bounds.size.width.0 / scale;
    let max_h = bounds.size.height.0 / scale;

    let x0 = (a.0.min(b.0) - origin_x).clamp(0.0, max_w);
    let y0 = (a.1.min(b.1) - origin_y).clamp(0.0, max_h);
    let x1 = (a.0.max(b.0) - origin_x).clamp(0.0, max_w);
    let y1 = (a.1.max(b.1) - origin_y).clamp(0.0, max_h);
    Some(RectOverlay {
        left: x0,
        top: y0,
        width: (x1 - x0).max(1.0),
        height: (y1 - y0).max(1.0),
    })
}

fn apply_marquee_selection(
    browser: &Browser,
    layout: Layout,
    rect: (f32, f32, f32, f32),
    drag: &MarqueeDrag,
) {
    if !origin_matches(browser, &drag.origin) {
        return;
    }
    let (x0, y0, x1, y1) = rect;
    let row_h = browser.row_height_untracked().max(1.0);
    let grid = browser.grid_geometry_untracked();
    let scale = browser.0.list.scale().max(1.0);

    browser.update_selection(|state| {
        let tab = state.tab_mut();
        let count = tab.rows.len();
        if count == 0 {
            return;
        }
        let hit_indices: Vec<usize> = match layout {
            Layout::List { .. } => {
                if y1 < 0.0 || x1 < 0.0 {
                    Vec::new()
                } else {
                    let first = (y0.max(0.0) / row_h).floor() as usize;
                    let last = ((y1.max(0.0) / row_h).ceil() as usize).min(count);
                    (first.min(last)..last).collect()
                }
            }
            Layout::Grid => {
                let step_h = virtual_row_height(grid.row_height, scale).max(1.0);
                let first = (y0.max(0.0) / step_h).floor() as usize * grid.columns;
                let end = ((y1.max(0.0) / step_h).ceil() as usize * grid.columns).min(count);
                (first.min(end)..end)
                    .filter(|&index| {
                        let tile = grid.tile(index, scale);
                        tile.x < x1
                            && tile.x + tile.width > x0
                            && tile.y < y1
                            && tile.y + tile.height > y0
                    })
                    .collect()
            }
        };

        let hit_keys: Vec<RowKey> = hit_indices
            .iter()
            .filter_map(|&idx| tab.rows.get(idx).map(Row::key))
            .collect();

        let view = tab.navigation.view_mut();
        if drag.toggle {
            let hit_set: HashSet<RowKey> = hit_keys.iter().cloned().collect();
            let mut merged: Vec<RowKey> = drag
                .baseline
                .iter()
                .filter(|key| !hit_set.contains(*key))
                .cloned()
                .collect();
            for key in &hit_keys {
                if !drag.baseline.contains(key) {
                    merged.push(key.clone());
                }
            }
            view.selection.replace(merged);
        } else if drag.additive {
            let mut merged = drag.baseline.to_vec();
            for key in &hit_keys {
                if !drag.baseline.contains(key) {
                    merged.push(key.clone());
                }
            }
            view.selection.replace(merged);
        } else {
            view.selection.replace(hit_keys.clone());
        }
        if let Some(last) = hit_keys.last() {
            view.cursor = Some(last.clone());
        }
        if view.anchor.is_none() {
            view.anchor = hit_keys.first().cloned();
        }
    });
}
