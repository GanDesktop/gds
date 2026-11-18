//! Native text and input surfaces over the existing Alacritty terminal service.
mod grid;
mod palette;
use super::Terminal;
use crate::service::terminal::input;
use alacritty_terminal::{
    grid::Scroll,
    index::{Column, Line, Point as GridPoint},
    selection::{Selection, SelectionType},
    term::TermMode,
};
use anti_webui::{core::drawing::drawing, prelude::*};
use gds_uim::el;
use grid::{Geometry, Painter, Snapshot};
pub(super) use palette::Palette;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone)]
struct Surface {
    terminal: Terminal,
    on_key: Option<Rc<dyn Fn(&KeyboardEvent) -> bool>>,
    size: Signal<(f32, f32)>,
    geometry: Memo<Geometry>,
    painter: Rc<RefCell<Painter>>,
    snapshot: Rc<RefCell<Snapshot>>,
    cursor: Signal<PointF>,
    focused: Signal<bool>,
    blink: Signal<bool>,
    input: Signal<String>,
    input_ref: NodeRef,
    selecting: Rc<Cell<bool>>,
    mouse_button: Rc<Cell<Option<u8>>>,
    last_pointer: Rc<Cell<PointF>>,
    scroll_remainder: Rc<Cell<f32>>,
}
impl Terminal {
    pub fn view(&self) -> impl IntoView {
        Surface::new(self.clone(), None).into_view()
    }
    /// Lets the containing application handle its explicit shortcuts before PTY input.
    pub fn view_with_key_handler(
        &self,
        on_key: impl Fn(&KeyboardEvent) -> bool + 'static,
    ) -> impl IntoView {
        Surface::new(self.clone(), Some(Rc::new(on_key))).into_view()
    }
}
impl Surface {
    fn new(terminal: Terminal, on_key: Option<Rc<dyn Fn(&KeyboardEvent) -> bool>>) -> Self {
        let input_ref = node_ref();
        let focused = terminal.0.focused;
        let preferences = terminal.0.preferences;
        let font = memo(move || {
            preferences.with(|prefs| {
                let style = Style::default()
                    .font_family(prefs.font_family.as_str())
                    .text_size(prefs.font_size);
                Geometry::new(0.0, 0.0, &style, window_scale())
            })
        });
        let size = signal((0.0, 0.0));
        let geometry = memo(move || {
            let (width, height) = size.get();
            font.get().with_size(width, height)
        });
        terminal.0.focus.set(Some(input_ref));
        let pending_focus = terminal.clone();
        let controller = terminal.clone();
        on_cleanup(move || {
            if controller.0.focus.get() == Some(input_ref) {
                controller.0.focus.set(None);
                if focused.is_alive() {
                    focused.set(false);
                }
            }
        });
        let surface = Self {
            terminal,
            on_key,
            size,
            geometry,
            painter: Rc::default(),
            snapshot: Rc::default(),
            cursor: signal(PointF::new(0.0, 0.0)),
            focused,
            blink: signal(true),
            input: signal(String::new()),
            input_ref,
            selecting: Rc::new(Cell::new(false)),
            mouse_button: Rc::new(Cell::new(None)),
            last_pointer: Rc::new(Cell::new(PointF::new(0.0, 0.0))),
            scroll_remainder: Rc::new(Cell::new(0.0)),
        };
        let blink = surface.blink;
        let focused = surface.focused;
        let sender = ui_channel(move |()| {
            if focused.get_untracked() && preferences.get_untracked().cursor_blink {
                blink.update(|visible| *visible = !*visible);
            } else {
                blink.set_if_changed(true);
            }
        });
        let scope = crate::app::Gds::current()
            .tasks()
            .owner_scope("Terminal cursor");
        scope
            .spawn_cancellable_named("Blink terminal cursor", async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(600));
                loop {
                    interval.tick().await;
                    sender.send(());
                }
            })
            .on_complete(&scope, |result: Result<(), _>| {
                if let Err(error) = result {
                    tracing::debug!(%error, "terminal cursor stopped");
                }
            });
        let ready = memo({
            let terminal = surface.terminal.clone();
            move || terminal.status() != super::Status::Starting
        });
        effect(move || {
            if ready.get() && pending_focus.0.focus_requested.get() {
                let terminal = pending_focus.clone();
                request_frame(move |_| {
                    if terminal.0.focus.get() == Some(input_ref) && terminal.0.focus_requested.get()
                    {
                        input_ref.focus();
                    }
                });
            }
        });
        let resized = surface.terminal.clone();
        let pty_size = memo(move || geometry.get().pty_size());
        effect(move || {
            if ready.get()
                && let Some([columns, rows, cell_width, cell_height]) = pty_size.get()
                && let Some(session) = resized.0.service.session()
                && let Err(error) =
                    session.resize_with_pixels(columns, rows, cell_width, cell_height)
            {
                resized.0.error.set_if_changed(Some(error.to_string()));
            }
        });
        surface
    }
    fn key(&self, event: &KeyboardEvent) {
        event.stop_propagation();
        if self.on_key.as_ref().is_some_and(|handler| handler(event)) {
            event.prevent_default();
            return;
        }
        if event.modifiers.ctrl && event.modifiers.shift && event.key.is_char('c') {
            if let Some(session) = self.terminal.0.service.session()
                && let Some(text) = session.with_term(|term| term.selection_to_string())
            {
                set_clipboard(ClipboardKind::Clipboard, text);
            }
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        if (event.modifiers.ctrl && event.modifiers.shift && event.key.is_char('v'))
            || (event.modifiers.shift && matches!(event.key, Key::Named(NamedKey::Insert)))
        {
            let terminal = self.terminal.clone();
            read_clipboard(
                ClipboardKind::Clipboard,
                vec![
                    "text/plain;charset=utf-8".into(),
                    "UTF8_STRING".into(),
                    "text/plain".into(),
                ],
                4 * 1024 * 1024,
            )
            .then(move |result| match result {
                Ok(offer) => {
                    if let Some(data) = offer.data {
                        terminal.paste_text(&String::from_utf8_lossy(&data.bytes));
                    }
                }
                Err(error) => terminal.0.error.set(Some(error.to_string())),
            });
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        if event.modifiers.types_text()
            && !event.modifiers.logo
            && matches!(
                event.key,
                Key::Character(_) | Key::Named(NamedKey::Space) | Key::Unidentified
            )
            && let Some(text) = event.text.as_deref().filter(|text| !text.is_empty())
        {
            self.input_bytes(text.as_bytes());
            event.prevent_default();
            return;
        }
        let Some(session) = self.terminal.0.service.session() else {
            return;
        };
        if event.modifiers.shift
            && !event.modifiers.ctrl
            && matches!(
                event.key,
                Key::Named(NamedKey::PageUp | NamedKey::PageDown | NamedKey::Home | NamedKey::End)
            )
        {
            session.with_term(|term| {
                term.scroll_display(match event.key {
                    Key::Named(NamedKey::PageUp) => Scroll::PageUp,
                    Key::Named(NamedKey::PageDown) => Scroll::PageDown,
                    Key::Named(NamedKey::Home) => Scroll::Top,
                    _ => Scroll::Bottom,
                })
            });
            self.terminal.changed();
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        let key = match &event.key {
            Key::Character(text) => text.to_lowercase(),
            Key::Named(key) => match key {
                NamedKey::Enter => "enter".into(),
                NamedKey::Tab => "tab".into(),
                NamedKey::Space => "space".into(),
                NamedKey::Backspace => "backspace".into(),
                NamedKey::Delete => "delete".into(),
                NamedKey::Escape => "escape".into(),
                NamedKey::ArrowLeft => "left".into(),
                NamedKey::ArrowRight => "right".into(),
                NamedKey::ArrowUp => "up".into(),
                NamedKey::ArrowDown => "down".into(),
                NamedKey::Home => "home".into(),
                NamedKey::End => "end".into(),
                NamedKey::PageUp => "pageup".into(),
                NamedKey::PageDown => "pagedown".into(),
                NamedKey::Insert => "insert".into(),
                NamedKey::F(number) => format!("f{number}"),
                _ => return,
            },
            Key::Unidentified => return,
        };
        let mode = session.with_term(|term| *term.mode());
        let bytes = input::key_bytes(
            &input::Keystroke {
                key: &key,
                key_char: event.text.as_deref(),
                modifiers: Self::modifiers(event.modifiers),
            },
            mode,
            self.terminal.0.preferences.get_untracked().alt_sends_escape,
        );
        if let Some(bytes) = bytes {
            self.input_bytes(&bytes);
            event.prevent_default();
        }
    }
    fn modifiers(value: Modifiers) -> input::Modifiers {
        input::Modifiers {
            shift: value.shift,
            control: value.ctrl,
            alt: value.alt,
            platform: value.logo,
            function: false,
        }
    }
    fn committed(&self, text: &str) {
        if !text.is_empty() {
            self.input_bytes(text.as_bytes());
            self.input.set(String::new());
        }
    }
    fn input_bytes(&self, bytes: &[u8]) {
        if let Some(session) = self.terminal.0.service.session()
            && session.with_term(|term| {
                let selected = term.selection.take().is_some();
                let scrolled = term.grid().display_offset() > 0;
                if scrolled {
                    term.scroll_display(Scroll::Bottom);
                }
                selected || scrolled
            })
        {
            self.terminal.changed();
        }
        self.terminal.send(bytes);
        self.blink.set_if_changed(true);
    }
    fn pointer_down(&self, event: &PointerEvent) {
        self.terminal.focus();
        self.last_pointer.set(event.local);
        let Some(session) = self.terminal.0.service.session() else {
            return;
        };
        let (column, row, side) = untrack(|| self.geometry.get()).point(event.local);
        let mode = session.with_term(|term| *term.mode());
        let button = match event.button {
            Some(PointerButton::Primary) => 0,
            Some(PointerButton::Middle) => 1,
            Some(PointerButton::Secondary) => 2,
            _ => 3,
        };
        self.mouse_button.set(Some(button));
        if mode.intersects(TermMode::MOUSE_MODE)
            && !event.modifiers.shift
            && let Some(bytes) = input::mouse_bytes(
                button,
                column,
                row,
                false,
                Self::modifiers(event.modifiers),
                mode,
            )
        {
            self.terminal.send(&bytes);
        } else if event.button == Some(PointerButton::Primary) {
            session.with_term(|term| {
                let point = GridPoint::new(
                    Line(row as i32 - term.grid().display_offset() as i32),
                    Column(column),
                );
                let kind = match event.click_count {
                    2 => SelectionType::Semantic,
                    3.. => SelectionType::Lines,
                    _ if event.modifiers.alt => SelectionType::Block,
                    _ => SelectionType::Simple,
                };
                term.selection = Some(Selection::new(kind, point, side));
            });
            self.selecting.set(true);
            self.terminal.changed();
        }
        event.stop_propagation();
    }
    fn pointer_up(&self, event: &PointerEvent) {
        self.release_pointer(event.local, event.modifiers);
        event.stop_propagation();
    }
    fn release_pointer(&self, position: PointF, modifiers: Modifiers) {
        self.selecting.set(false);
        let Some(button) = self.mouse_button.take() else {
            return;
        };
        let Some(session) = self.terminal.0.service.session() else {
            return;
        };
        let (column, row, _) = untrack(|| self.geometry.get()).point(position);
        let mode = session.with_term(|term| *term.mode());
        if mode.intersects(TermMode::MOUSE_MODE)
            && !modifiers.shift
            && let Some(bytes) =
                input::mouse_bytes(button, column, row, true, Self::modifiers(modifiers), mode)
        {
            self.terminal.send(&bytes);
        }
    }
    fn pointer_move(&self, event: &PointerEvent) {
        self.motion(event.local, event.modifiers);
    }
    fn motion(&self, position: PointF, modifiers: Modifiers) {
        self.last_pointer.set(position);
        let Some(session) = self.terminal.0.service.session() else {
            return;
        };
        let (column, row, side) = untrack(|| self.geometry.get()).point(position);
        let mode = session.with_term(|term| *term.mode());
        if !modifiers.shift
            && (mode.contains(TermMode::MOUSE_MOTION)
                || (mode.contains(TermMode::MOUSE_DRAG) && self.mouse_button.get().is_some()))
        {
            if let Some(bytes) = input::mouse_bytes(
                32 + self.mouse_button.get().unwrap_or(3),
                column,
                row,
                false,
                Self::modifiers(modifiers),
                mode,
            ) {
                self.terminal.send(&bytes);
            }
        } else if self.selecting.get() {
            session.with_term(|term| {
                let point = GridPoint::new(
                    Line(row as i32 - term.grid().display_offset() as i32),
                    Column(column),
                );
                if let Some(selection) = &mut term.selection {
                    selection.update(point, side);
                }
            });
            self.terminal.changed();
        }
    }
    fn grabbed(&self, event: &GrabEvent) {
        match event.phase {
            GrabPhase::Start | GrabPhase::Move => self.motion(event.local, event.modifiers),
            GrabPhase::End | GrabPhase::Cancel => {
                self.release_pointer(event.local, event.modifiers)
            }
        }
    }
    fn scroll(&self, event: &ScrollEvent) {
        let Some(session) = self.terminal.0.service.session() else {
            return;
        };
        let geometry = untrack(|| self.geometry.get());
        let amount = self.scroll_remainder.get()
            - event.delta_y / window_scale().as_f32() / geometry.cell_height;
        let lines = amount.trunc() as i32;
        self.scroll_remainder.set(amount - lines as f32);
        event.stop_propagation();
        if lines == 0 {
            return;
        }
        let mode = session.with_term(|term| *term.mode());
        if mode.intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            let (column, row, _) = geometry.point(self.last_pointer.get());
            for _ in 0..lines.unsigned_abs().min(100) {
                if let Some(bytes) = input::mouse_bytes(
                    if lines > 0 { 64 } else { 65 },
                    column,
                    row,
                    false,
                    Self::modifiers(event.modifiers),
                    mode,
                ) {
                    self.terminal.send(&bytes);
                }
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
            && !event.modifiers.shift
        {
            for _ in 0..lines.unsigned_abs().min(100) {
                self.terminal
                    .send(if lines > 0 { b"\x1bOA" } else { b"\x1bOB" });
            }
        } else {
            session.with_term(|term| term.scroll_display(Scroll::Delta(lines)));
            self.terminal.changed();
        }
    }
    fn input_style(&self, style: Style) -> Style {
        let position = self.cursor.get();
        let preferences = self.terminal.0.preferences.get();
        let geometry = self.geometry.get();
        style
            .left(position.x)
            .top(position.y)
            .w((geometry.width - position.x).max(geometry.cell_width))
            .h(geometry.cell_height)
            .font_family(preferences.font_family.as_str())
            .text_size(preferences.font_size)
            .line_height(LineHeight::Px(geometry.cell_height))
            .text_color(Palette::new().foreground)
    }
    fn focus_changed(&self, focused: bool) {
        if focused {
            self.terminal.0.focus_requested.set(false);
        }
        self.focused.set_if_changed(focused);
        self.blink.set_if_changed(true);
        if let Some(session) = self.terminal.0.service.session()
            && session.with_term(|term| term.mode().contains(TermMode::FOCUS_IN_OUT))
        {
            self.terminal
                .send(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
    }
    fn grid(&self) -> View {
        self.terminal.0.redraw.revision.get();
        let geometry = self.geometry.get();
        let preferences = self.terminal.0.preferences.get();
        let palette = Palette::new();
        let focused = self.focused.get();
        let cursor_on = self.blink.get() || !preferences.cursor_blink;
        let Some(session) = self.terminal.0.service.session() else {
            return el! { .div.{ size_full; bg:palette->background; pointer_events_none } }
                .into_view();
        };
        let (picture, cursor, background) = {
            let mut snapshot = self.snapshot.borrow_mut();
            if !snapshot.refresh(&session) {
                self.terminal.changed();
            }
            let mut painter = self.painter.borrow_mut();
            painter.font(&preferences.font_family, preferences.font_size);
            let picture = painter.draw(&snapshot, geometry, &palette, focused, cursor_on);
            (
                Arc::new(picture),
                snapshot.cursor(geometry),
                snapshot.background(&palette),
            )
        };
        self.cursor.set_if_changed(cursor);
        el! { picture.drawing.{ size_full; bg:background; text_size:1.0_f32; pointer_events_none } }
            .into_view()
    }
}
impl IntoView for Surface {
    fn into_view(self) -> View {
        let size = self.size;
        let key = self.clone();
        let input = self.clone();
        let down = self.clone();
        let up = self.clone();
        let moved = self.clone();
        let scroll = self.clone();
        let grid = self.clone();
        let focus = self.clone();
        let style = self.clone();
        let dropped = self.clone();
        let grabbed = self.clone();
        el! { .div.{ relative; flex_1; size_full; min_h: 0.0_f32; overflow_hidden; cursor:CursorIcon::Text;
            on_resized: move (width, height) => size.{set_if_changed: @(width, height)};
            on_pointer_down: move event => &down.{ pointer_down: event };
            on_pointer_up: move event => &up.{ pointer_up: event };
            on_pointer_move: move event => &moved.{ pointer_move: event };
            on_grab: move event => &grabbed.{ grabbed: event };
            on_scroll: move event => &scroll.{ scroll: event };
            on_drop_files: move (paths, _) => $(dropped.terminal.paste_paths(paths));
        }.[
            (move () => &grid.{ grid }).dyn_view,
            .text_input.{ value: self->input; node_ref: self->input_ref; absolute; min_w: 4.0_f32; max_w: 100.0_f32.pct; p: 0.0_f32; border: 0.0_f32;
                bg: Color::TRANSPARENT; caret_color: Color::TRANSPARENT; pointer_events_none;
                dyn_style:move s=>&style.{input_style:s};
                on_key_down: move event => &key.{ key: event };
                on_input: move text => &input.{committed:text};
                on_focus_change:move focused=>&focus.{focus_changed:focused};
            },
        ] }.into_view()
    }
}
