use super::{
    Copy, INPUT_CONTEXT, Paste, Preferences, ScrollBottom, ScrollPageDown, ScrollPageUp, ScrollTop,
    input,
};
use crate::service::terminal::Session;
use crate::{app::AppExt as _, i18n};
use alacritty_terminal::{
    grid::Scroll,
    index::{Column, Line, Point as GridPoint, Side},
    selection::{Selection, SelectionType},
    term::TermMode,
};
use gpui_kit::{prelude::*, *};
use std::time::Duration;

mod fonts;
mod grid;
mod ime;
pub(super) mod palette;
#[cfg(all(test, feature = "gui-tests", unix))]
mod tests;

const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(600);

pub(super) struct View {
    pub(super) session: Session,
    focus: FocusHandle,
    preferences: Preferences,
    fonts: Option<fonts::Fonts>,
    snapshot: grid::Snapshot,
    geometry: grid::Geometry,
    preedit: String,
    preedit_selection: std::ops::Range<usize>,
    selecting: bool,
    scroll_remainder: f32,
    cursor_on: bool,
    focused: bool,
    pub(super) error: Option<String>,
    _updates: Task<()>,
    _blink: Option<Task<()>>,
    _focus_subscriptions: Vec<Subscription>,
}

impl View {
    pub(super) fn new(
        session: Session,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let mut updates = session.updates();
        let task = cx.spawn(async move |view, cx| {
            loop {
                if *updates.borrow() {
                    // Completion may drop between subscribing/observing and polling this loop.
                    let _ = view.update(cx, |view, cx| {
                        view.cursor_on = true;
                        cx.notify();
                    });
                    break;
                }
                if updates.changed().await.is_err() {
                    break;
                }
                // Coalesce output bursts to one UI invalidation per frame budget.
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let stopped = *updates.borrow_and_update();
                if view
                    .update(cx, |view, cx| {
                        view.cursor_on = true;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if stopped {
                    break;
                }
            }
        });
        let subscriptions = vec![
            cx.on_focus(&focus, window, |view, _, cx| {
                view.focused = true;
                view.cursor_on = true;
                view.sync_blink(cx);
                if view.mode().contains(TermMode::FOCUS_IN_OUT) {
                    view.send(b"\x1b[I", cx);
                }
                cx.notify();
            }),
            cx.on_blur(&focus, window, |view, _, cx| {
                view.focused = false;
                view.preedit.clear();
                view.preedit_selection = 0..0;
                view.selecting = false;
                view.sync_blink(cx);
                if view.mode().contains(TermMode::FOCUS_IN_OUT) {
                    view.send(b"\x1b[O", cx);
                }
                cx.notify();
            }),
        ];
        Self {
            session,
            focus,
            preferences,
            fonts: None,
            snapshot: grid::Snapshot::default(),
            geometry: grid::Geometry::default(),
            preedit: String::new(),
            preedit_selection: 0..0,
            selecting: false,
            scroll_remainder: 0.,
            cursor_on: true,
            focused: false,
            error: None,
            _updates: task,
            _blink: None,
            _focus_subscriptions: subscriptions,
        }
    }

    pub(super) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }
    pub(super) fn set_preferences(&mut self, preferences: Preferences, cx: &mut Context<Self>) {
        self.preferences = preferences;
        self.sync_blink(cx);
        cx.notify();
    }
    fn should_blink(&self, cx: &App) -> bool {
        self.focused && self.preferences.cursor_blink && !cx.reduce_motion()
    }
    fn sync_blink(&mut self, cx: &mut Context<Self>) {
        if !self.should_blink(cx) {
            self._blink = None;
            self.cursor_on = true;
            return;
        }
        if self._blink.is_some() {
            return;
        }
        self._blink = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(CURSOR_BLINK_INTERVAL).await;
                let keep_running = view.update(cx, |view, cx| {
                    if !view.should_blink(cx) {
                        view._blink = None;
                        let was_off = !view.cursor_on;
                        view.cursor_on = true;
                        if was_off {
                            cx.notify();
                        }
                        return false;
                    }
                    view.cursor_on = !view.cursor_on;
                    cx.notify();
                    true
                });
                if !matches!(keep_running, Ok(true)) {
                    break;
                }
            }
        }));
    }
    fn mode(&self) -> TermMode {
        self.session.with_term(|term| *term.mode())
    }
    fn send(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if bytes.is_empty() {
            return;
        }
        if let Err(error) = self.session.write(bytes) {
            self.error = Some(error.to_string());
        }
        self.cursor_on = true;
        cx.notify();
    }
    fn input(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.session.with_term(|term| {
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        });
        self.send(bytes, cx);
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.session.with_term(|term| term.selection_to_string()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.paste_text(&text, cx);
        }
    }
    pub(super) fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let bytes = input::paste_bytes(text, self.mode());
        self.input(&bytes, cx);
    }
    fn scroll_page_up(&mut self, _: &ScrollPageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_display(Scroll::PageUp, cx);
    }
    fn scroll_page_down(&mut self, _: &ScrollPageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_display(Scroll::PageDown, cx);
    }
    fn scroll_top(&mut self, _: &ScrollTop, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_display(Scroll::Top, cx);
    }
    fn scroll_bottom(&mut self, _: &ScrollBottom, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_display(Scroll::Bottom, cx);
    }
    fn scroll_display(&mut self, scroll: Scroll, cx: &mut Context<Self>) {
        self.session.with_term(|term| term.scroll_display(scroll));
        cx.notify();
    }
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) || !self.preedit.is_empty() {
            return;
        }
        if let Some(bytes) = input::key_bytes(
            &event.keystroke,
            self.mode(),
            self.preferences.alt_sends_escape,
        ) {
            self.input(&bytes, cx);
            cx.stop_propagation();
        }
    }
    fn grid_point(&self, position: Point<Pixels>) -> (GridPoint, Side, usize, usize) {
        let (column, row, side) = self.geometry.cell_at(position);
        let point = GridPoint::new(
            Line(row as i32 - self.snapshot.display_offset as i32),
            Column(column),
        );
        (point, side, column, row)
    }
    fn mouse_report(
        &mut self,
        position: Point<Pixels>,
        button: u8,
        released: bool,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let (_, _, column, row) = self.grid_point(position);
        if let Some(bytes) =
            input::mouse_bytes(button, column, row, released, modifiers, self.mode())
        {
            self.send(&bytes, cx);
        }
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        if self.mode().intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            self.mouse_report(
                event.position,
                mouse_button(event.button),
                false,
                event.modifiers,
                cx,
            );
        } else if event.button == MouseButton::Left {
            let (point, side, _, _) = self.grid_point(event.position);
            let ty = match event.click_count {
                2 => SelectionType::Semantic,
                3.. => SelectionType::Lines,
                _ if event.modifiers.alt => SelectionType::Block,
                _ => SelectionType::Simple,
            };
            self.session
                .with_term(|term| term.selection = Some(Selection::new(ty, point, side)));
            self.selecting = true;
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        // A file drag belongs to the host's drop target, not PTY mouse reporting.
        // Let MouseUp bubble so its on_drop handler receives the payload.
        if cx.has_active_drag() {
            return;
        }
        if !self.selecting && self.mode().intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift
        {
            self.mouse_report(
                event.position,
                mouse_button(event.button),
                true,
                event.modifiers,
                cx,
            );
        }
        self.selecting = false;
        cx.stop_propagation();
        cx.notify();
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting && event.dragging() {
            let (point, side, _, _) = self.grid_point(event.position);
            self.session.with_term(|term| {
                if let Some(selection) = term.selection.as_mut() {
                    selection.update(point, side);
                }
                if event.position.y < self.geometry.bounds.top() {
                    term.scroll_display(Scroll::Delta(1));
                } else if event.position.y > self.geometry.bounds.bottom() {
                    term.scroll_display(Scroll::Delta(-1));
                }
            });
            cx.notify();
        } else if !event.modifiers.shift {
            let mode = self.mode();
            if mode.contains(TermMode::MOUSE_MOTION)
                || (mode.contains(TermMode::MOUSE_DRAG) && event.pressed_button.is_some())
            {
                self.mouse_report(
                    event.position,
                    32 + event.pressed_button.map(mouse_button).unwrap_or(3),
                    false,
                    event.modifiers,
                    cx,
                );
            }
        }
    }
    fn scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = match event.delta {
            ScrollDelta::Lines(lines) => lines.y,
            ScrollDelta::Pixels(delta) => {
                f32::from(delta.y) / f32::from(self.geometry.cell.height).max(1.)
            }
        };
        self.scroll_remainder += delta;
        let lines = self.scroll_remainder.trunc() as i32;
        self.scroll_remainder -= lines as f32;
        let mode = self.mode();
        if mode.intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            for _ in 0..lines.unsigned_abs().min(100) {
                self.mouse_report(
                    event.position,
                    if lines > 0 { 64 } else { 65 },
                    false,
                    event.modifiers,
                    cx,
                );
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
            && !event.modifiers.shift
        {
            let sequence: &[u8] = if lines > 0 { b"\x1bOA" } else { b"\x1bOB" };
            for _ in 0..lines.unsigned_abs().min(100) {
                self.send(sequence, cx);
            }
        } else {
            self.session
                .with_term(|term| term.scroll_display(Scroll::Delta(lines)));
        }
        cx.stop_propagation();
        cx.notify();
    }
}

fn mouse_button(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        _ => 3,
    }
}
impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_blink(cx);
        let theme = cx.gds_ro().appearance().theme();
        let colors = theme.colors();
        let content_background = theme.content_background();
        let status = match self.session.status() {
            crate::service::terminal::ProcessStatus::Running => None,
            crate::service::terminal::ProcessStatus::Stopping => {
                Some(i18n::text("gds-terminal-stopping", cx))
            }
            crate::service::terminal::ProcessStatus::Failed(error) => {
                Some(format!("{}: {error}", i18n::text("gds-terminal-io-failed", cx)).into())
            }
            crate::service::terminal::ProcessStatus::Exited { code, signal } => {
                let mut args = fluent_bundle::FluentArgs::new();
                args.set("code", i64::from(code));
                args.set("signal", signal.unwrap_or_else(|| "—".into()));
                Some(i18n::format("gds-terminal-exit-status", Some(&args), cx))
            }
        };
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .key_context(INPUT_CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::scroll_page_up))
            .on_action(cx.listener(Self::scroll_page_down))
            .on_action(cx.listener(Self::scroll_top))
            .on_action(cx.listener(Self::scroll_bottom))
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_scroll(cx.listener(Self::scroll))
            .child(grid::Grid { view: cx.entity() })
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .bg(content_background)
                        .text_xs()
                        .text_color(colors.error)
                        .child(error),
                )
            })
            .when_some(status, |this, status| {
                this.child(
                    div()
                        .bg(content_background)
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(status),
                )
            })
    }
}
