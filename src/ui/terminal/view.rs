//! Reusable interactive terminal view with font grid, IME, selection, and clipboard.

pub(super) mod fonts;
pub(super) mod grid;
pub(super) mod ime;
pub(crate) mod palette;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use zgui::app::Fonts;
use zgui::custom::custom;
use zgui::geom::{Css, CssPx, Point};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, RenderEffect, RwSignal, Signal, StoredValue, UnsyncCallback, on_cleanup_local,
    use_context, use_local_context,
};
use zgui::view::install_stylesheet;
use zgui::view::time::{IntervalHandle, set_interval};
use zgui::vocab::{ImeEvent, Key, Modifiers, NamedKey, PointerButton, ScrollDelta, Timestamp};
use zgui::{component, view};

use super::Terminal;
use super::input;
use crate::app::Gds;
use crate::service::terminal::Status;
use grid::{GridGeometry, TerminalGrid};
use ime::PreeditState;
use palette::Palette;

const SHEET: &str = "gds-terminal";
const BLINK_INTERVAL: Duration = Duration::from_millis(600);
const DOUBLE_CLICK_TIMEOUT: Duration = Duration::from_millis(450);

pub(super) const TERMINAL_CSS: &str = "
.gds-terminal-root {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    position: relative;
    overflow: hidden;
    background: transparent;
    outline: none;
    box-shadow: none;
    cursor: text;
}
.gds-terminal-root:focus,
.gds-terminal-root:focus-visible {
    outline: none;
    box-shadow: none;
}
.gds-terminal-canvas-host {
    flex: 1 1 0;
    width: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    background: transparent;
}
.gds-terminal-canvas {
    width: 100%;
    height: 100%;
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    display: block;
    background: transparent;
}
.gds-terminal-status-line {
    padding: 2px 12px;
    font-size: var(--gds-text-xs, 12px);
    background-color: transparent;
    color: var(--gds-muted-foreground);
}
.gds-terminal-root[data-paint-surface=\"true\"] .gds-terminal-status-line {
    background-color: var(--gds-content-bg, var(--gds-content));
}
.gds-terminal-error-line {
    padding: 2px 12px;
    font-size: var(--gds-text-xs, 12px);
    background-color: transparent;
    color: var(--gds-error);
}
.gds-terminal-root[data-paint-surface=\"true\"] .gds-terminal-error-line {
    background-color: var(--gds-content-bg, var(--gds-content));
}
";

#[cfg(test)]
thread_local! {
    static TEST_CANVAS_HOST_REF: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn last_test_canvas_host_ref() -> Option<NodeRef> {
    TEST_CANVAS_HOST_REF.with(|slot| *slot.borrow())
}

#[derive(Clone, Copy)]
struct ClickTracker {
    last_time: Option<Timestamp>,
    last_pos: Point<CssPx, Css>,
    last_button: Option<PointerButton>,
    count: u8,
}

impl Default for ClickTracker {
    fn default() -> Self {
        Self {
            last_time: None,
            last_pos: Point::new(CssPx(0.0), CssPx(0.0)),
            last_button: None,
            count: 0,
        }
    }
}

impl ClickTracker {
    fn press(
        &mut self,
        button: Option<PointerButton>,
        pos: Point<CssPx, Css>,
        now: Timestamp,
    ) -> u8 {
        let within_distance =
            (pos.x.0 - self.last_pos.x.0).hypot(pos.y.0 - self.last_pos.y.0) <= 4.0;
        let within_time = self
            .last_time
            .is_some_and(|prev| now.saturating_since(prev) <= DOUBLE_CLICK_TIMEOUT);
        if button.is_some() && button == self.last_button && within_time && within_distance {
            self.count = (self.count % 3) + 1;
        } else {
            self.count = 1;
        }
        self.last_time = Some(now);
        self.last_pos = pos;
        self.last_button = button;
        self.count
    }
}

#[derive(Default)]
struct PointerState {
    selecting: bool,
    pressed_button: Option<u8>,
    clicks: ClickTracker,
    scroll_remainder: f32,
}

fn mouse_button_code(button: Option<PointerButton>) -> u8 {
    match button {
        Some(PointerButton::Primary) => 0,
        Some(PointerButton::Middle) => 1,
        Some(PointerButton::Secondary) => 2,
        _ => 3,
    }
}

fn is_copy_shortcut(key: &Key, unmodified: &Key, modifiers: Modifiers) -> bool {
    let matches_c = matches!(unmodified, Key::Character(c) if c.eq_ignore_ascii_case("c"))
        || matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("c"));
    if !matches_c {
        return false;
    }
    if cfg!(target_os = "macos") {
        (modifiers.meta() && !modifiers.control() && !modifiers.alt())
            || (modifiers.control() && modifiers.shift() && !modifiers.alt() && !modifiers.meta())
    } else {
        modifiers.control() && modifiers.shift() && !modifiers.alt() && !modifiers.meta()
    }
}

fn is_paste_shortcut(key: &Key, unmodified: &Key, modifiers: Modifiers) -> bool {
    if modifiers.shift()
        && !modifiers.control()
        && !modifiers.alt()
        && !modifiers.meta()
        && *key == Key::Named(NamedKey::Insert)
    {
        return true;
    }
    let matches_v = matches!(unmodified, Key::Character(v) if v.eq_ignore_ascii_case("v"))
        || matches!(key, Key::Character(v) if v.eq_ignore_ascii_case("v"));
    if !matches_v {
        return false;
    }
    if cfg!(target_os = "macos") {
        (modifiers.meta() && !modifiers.control() && !modifiers.alt())
            || (modifiers.control() && modifiers.shift() && !modifiers.alt() && !modifiers.meta())
    } else {
        modifiers.control() && modifiers.shift() && !modifiers.alt() && !modifiers.meta()
    }
}

fn scroll_shortcut(key: &Key, modifiers: Modifiers) -> Option<Scroll> {
    if !modifiers.shift() || modifiers.control() || modifiers.alt() || modifiers.meta() {
        return None;
    }
    match key {
        Key::Named(NamedKey::PageUp) => Some(Scroll::PageUp),
        Key::Named(NamedKey::PageDown) => Some(Scroll::PageDown),
        Key::Named(NamedKey::Home) => Some(Scroll::Top),
        Key::Named(NamedKey::End) => Some(Scroll::Bottom),
        _ => None,
    }
}

fn pointer_local_dev(
    canvas_ref: NodeRef,
    event_pos: Point<CssPx, Css>,
    geometry: &GridGeometry,
    fallback_bounds: Option<zgui::geom::Rect<zgui::geom::DevicePx, zgui::geom::Device>>,
) -> (f32, f32) {
    let bounds = canvas_ref
        .window_bounds()
        .or(fallback_bounds)
        .unwrap_or_default();
    let scale = geometry.scale.max(1.0);
    (
        event_pos.x.0 * scale - bounds.origin.x.0,
        event_pos.y.0 * scale - bounds.origin.y.0,
    )
}

fn ime_caret_css(
    geometry: GridGeometry,
    session: Option<&crate::service::terminal::Session>,
    preedit: &PreeditState,
) -> (f32, f32, f32) {
    let scale = geometry.scale.max(1.0);
    let (cursor_line, cursor_col, display_offset) = session
        .map(|s| {
            s.with_term(|term| {
                let grid = term.grid();
                (
                    grid.cursor.point.line.0,
                    grid.cursor.point.column.0,
                    grid.display_offset(),
                )
            })
        })
        .unwrap_or((0, 0, geometry.display_offset));

    let max_row = geometry.rows.saturating_sub(1) as i32;
    let row = (cursor_line + display_offset as i32).clamp(0, max_row.max(0)) as usize;
    let max_col = geometry.columns.saturating_sub(1);
    let col = if preedit.is_empty() {
        cursor_col.min(max_col)
    } else {
        let comp_col = geometry.composition_column(cursor_col, &preedit.text);
        let caret_cols = unicode_width::UnicodeWidthStr::width(&preedit.text[..preedit.cursor.end]);
        comp_col.saturating_add(caret_cols).min(max_col)
    };

    let x_css = ((geometry.origin_x + col as f32 * geometry.cell_width) / scale).max(0.0);
    let y_css = ((geometry.origin_y + row as f32 * geometry.cell_height) / scale).max(0.0);
    let h_css = (geometry.cell_height / scale).max(1.0);
    (x_css, y_css, h_css)
}

/// Interactive terminal pane bound to a [`Terminal`] handle.
#[component]
pub fn TerminalView(
    terminal: Terminal,
    #[prop(default = true)] paint_surface: bool,
    #[prop(optional)] on_key: Option<UnsyncCallback<(Key, Modifiers), bool>>,
    #[prop(into, optional)] drag_active: Option<Signal<bool, LocalStorage>>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    install_stylesheet(SHEET, TERMINAL_CSS);

    let root_ref = node_ref.unwrap_or_default();
    let canvas_ref = NodeRef::new();
    #[cfg(test)]
    TEST_CANVAS_HOST_REF.with(|slot| *slot.borrow_mut() = Some(canvas_ref));
    terminal.register_focus_target(Some(root_ref));
    let term_for_focus = terminal.clone();
    let focus_effect = RenderEffect::new(move |_| {
        if root_ref.get().is_some() {
            term_for_focus.apply_pending_focus();
        }
    });
    let _focus_effect_store = StoredValue::new_local(focus_effect);

    let gds = Gds::try_current();
    let fonts = use_context::<Fonts>().unwrap_or_else(Fonts::system);
    let clipboard = try_use_clipboard();
    let context_path_drag =
        use_local_context::<crate::ui::drag::DragOverlayState<Vec<std::path::PathBuf>>>();

    let initial_prefs = terminal.preferences().get_untracked();
    let focused_sig = terminal.focused_signal();
    let cursor_on = RwSignal::new_local(true);
    let preedit_sig = RwSignal::new_local(PreeditState::default());

    #[cfg(not(test))]
    let shared_geometry = Rc::new(std::cell::Cell::new(GridGeometry::default()));
    #[cfg(test)]
    let shared_geometry = terminal.test_geometry_cell();
    let pointer_state = Rc::new(RefCell::new(PointerState::default()));
    let blink_handle: Rc<RefCell<Option<IntervalHandle>>> = Rc::new(RefCell::new(None));

    let (grid_element, grid_handle) = custom(TerminalGrid::new(
        terminal.session(),
        Palette::fallback(),
        paint_surface,
        fonts.shaper(),
        initial_prefs,
        focused_sig.get_untracked(),
        false,
        Rc::clone(&shared_geometry),
    ));
    let grid_element = grid_element.class("gds-terminal-canvas");
    let handle_store = StoredValue::new_local(grid_handle);

    let canvas_size = canvas_ref.observe_border_size();
    let ime_caret = Signal::derive_local({
        let terminal = terminal.clone();
        let geom_ref = Rc::clone(&shared_geometry);
        move || {
            let _ = canvas_size.get();
            let _ = terminal.redraw_revision().get();
            let _ = terminal.status().get();
            let _ = terminal.preferences().get();
            let preedit = preedit_sig.get();
            let session = terminal.session();
            ime_caret_css(geom_ref.get(), session.as_ref(), &preedit)
        }
    });

    let term_for_cleanup = terminal.clone();
    let blink_for_cleanup = Rc::clone(&blink_handle);
    on_cleanup_local(move || {
        #[cfg(test)]
        TEST_CANVAS_HOST_REF.with(|slot| {
            if *slot.borrow() == Some(canvas_ref) {
                *slot.borrow_mut() = None;
            }
        });
        term_for_cleanup.register_focus_target(None);
        *blink_for_cleanup.borrow_mut() = None;
    });

    // Keep cursor blink timer synchronized with focus, user preference, and reduced-motion policy.
    let term_for_blink = terminal.clone();
    let blink_for_effect = Rc::clone(&blink_handle);
    let gds_for_blink = gds.clone();
    let blink_effect = RenderEffect::new(move |_| {
        let is_focused = focused_sig.get();
        let blink_enabled = term_for_blink.preferences().get().cursor_blink;
        let reduced = gds_for_blink
            .as_ref()
            .is_some_and(|app| app.appearance().motion().reduced());
        let should_blink = is_focused && blink_enabled && !reduced;

        if should_blink {
            if blink_for_effect.borrow().is_none() {
                let next_blink = set_interval(BLINK_INTERVAL, move || {
                    cursor_on.update(|on| *on = !*on);
                });
                *blink_for_effect.borrow_mut() = Some(next_blink);
            }
        } else {
            *blink_for_effect.borrow_mut() = None;
            if !cursor_on.get_untracked() {
                cursor_on.set(true);
            }
        }
        #[cfg(test)]
        term_for_blink.set_test_cursor(
            is_focused,
            blink_for_effect.borrow().is_some(),
            cursor_on.get_untracked(),
        );
    });
    let _blink_effect_store = StoredValue::new_local(blink_effect);

    // Push session, preferences, theme palette, cursor state, and IME preedit changes to the grid.
    let term_for_grid = terminal.clone();
    let gds_for_grid = gds.clone();
    let grid_effect = RenderEffect::new(move |_| {
        let _rev = term_for_grid.redraw_revision().get();
        let _status = term_for_grid.status().get();
        let prefs = term_for_grid.preferences().get();
        let is_focused = focused_sig.get();
        let blink_visible = cursor_on.get();
        let preedit = preedit_sig.get();
        let reduced = gds_for_grid
            .as_ref()
            .is_some_and(|app| app.appearance().motion().reduced());
        if let Some(gds) = &gds_for_grid {
            gds.appearance().theme().prepared().with(|_| ());
        }
        let palette = Palette::current();
        let session = term_for_grid.session();

        handle_store.with_value(|handle| {
            let mut needs_relayout = false;
            handle.update(|grid| {
                if grid.preferences != prefs {
                    grid.preferences = prefs;
                    needs_relayout = true;
                }
                grid.session = session;
                grid.palette = palette;
                grid.paint_surface = paint_surface;
                grid.focused = is_focused;
                grid.cursor_on = blink_visible;
                grid.reduce_motion = reduced;
                grid.preedit = preedit;
                grid.sync_pty_size();
            });
            if needs_relayout {
                handle.relayout();
            }
            handle.repaint();
        });
        #[cfg(test)]
        term_for_grid.set_test_cursor(is_focused, blink_handle.borrow().is_some(), blink_visible);
    });
    let _grid_effect_store = StoredValue::new_local(grid_effect);

    let send_input = {
        let terminal = terminal.clone();
        move |bytes: &[u8]| {
            if bytes.is_empty() {
                return;
            }
            if let Some(session) = terminal.session() {
                session.with_term(|term| {
                    term.scroll_display(Scroll::Bottom);
                    term.selection = None;
                });
                if session.is_running() {
                    terminal.write(bytes);
                }
            }
            cursor_on.set(true);
            terminal.request_redraw();
        }
    };

    let on_focus_in = {
        let terminal = terminal.clone();
        move |_: &mut EventCx<'_, zgui::view::events::FocusIn>| {
            focused_sig.set(true);
            cursor_on.set(true);
            if let Some(session) = terminal.session()
                && session.with_term(|term| term.mode().contains(TermMode::FOCUS_IN_OUT))
                && session.is_running()
            {
                terminal.write(b"\x1b[I");
            }
            terminal.request_redraw();
        }
    };

    let on_focus_out = {
        let terminal = terminal.clone();
        let pointer = Rc::clone(&pointer_state);
        move |_: &mut EventCx<'_, zgui::view::events::FocusOut>| {
            focused_sig.set(false);
            cursor_on.set(true);
            preedit_sig.update(PreeditState::clear);
            {
                let mut state = pointer.borrow_mut();
                state.selecting = false;
                state.pressed_button = None;
            }
            if let Some(session) = terminal.session()
                && session.with_term(|term| term.mode().contains(TermMode::FOCUS_IN_OUT))
                && session.is_running()
            {
                terminal.write(b"\x1b[O");
            }
            terminal.request_redraw();
        }
    };

    let on_key_down = {
        let terminal = terminal.clone();
        let send_input = send_input.clone();
        let clipboard = clipboard.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::KeyDown>| {
            if let Some(cb) = on_key
                && cb.run((ev.key.clone(), ev.modifiers))
            {
                ev.stop_propagation();
                ev.prevent_default();
                return;
            }

            if !preedit_sig.with_untracked(PreeditState::is_empty) {
                ev.prevent_default();
                return;
            }

            if is_copy_shortcut(&ev.key, &ev.key_without_modifiers, ev.modifiers) {
                if let Some(session) = terminal.session()
                    && let Some(text) = session.with_term(|term| term.selection_to_string())
                    && let Some(clip) = clipboard.as_ref()
                {
                    clip.set_text(ClipboardKind::Standard, text);
                }
                ev.stop_propagation();
                ev.prevent_default();
                return;
            }

            if is_paste_shortcut(&ev.key, &ev.key_without_modifiers, ev.modifiers) {
                if let Some(clip) = clipboard.as_ref() {
                    let term = terminal.clone();
                    clip.read_text(ClipboardKind::Standard, move |maybe_text| {
                        if let Some(text) = maybe_text {
                            term.paste_text(&text);
                            term.request_redraw();
                        }
                    });
                }
                ev.stop_propagation();
                ev.prevent_default();
                return;
            }

            if let Some(scroll) = scroll_shortcut(&ev.key, ev.modifiers) {
                if let Some(session) = terminal.session() {
                    session.with_term(|term| term.scroll_display(scroll));
                    terminal.request_redraw();
                }
                ev.stop_propagation();
                ev.prevent_default();
                return;
            }

            let mode = terminal.session().map_or_else(TermMode::empty, |session| {
                session.with_term(|term| *term.mode())
            });
            let alt_escape = terminal.preferences().get_untracked().alt_sends_escape;
            if let Some(bytes) = input::key_bytes(ev, ev.modifiers, mode, alt_escape) {
                send_input(&bytes);
                ev.stop_propagation();
                ev.prevent_default();
            }
        }
    };

    let on_text = {
        let send_input = send_input.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::Text>| {
            if !preedit_sig.with_untracked(PreeditState::is_empty) || ev.text.is_empty() {
                return;
            }
            send_input(ev.text.as_bytes());
            ev.stop_propagation();
            ev.prevent_default();
        }
    };

    let on_ime_start = move |ev: &mut EventCx<'_, zgui::view::events::ImeStart>| {
        preedit_sig.update(PreeditState::clear);
        ev.stop_propagation();
        ev.prevent_default();
    };

    let on_ime_preedit = {
        let terminal = terminal.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::ImePreedit>| {
            if let ImeEvent::Preedit { text, cursor } = &**ev {
                preedit_sig.update(|state| state.set(text.as_str(), cursor.clone()));
                terminal.request_redraw();
            }
            ev.stop_propagation();
            ev.prevent_default();
        }
    };

    let on_ime_commit = {
        let send_input = send_input.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::ImeCommit>| {
            preedit_sig.update(PreeditState::clear);
            if let ImeEvent::Commit(text) = &**ev
                && !text.is_empty()
            {
                send_input(text.as_bytes());
            }
            ev.stop_propagation();
            ev.prevent_default();
        }
    };

    let on_ime_end = {
        let terminal = terminal.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::ImeEnd>| {
            preedit_sig.update(PreeditState::clear);
            terminal.request_redraw();
            ev.stop_propagation();
            ev.prevent_default();
        }
    };

    let on_pointer_down = {
        let terminal = terminal.clone();
        let geom_ref = Rc::clone(&shared_geometry);
        let pointer = Rc::clone(&pointer_state);
        let clipboard = clipboard.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::PointerDown>| {
            root_ref.focus();
            ev.capture_pointer();

            let geometry = geom_ref.get();
            let (local_x, local_y) =
                pointer_local_dev(canvas_ref, ev.position, &geometry, ev.bounds());
            let mut state = pointer.borrow_mut();
            let click_count = state.clicks.press(ev.button, ev.position, ev.timestamp);
            let button = mouse_button_code(ev.button);
            state.pressed_button = Some(button);

            if let Some(session) = terminal.session() {
                let (mode, display_offset) =
                    session.with_term(|term| (*term.mode(), term.grid().display_offset()));
                let (point, side, column, row) =
                    grid::grid_point(geometry, display_offset, local_x, local_y);
                if mode.intersects(TermMode::MOUSE_MODE) && !ev.modifiers.shift() {
                    if let Some(bytes) =
                        input::mouse_bytes(button, column, row, false, ev.modifiers, mode)
                        && session.is_running()
                    {
                        terminal.write(&bytes);
                    }
                    state.selecting = false;
                } else if ev.button == Some(PointerButton::Primary) {
                    let ty = match click_count {
                        2 => SelectionType::Semantic,
                        3.. => SelectionType::Lines,
                        _ if ev.modifiers.alt() => SelectionType::Block,
                        _ => SelectionType::Simple,
                    };
                    session.with_term(|term| {
                        term.selection = Some(Selection::new(ty, point, side));
                    });
                    state.selecting = true;
                } else if ev.button == Some(PointerButton::Middle)
                    && let Some(clip) = clipboard.as_ref()
                {
                    let term = terminal.clone();
                    clip.read_text(ClipboardKind::Primary, move |maybe_text| {
                        if let Some(text) = maybe_text {
                            term.paste_text(&text);
                            term.request_redraw();
                        }
                    });
                    state.selecting = false;
                }
            }
            terminal.request_redraw();
            ev.stop_propagation();
        }
    };

    let on_pointer_move = {
        let terminal = terminal.clone();
        let geom_ref = Rc::clone(&shared_geometry);
        let pointer = Rc::clone(&pointer_state);
        move |ev: &mut EventCx<'_, zgui::view::events::PointerMove>| {
            let geometry = geom_ref.get();
            let (local_x, local_y) =
                pointer_local_dev(canvas_ref, ev.position, &geometry, ev.bounds());
            let state = pointer.borrow();
            let Some(session) = terminal.session() else {
                return;
            };
            if state.selecting {
                session.with_term(|term| {
                    let display_offset = term.grid().display_offset();
                    let (point, side, _, _) =
                        grid::grid_point(geometry, display_offset, local_x, local_y);
                    if let Some(selection) = term.selection.as_mut() {
                        selection.update(point, side);
                    }
                    if local_y < geometry.origin_y {
                        term.scroll_display(Scroll::Delta(1));
                    } else if local_y > geometry.bounds_height - geometry.origin_y {
                        term.scroll_display(Scroll::Delta(-1));
                    }
                });
                terminal.request_redraw();
            } else if !ev.modifiers.shift() {
                let (mode, display_offset) =
                    session.with_term(|term| (*term.mode(), term.grid().display_offset()));
                if mode.contains(TermMode::MOUSE_MOTION)
                    || (mode.contains(TermMode::MOUSE_DRAG) && state.pressed_button.is_some())
                {
                    let (_, _, column, row) =
                        grid::grid_point(geometry, display_offset, local_x, local_y);
                    let report_button = 32 + state.pressed_button.unwrap_or(3);
                    if let Some(bytes) =
                        input::mouse_bytes(report_button, column, row, false, ev.modifiers, mode)
                        && session.is_running()
                    {
                        terminal.write(&bytes);
                    }
                }
            }
        }
    };

    let on_pointer_up = {
        let terminal = terminal.clone();
        let geom_ref = Rc::clone(&shared_geometry);
        let pointer = Rc::clone(&pointer_state);
        let clipboard = clipboard.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::PointerUp>| {
            let active_drag = drag_active.is_some_and(|sig| sig.get_untracked())
                || context_path_drag.is_some_and(|drag| drag.is_active().get_untracked());
            let (was_selecting, pressed) = {
                let mut state = pointer.borrow_mut();
                let selecting = state.selecting;
                let button = state.pressed_button.take();
                state.selecting = false;
                (selecting, button)
            };
            // File drags belong to the host's drop target rather than PTY mouse reporting;
            // let PointerUp bubble so the enclosing drop surface receives the payload.
            if active_drag || (!was_selecting && pressed.is_none()) {
                return;
            }

            ev.release_pointer();
            let geometry = geom_ref.get();
            let (local_x, local_y) =
                pointer_local_dev(canvas_ref, ev.position, &geometry, ev.bounds());

            if let Some(session) = terminal.session() {
                if was_selecting {
                    if let Some(text) = session.with_term(|term| term.selection_to_string())
                        && let Some(clip) = clipboard.as_ref()
                    {
                        clip.set_text(ClipboardKind::Primary, text);
                    }
                } else {
                    let (mode, display_offset) =
                        session.with_term(|term| (*term.mode(), term.grid().display_offset()));
                    if mode.intersects(TermMode::MOUSE_MODE) && !ev.modifiers.shift() {
                        let (_, _, column, row) =
                            grid::grid_point(geometry, display_offset, local_x, local_y);
                        let button = mouse_button_code(ev.button);
                        if let Some(bytes) =
                            input::mouse_bytes(button, column, row, true, ev.modifiers, mode)
                            && session.is_running()
                        {
                            terminal.write(&bytes);
                        }
                    }
                }
            }
            terminal.request_redraw();
            ev.stop_propagation();
        }
    };

    let on_pointer_cancel = {
        let pointer = Rc::clone(&pointer_state);
        move |ev: &mut EventCx<'_, zgui::view::events::PointerCancel>| {
            ev.release_pointer();
            let mut state = pointer.borrow_mut();
            state.selecting = false;
            state.pressed_button = None;
        }
    };

    let on_wheel = {
        let terminal = terminal.clone();
        let geom_ref = Rc::clone(&shared_geometry);
        let pointer = Rc::clone(&pointer_state);
        move |ev: &mut EventCx<'_, zgui::view::events::Wheel>| {
            let geometry = geom_ref.get();
            let (local_x, local_y) =
                pointer_local_dev(canvas_ref, ev.position, &geometry, ev.bounds());
            // zgui reports negative Y for wheel-up; Alacritty positive Delta scrolls up into history.
            let cell_css_h = (geometry.cell_height / geometry.scale.max(1.0)).max(1.0);
            let delta = match ev.delta {
                ScrollDelta::Lines { y, .. } => -y,
                ScrollDelta::Pixels(pixels) => -pixels.height.0 / cell_css_h,
                _ => 0.0,
            };
            let mut state = pointer.borrow_mut();
            state.scroll_remainder += delta;
            let lines = state.scroll_remainder.trunc() as i32;
            state.scroll_remainder -= lines as f32;
            if lines == 0 {
                ev.stop_propagation();
                return;
            }

            if let Some(session) = terminal.session() {
                let (mode, display_offset) =
                    session.with_term(|term| (*term.mode(), term.grid().display_offset()));
                let (_, _, column, row) =
                    grid::grid_point(geometry, display_offset, local_x, local_y);
                if mode.intersects(TermMode::MOUSE_MODE) && !ev.modifiers.shift() {
                    let button = if lines > 0 { 64 } else { 65 };
                    for _ in 0..lines.unsigned_abs().min(100) {
                        if let Some(bytes) =
                            input::mouse_bytes(button, column, row, false, ev.modifiers, mode)
                            && session.is_running()
                        {
                            terminal.write(&bytes);
                        }
                    }
                } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
                    && !ev.modifiers.shift()
                {
                    let seq: &[u8] = if lines > 0 { b"\x1bOA" } else { b"\x1bOB" };
                    for _ in 0..lines.unsigned_abs().min(100) {
                        if session.is_running() {
                            terminal.write(seq);
                        }
                    }
                } else {
                    session.with_term(|term| term.scroll_display(Scroll::Delta(lines)));
                }
                terminal.request_redraw();
            }
            ev.stop_propagation();
        }
    };

    let on_drop = {
        let terminal = terminal.clone();
        move |ev: &mut EventCx<'_, zgui::view::events::Drop>| {
            if !ev.paths.is_empty() {
                terminal.paste_paths(&ev.paths);
                root_ref.focus();
                terminal.request_redraw();
                ev.stop_propagation();
            }
        }
    };

    let status_sig = terminal.status();
    let error_sig = terminal.error();
    let status_lines_view = move || {
        let status = status_sig.get();
        let error = error_sig.get();
        let non_duplicate_error =
            error.filter(|err| !matches!(&status, Status::Failed(failed_msg) if failed_msg == err));
        let status_notice = match status {
            Status::Starting | Status::Running => None,
            Status::Stopping => Some((crate::i18n::text("gds-terminal-stopping"), false)),
            Status::Closed => Some((crate::i18n::text("gds-terminal-exited"), false)),
            Status::Failed(msg) => Some((
                format!("{}: {msg}", crate::i18n::text("gds-terminal-io-failed")),
                true,
            )),
            Status::Exited { code, signal } => Some((
                crate::t!(
                    "gds-terminal-exit-status",
                    code = code,
                    signal = signal.unwrap_or_else(|| "—".to_owned())
                ),
                false,
            )),
        };

        if non_duplicate_error.is_none() && status_notice.is_none() {
            return None;
        }
        Some(view! {
            column {
                {non_duplicate_error.map(|err| view! {
                    box(class = "gds-terminal-error-line", attr:data-testid = "terminal-error") {
                        text {{err}}
                    }
                })}
                {status_notice.map(|(label, is_err)| view! {
                    box(
                        class = if is_err {
                            "gds-terminal-error-line"
                        } else {
                            "gds-terminal-status-line"
                        },
                        attr:data-testid = "terminal-status",
                    ) {
                        text {{label}}
                    }
                })}
            }
        })
    };

    view! {
        box(
            node_ref = root_ref,
            tabindex = {Focus::Sequential},
            class = "gds-terminal-root",
            class = class,
            attr:data-paint-surface = paint_surface.then(|| "true".to_owned()),
            attr:data-zgui-ime = "true",
            attr:data-zgui-ime-x = move || Some(format!("{:.2}", ime_caret.get().0)),
            attr:data-zgui-ime-y = move || Some(format!("{:.2}", ime_caret.get().1)),
            attr:data-zgui-ime-h = move || Some(format!("{:.2}", ime_caret.get().2)),
            on:focus_in = on_focus_in,
            on:focus_out = on_focus_out,
            on:key_down = on_key_down,
            on:text = on_text,
            on:ime_start = on_ime_start,
            on:ime_preedit = on_ime_preedit,
            on:ime_commit = on_ime_commit,
            on:ime_end = on_ime_end,
            on:pointer_down = on_pointer_down,
            on:pointer_move = on_pointer_move,
            on:pointer_up = on_pointer_up,
            on:pointer_cancel = on_pointer_cancel,
            on:wheel = on_wheel,
            on:drop = on_drop,
        ) {
            box(node_ref = canvas_ref, class = "gds-terminal-canvas-host") {
                {grid_element}
            }
            {status_lines_view}
        }
    }
}
