//! Shared top-bar window drag, double-press maximize, and native window-menu gesture controller.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use zgui::geom::{Css, CssPx, Point};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, on_cleanup_local, provide_local_context, use_local_context};
use zgui::view::{EventCx, ListenerGuard, ListenerOptions, events};
use zgui::vocab::{PointerButton, PointerId, Timestamp};

use crate::ui::interaction::drag::DRAG_THRESHOLD;

const DOUBLE_PRESS_WINDOW: Duration = Duration::from_millis(400);
const DOUBLE_PRESS_SLOP: f32 = 4.0;

#[derive(Clone, Copy)]
struct WindowDragScope;

#[derive(Clone, Copy, Debug)]
struct DragAnchor {
    pointer_id: PointerId,
    origin: Point<CssPx, Css>,
}

#[derive(Clone, Copy, Debug, Default)]
enum DragPhase {
    #[default]
    Idle,
    /// Primary press observed inside the top bar; awaiting either threshold motion or release.
    Armed(DragAnchor),
    /// Threshold crossed and compositor move requested; suppress any trailing release activation
    /// for this pointer if the backend delivers a release in the same batch or declines the drag.
    Moved(PointerId),
}

#[derive(Default)]
struct DragSurfaceState {
    phase: Cell<DragPhase>,
    last_empty_press: Cell<Option<(Timestamp, Point<CssPx, Css>)>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DragAction {
    MoveDrag,
    DoubleClick,
    WindowMenu(Point<CssPx, Css>),
}

#[cfg(test)]
thread_local! {
    static TEST_DRAG_ACTIONS: RefCell<Option<Vec<DragAction>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct DragActionGuard;

#[cfg(test)]
impl DragActionGuard {
    pub(crate) fn take(&self) -> Vec<DragAction> {
        TEST_DRAG_ACTIONS.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .map_or_else(Vec::new, std::mem::take)
        })
    }
}

#[cfg(test)]
impl Drop for DragActionGuard {
    fn drop(&mut self) {
        TEST_DRAG_ACTIONS.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
pub(crate) fn record_drag_actions() -> DragActionGuard {
    TEST_DRAG_ACTIONS.with(|slot| {
        *slot.borrow_mut() = Some(Vec::new());
    });
    DragActionGuard
}

#[cfg(test)]
fn record_test_action(action: DragAction) {
    TEST_DRAG_ACTIONS.with(|slot| {
        if let Some(log) = slot.borrow_mut().as_mut() {
            log.push(action);
        }
    });
}

/// Marks the current component subtree as enclosed by a top-bar drag controller so nested
/// [`crate::ui::frame::WindowDragArea`] spacers delegate to the enclosing bar instead of attaching
/// duplicate listeners or splitting double-press state.
pub(crate) fn provide_window_drag_scope() {
    provide_local_context(WindowDragScope);
}

pub(crate) fn inside_window_drag_scope() -> bool {
    zgui::reactive::is_ui_thread() && use_local_context::<WindowDragScope>().is_some()
}

/// Returns `true` when `target` is inside a focusable interactive control beneath `current`.
pub(crate) fn is_interactive_descendant(current: NodeId, target: NodeId) -> bool {
    if current == target {
        return false;
    }
    zgui::view::current_host().is_some_and(|host| {
        host.focusables(current).into_iter().any(|focusable| {
            focusable != current && (focusable == target || host.contains(focusable, target))
        })
    })
}

/// Returns `true` when `target` is a text entry element (or inside one) beneath `current`.
fn is_text_entry_descendant(current: NodeId, target: NodeId) -> bool {
    zgui::view::current_host().is_some_and(|host| {
        if host.is_text_entry(target) {
            return true;
        }
        host.focusables(current).into_iter().any(|focusable| {
            focusable != current
                && host.is_text_entry(focusable)
                && (focusable == target || host.contains(focusable, target))
        })
    })
}

fn trigger_move_drag(window: Option<&WindowHandle>) {
    if let Some(w) = window {
        #[cfg(test)]
        record_test_action(DragAction::MoveDrag);
        w.begin_move_drag();
    }
}

fn trigger_double_click(window: Option<&WindowHandle>) {
    if let Some(w) = window {
        #[cfg(test)]
        record_test_action(DragAction::DoubleClick);
        w.title_bar_double_click();
    }
}

fn trigger_window_menu(window: Option<&WindowHandle>, position: Point<CssPx, Css>) {
    if let Some(w) = window
        && !w.server_decorated().get_untracked()
    {
        #[cfg(test)]
        record_test_action(DragAction::WindowMenu(position));
        super::show_window_menu(w, position);
    }
}

fn install_root_listeners(
    bar_ref: NodeRef,
    window: &Option<WindowHandle>,
    state: &Rc<DragSurfaceState>,
    guards: &Rc<RefCell<Vec<ListenerGuard>>>,
) {
    if !guards.borrow().is_empty() || bar_ref.get_untracked().is_none() {
        return;
    }
    let Some(root) = bar_ref.window_root() else {
        return;
    };

    let mut installed = Vec::with_capacity(5);

    let down_state = Rc::clone(state);
    if let Some(g) = root.listen(
        events::POINTER_DOWN,
        ListenerOptions::CAPTURE,
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            if matches!(down_state.phase.get(), DragPhase::Moved(_)) {
                down_state.phase.set(DragPhase::Idle);
            }
            let inside = bar_ref
                .get_untracked()
                .is_some_and(|bar_id| bar_id == ev.target || bar_ref.contains(ev.target));
            let on_text_entry =
                zgui::view::current_host().is_some_and(|host| host.is_text_entry(ev.target));
            if !inside || on_text_entry {
                down_state.phase.set(DragPhase::Idle);
                down_state.last_empty_press.set(None);
            }
        },
    ) {
        installed.push(g);
    }

    let move_state = Rc::clone(state);
    let move_window = window.clone();
    if let Some(g) = root.listen(
        events::POINTER_MOVE,
        ListenerOptions::CAPTURE,
        move |ev: &mut EventCx<'_, events::PointerMove>| {
            let DragPhase::Armed(anchor) = move_state.phase.get() else {
                return;
            };
            if ev.id != anchor.pointer_id {
                return;
            }
            let dx = ev.position.x.0 - anchor.origin.x.0;
            let dy = ev.position.y.0 - anchor.origin.y.0;
            if dx.hypot(dy) < DRAG_THRESHOLD {
                return;
            }
            move_state.phase.set(DragPhase::Moved(anchor.pointer_id));
            move_state.last_empty_press.set(None);
            trigger_move_drag(move_window.as_ref());
            ev.stop_propagation();
        },
    ) {
        installed.push(g);
    }

    let up_state = Rc::clone(state);
    if let Some(g) = root.listen(
        events::POINTER_UP,
        ListenerOptions::CAPTURE,
        move |ev: &mut EventCx<'_, events::PointerUp>| match up_state.phase.get() {
            DragPhase::Armed(anchor) if ev.id == anchor.pointer_id => {
                up_state.phase.set(DragPhase::Idle);
            }
            DragPhase::Moved(id) if ev.id == id => {
                up_state.phase.set(DragPhase::Idle);
                ev.prevent_default();
                ev.stop_propagation();
            }
            _ => {}
        },
    ) {
        installed.push(g);
    }

    let cancel_state = Rc::clone(state);
    if let Some(g) = root.listen(
        events::POINTER_CANCEL,
        ListenerOptions::CAPTURE,
        move |ev: &mut EventCx<'_, events::PointerCancel>| match cancel_state.phase.get() {
            DragPhase::Armed(anchor) if ev.id == anchor.pointer_id => {
                cancel_state.phase.set(DragPhase::Idle);
            }
            DragPhase::Moved(id) if ev.id == id => {
                cancel_state.phase.set(DragPhase::Idle);
            }
            _ => {}
        },
    ) {
        installed.push(g);
    }

    let key_state = Rc::clone(state);
    if let Some(g) = root.listen(
        events::KEY_DOWN,
        ListenerOptions::CAPTURE,
        move |_: &mut EventCx<'_, events::KeyDown>| {
            key_state.last_empty_press.set(None);
            if matches!(key_state.phase.get(), DragPhase::Moved(_)) {
                key_state.phase.set(DragPhase::Idle);
            }
        },
    ) {
        installed.push(g);
    }

    *guards.borrow_mut() = installed;
}

/// Attaches threshold-based top-bar window dragging, empty-area double-press maximize, and
/// empty-area secondary-click window menu to `bar_ref`.
///
/// Primary presses anywhere on the top bar except text-entry inputs arm a window move once the
/// pointer travels [`DRAG_THRESHOLD`] CSS pixels (even if it leaves the bar vertically), while
/// clicks within the threshold still activate buttons, breadcrumbs, and address/rename controls.
pub(crate) fn attach_window_drag(
    bar_ref: NodeRef,
    window: Option<WindowHandle>,
) -> impl Fn(&mut EventCx<'_, events::PointerDown>) {
    let state = Rc::new(DragSurfaceState::default());
    let guards: Rc<RefCell<Vec<ListenerGuard>>> = Rc::new(RefCell::new(Vec::new()));

    let attach_fx = RenderEffect::new({
        let state = Rc::clone(&state);
        let guards = Rc::clone(&guards);
        let window = window.clone();
        move |_| {
            if bar_ref.get().is_none() || !guards.borrow().is_empty() {
                return;
            }
            install_root_listeners(bar_ref, &window, &state, &guards);
        }
    });

    let focus_fx = window.as_ref().map(|win| {
        let focused = win.focused();
        let state = Rc::clone(&state);
        RenderEffect::new(move |_| {
            if !focused.get() {
                state.phase.set(DragPhase::Idle);
                state.last_empty_press.set(None);
            }
        })
    });

    let cleanup_guards = Rc::clone(&guards);
    on_cleanup_local(move || {
        drop(attach_fx);
        drop(focus_fx);
        cleanup_guards.borrow_mut().clear();
    });

    move |ev: &mut EventCx<'_, events::PointerDown>| {
        if is_text_entry_descendant(ev.current, ev.target) {
            state.phase.set(DragPhase::Idle);
            state.last_empty_press.set(None);
            return;
        }
        let interactive = is_interactive_descendant(ev.current, ev.target);
        match ev.button {
            Some(PointerButton::Secondary) => {
                state.phase.set(DragPhase::Idle);
                state.last_empty_press.set(None);
                if !interactive {
                    trigger_window_menu(window.as_ref(), ev.position);
                    ev.stop_propagation();
                }
            }
            Some(PointerButton::Primary) if ev.primary => {
                install_root_listeners(bar_ref, &window, &state, &guards);
                if !interactive {
                    let is_double = state.last_empty_press.get().is_some_and(|(when, prev)| {
                        ev.timestamp.saturating_since(when) <= DOUBLE_PRESS_WINDOW
                            && (prev.x.0 - ev.position.x.0).abs() <= DOUBLE_PRESS_SLOP
                            && (prev.y.0 - ev.position.y.0).abs() <= DOUBLE_PRESS_SLOP
                    });
                    if is_double {
                        state.last_empty_press.set(None);
                        state.phase.set(DragPhase::Idle);
                        trigger_double_click(window.as_ref());
                        ev.stop_propagation();
                        return;
                    }
                    state
                        .last_empty_press
                        .set(Some((ev.timestamp, ev.position)));
                } else {
                    state.last_empty_press.set(None);
                }
                state.phase.set(DragPhase::Armed(DragAnchor {
                    pointer_id: ev.id,
                    origin: ev.position,
                }));
            }
            _ => {}
        }
    }
}
