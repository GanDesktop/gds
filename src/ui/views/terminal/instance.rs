//! Reactive controller and PTY lifecycle handle for a reusable terminal surface.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use tokio::sync::watch;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Owner, RenderEffect, RwSignal, Signal, UnsyncCallback};

#[cfg(test)]
use super::geometry::GridGeometry;
use super::palette::Palette;
use super::{Event, Preferences};
use crate::app::Gds;
use crate::i18n;
use crate::service::task_manager::{Owner as TaskOwner, Scope};
use crate::service::terminal::{
    self as service, Error, Session, Shutdown, SpawnOptions, Status, input as terminal_input,
};

#[cfg(all(test, unix))]
mod tests;

const OUTPUT_COALESCE_INTERVAL: Duration = Duration::from_millis(16);

struct TerminalInner {
    scope: TaskOwner,
    service: service::Terminal,
    preferences: RwSignal<Preferences, LocalStorage>,
    status: RwSignal<Status, LocalStorage>,
    title: RwSignal<Option<String>, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
    redraw: RwSignal<u64, LocalStorage>,
    focused: RwSignal<bool, LocalStorage>,
    focus_target: RefCell<Option<NodeRef>>,
    focus_requested: Cell<bool>,
    next_callback_id: Cell<u64>,
    callbacks: RefCell<Vec<(u64, UnsyncCallback<Event>)>>,
    observed: RefCell<(Status, Option<String>)>,
    shutdown_done: watch::Receiver<bool>,
    theme_effect: RefCell<Option<RenderEffect<()>>>,
    owner: Owner,
    #[cfg(test)]
    test_geometry: Rc<Cell<GridGeometry>>,
    #[cfg(test)]
    test_cursor: Cell<(bool, bool, bool)>,
}

impl Drop for TerminalInner {
    fn drop(&mut self) {
        self.scope.cancel();
        *self.theme_effect.borrow_mut() = None;
        self.callbacks.borrow_mut().clear();
        self.owner.cleanup();
    }
}

/// Interactive terminal controller owning PTY startup, updates, theme sync, and shutdown.
#[derive(Clone)]
pub struct Terminal(Rc<TerminalInner>);

impl Terminal {
    /// Spawn a new terminal controller under `tasks` without tying its lifetime to a view.
    ///
    /// # Errors
    /// Returns an error if the terminal configuration cannot be prepared for launch.
    pub fn spawn(
        mut options: SpawnOptions,
        preferences: Preferences,
        tasks: &Scope,
    ) -> Result<Self, Error> {
        let preferences = preferences.normalized();
        preferences.apply_to_spawn(&mut options);
        if options.palette.is_none() {
            options.palette = Some(Palette::fallback().query_colors());
        }

        let gds = Gds::try_current();
        let scope = tasks.named_child(format_args!("Terminal · {}", options.cwd.display()));
        let (service, launch) = service::Terminal::prepare((*scope).clone(), options);

        let cancel = scope.token();
        let cleanup = service.clone();
        let (finished, shutdown_done) = watch::channel(false);
        scope.spawn_named("Shut down terminal", async move {
            cancel.cancelled().await;
            cleanup.shutdown().await;
            finished.send_replace(true);
        });
        scope.spawn_blocking_named("Launch terminal", move || launch.run());

        let (ui_tx, mut ui_rx) = watch::channel(0_u64);
        let observing = service.clone();
        scope.spawn_cancellable_named("Observe terminal", async move {
            let notify_ui = || {
                ui_tx.send_modify(|revision| *revision = revision.wrapping_add(1));
            };
            let Some(session) = observing.ready().await else {
                notify_ui();
                return;
            };
            let mut updates = session.updates();
            let mut stopped = *updates.borrow_and_update();
            notify_ui();
            if stopped {
                return;
            }
            while updates.changed().await.is_ok() {
                tokio::time::sleep(OUTPUT_COALESCE_INTERVAL).await;
                stopped = *updates.borrow_and_update();
                notify_ui();
                if stopped {
                    break;
                }
            }
            notify_ui();
        });

        let owner = Owner::new();
        let inner = owner.with(|| {
            let initial_status = service.status();
            let initial_title = service.title().filter(|value| !value.is_empty());
            let preferences_signal = RwSignal::new_local(preferences);
            let status_signal = RwSignal::new_local(initial_status.clone());
            let title_signal = RwSignal::new_local(initial_title.clone());
            let error_signal = RwSignal::new_local(match &initial_status {
                Status::Failed(message) => Some(message.clone()),
                _ => None,
            });
            let redraw_signal = RwSignal::new_local(0_u64);
            let focused_signal = RwSignal::new_local(false);
            let callbacks: RefCell<Vec<(u64, UnsyncCallback<Event>)>> = RefCell::new(Vec::new());
            let observed = RefCell::new((initial_status, initial_title));

            let theme_effect = gds.as_ref().map(|gds| {
                let prepared = gds.appearance().theme().prepared();
                let palette_service = service.clone();
                RenderEffect::new(move |_| {
                    prepared.with(|_| ());
                    palette_service.set_palette(Palette::current().query_colors());
                    redraw_signal.try_update(|revision| *revision = revision.wrapping_add(1));
                })
            });

            Rc::new(TerminalInner {
                scope,
                service,
                preferences: preferences_signal,
                status: status_signal,
                title: title_signal,
                error: error_signal,
                redraw: redraw_signal,
                focused: focused_signal,
                focus_target: RefCell::new(None),
                focus_requested: Cell::new(false),
                next_callback_id: Cell::new(0),
                callbacks,
                observed,
                shutdown_done,
                theme_effect: RefCell::new(theme_effect),
                owner: owner.clone(),
                #[cfg(test)]
                test_geometry: Rc::new(Cell::new(GridGeometry::default())),
                #[cfg(test)]
                test_cursor: Cell::new((false, false, true)),
            })
        });

        let weak = Rc::downgrade(&inner);
        owner.with(|| {
            zgui::reactive::spawn_local(async move {
                while ui_rx.changed().await.is_ok() {
                    let Some(strong) = weak.upgrade() else {
                        break;
                    };
                    sync_from_service(&strong);
                }
            });
        });

        Ok(Self(inner))
    }

    /// Replace active preferences (`normalized`); shell changes apply to future sessions only.
    pub fn apply_preferences(&self, prefs: Preferences) {
        let normalized = prefs.normalized();
        if self
            .0
            .preferences
            .try_get_untracked()
            .is_some_and(|cur| cur != normalized)
        {
            let _ = self.0.preferences.try_set(normalized);
        }
    }

    /// Reactive stream of normalized terminal preferences.
    #[must_use]
    pub fn preferences(&self) -> Signal<Preferences, LocalStorage> {
        self.0.preferences.into()
    }

    /// Reactive terminal window/process title reported via OSC title sequences.
    #[must_use]
    pub fn title(&self) -> Signal<Option<String>, LocalStorage> {
        self.0.title.into()
    }

    /// Reactive convenience flag indicating whether startup or shell execution is active.
    #[must_use]
    pub fn running(&self) -> Signal<bool, LocalStorage> {
        let status = self.0.status;
        Signal::derive_local(move || status.get().is_running())
    }

    /// Reactive lifecycle status of the terminal process.
    #[must_use]
    pub fn status(&self) -> Signal<Status, LocalStorage> {
        self.0.status.into()
    }

    /// Reactive I/O error banner text cleared on the next successful write.
    #[must_use]
    pub fn error(&self) -> Signal<Option<String>, LocalStorage> {
        self.0.error.into()
    }

    /// Working directory originally configured for this terminal.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        self.0.service.cwd()
    }

    /// Current non-reactive check of whether the terminal is still alive.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.0
            .status
            .try_get_untracked()
            .is_some_and(|status| status.is_running())
    }

    /// Localized label describing the current terminal lifecycle state.
    #[must_use]
    pub fn status_label(&self) -> String {
        match self.0.status.try_get_untracked().unwrap_or(Status::Closed) {
            Status::Starting => i18n::text("gds-terminal-starting"),
            Status::Running => i18n::text("gds-terminal-running"),
            Status::Stopping => i18n::text("gds-terminal-stopping"),
            Status::Closed => i18n::text("gds-terminal-exited"),
            Status::Failed(error) => {
                format!("{}: {error}", i18n::text("gds-terminal-start-failed"))
            }
            Status::Exited { code, signal } => crate::t!(
                "gds-terminal-exit-status",
                code = code,
                signal = signal.unwrap_or_default()
            ),
        }
    }

    /// Write raw keyboard or escape sequence bytes to the running PTY session.
    pub fn write(&self, bytes: &[u8]) {
        let Some(session) = self.0.service.session() else {
            return;
        };
        match session.write(bytes) {
            Ok(()) => self.set_error(None),
            Err(error) => self.set_error(Some(error.to_string())),
        }
    }

    /// Resize the PTY character grid directly.
    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(session) = self.0.service.session() {
            let _ = session.resize_with_pixels(cols, rows, 0, 0);
        }
    }

    /// Move keyboard focus to the mounted `TerminalView`, or defer until it mounts.
    pub fn focus(&self) {
        let already_focused = self.0.focused.try_get_untracked().unwrap_or(false);
        if already_focused {
            self.0.focus_requested.set(false);
            return;
        }
        self.0.focus_requested.set(true);
        self.apply_pending_focus();
    }

    /// Register a callback invoked for [`Event`] transitions on the UI thread.
    pub fn on_event(&self, cb: UnsyncCallback<Event>) {
        let id = self.0.next_callback_id.get();
        self.0.next_callback_id.set(id.wrapping_add(1));
        self.0.callbacks.borrow_mut().push((id, cb));
    }

    /// Change the active shell working directory asynchronously.
    ///
    /// # Errors
    /// Reports an unavailable session, invalid path, or unverified shell input.
    pub fn change_directory(
        &self,
        cwd: PathBuf,
    ) -> impl Future<Output = anyhow::Result<()>> + Send + 'static + use<> {
        let session = (!self.0.service.is_closing())
            .then(|| self.0.service.session())
            .flatten();
        async move {
            let session = session.ok_or(Error::ShellBusy)?;
            session.change_directory(&cwd).await?;
            Ok(())
        }
    }

    /// Paste quoted file paths without submitting the command line.
    pub fn paste_paths(&self, paths: &[PathBuf]) {
        match terminal_input::path_text(paths) {
            Ok(text) => self.paste_text(&text),
            Err(error) => self.set_error(Some(error.to_string())),
        }
    }

    /// Encode text respecting bracketed paste mode and write it to the PTY.
    pub fn paste_text(&self, text: &str) {
        let Some(session) = self.0.service.session() else {
            return;
        };
        let bytes = session.with_term(|term| terminal_input::paste_bytes(text, *term.mode()));
        self.write(&bytes);
    }

    /// Terminate the PTY process cleanly and await service cleanup completion.
    pub fn shutdown(&self) -> Shutdown {
        self.0.scope.cancel();
        drop(self.0.service.shutdown());
        sync_from_service(&self.0);
        self.0.callbacks.borrow_mut().clear();
        let mut done = self.0.shutdown_done.clone();
        Box::pin(async move {
            let _ = done.wait_for(|finished| *finished).await;
        })
    }

    pub(super) fn session(&self) -> Option<Session> {
        self.0.service.session()
    }

    pub(super) fn redraw_revision(&self) -> Signal<u64, LocalStorage> {
        self.0.redraw.into()
    }

    pub(super) fn request_redraw(&self) {
        self.0
            .redraw
            .try_update(|revision| *revision = revision.wrapping_add(1));
    }

    pub(super) fn focused_signal(&self) -> RwSignal<bool, LocalStorage> {
        self.0.focused
    }

    pub(super) fn register_focus_target(&self, node: Option<NodeRef>) {
        *self.0.focus_target.borrow_mut() = node;
        self.apply_pending_focus();
    }

    pub(super) fn apply_pending_focus(&self) {
        if let Some(target) = *self.0.focus_target.borrow()
            && target.get_untracked().is_some()
            && self.0.focus_requested.replace(false)
        {
            target.focus();
        }
    }

    pub(super) fn set_error(&self, message: Option<String>) {
        if self
            .0
            .error
            .try_get_untracked()
            .is_some_and(|cur| cur != message)
        {
            let _ = self.0.error.try_set(message);
        }
    }

    #[cfg(test)]
    pub(super) fn ready(&self) -> impl Future<Output = Option<Session>> + Send + 'static + use<> {
        self.0.service.ready()
    }

    #[cfg(test)]
    pub(super) fn set_test_cursor(&self, focused: bool, blink_active: bool, cursor_on: bool) {
        self.0.test_cursor.set((focused, blink_active, cursor_on));
    }

    #[cfg(test)]
    pub(super) fn test_geometry_cell(&self) -> Rc<Cell<GridGeometry>> {
        Rc::clone(&self.0.test_geometry)
    }

    #[cfg(test)]
    pub(super) fn test_geometry(&self) -> GridGeometry {
        self.0.test_geometry.get()
    }

    #[cfg(test)]
    pub(super) fn test_cursor_state(&self) -> (bool, bool, bool) {
        self.0.test_cursor.get()
    }
}

fn sync_from_service(inner: &TerminalInner) {
    let next_status = inner.service.status();
    let next_title = inner.service.title().filter(|value| !value.is_empty());

    let prev = inner.observed.borrow().clone();
    let state_changed = prev.0 != next_status || prev.1 != next_title;
    let title_changed = prev.1 != next_title;
    let exited_code = if prev.0 != next_status {
        match &next_status {
            Status::Exited { code, .. } => Some(i32::try_from(*code).ok()),
            Status::Closed | Status::Failed(_) => Some(None),
            Status::Starting | Status::Running | Status::Stopping => None,
        }
    } else {
        None
    };

    *inner.observed.borrow_mut() = (next_status.clone(), next_title.clone());

    if inner
        .status
        .try_get_untracked()
        .is_some_and(|cur| cur != next_status)
    {
        let error_text = match &next_status {
            Status::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let _ = inner.status.try_set(next_status);
        if let Some(error_text) = error_text {
            let _ = inner.error.try_set(Some(error_text));
        }
    }
    if inner
        .title
        .try_get_untracked()
        .is_some_and(|cur| cur != next_title)
    {
        let _ = inner.title.try_set(next_title.clone());
    }
    let _ = inner
        .redraw
        .try_update(|revision| *revision = revision.wrapping_add(1));

    if title_changed && let Some(title) = next_title {
        emit_event(inner, &Event::TitleChanged(title));
    }
    if let Some(code) = exited_code {
        emit_event(inner, &Event::Closed(code));
    }
    if state_changed {
        emit_event(inner, &Event::Changed);
    }
}

fn emit_event(inner: &TerminalInner, event: &Event) {
    let listeners: Vec<_> = inner.callbacks.borrow().iter().copied().collect();
    let mut dead_ids = Vec::new();
    for (id, cb) in listeners {
        if cb.try_run(event.clone()).is_none() {
            dead_ids.push(id);
        }
    }
    if !dead_ids.is_empty()
        && let Ok(mut callbacks) = inner.callbacks.try_borrow_mut()
    {
        callbacks.retain(|(id, _)| !dead_ids.contains(id));
    }
}

#[cfg(all(test, unix))]
mod teardown_tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

    use zgui::reactive::{Owner, UnsyncCallback};

    use super::{Event, Preferences, Terminal, sync_from_service};
    use crate::service::task_manager::Supervisor;
    use crate::service::terminal::SpawnOptions;

    fn posix_shell() -> PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|path| path.join("sh"))
            .chain([PathBuf::from("/bin/sh")])
            .find(|path| path.is_absolute() && path.is_file())
            .expect("POSIX shell")
    }

    #[test]
    fn shutdown_skips_disposed_child_owner_callbacks_without_panicking() {
        zgui::reactive::install().expect("install zgui reactive runtime");
        let supervisor = Supervisor::new(false).expect("test Tokio supervisor");
        let directory = tempfile::tempdir().expect("tempdir");
        let runtime = supervisor
            .scope()
            .runtime_handle()
            .expect("supervisor runtime handle")
            .clone();
        let root_owner = Owner::new();

        root_owner.with(|| {
            let terminal = Terminal::spawn(
                SpawnOptions {
                    cwd: directory.path().to_owned(),
                    ..SpawnOptions::default()
                },
                Preferences {
                    shell: Some(posix_shell()),
                    shell_args: vec!["-c".into(), "read go; exit 7".into()],
                    ..Default::default()
                },
                &supervisor.scope(),
            )
            .expect("spawn terminal");

            let session = runtime
                .block_on(terminal.ready())
                .expect("terminal session ready");
            sync_from_service(&terminal.0);

            let disposed_calls = Rc::new(Cell::new(0_usize));
            let child_owner = root_owner.child();
            child_owner.with(|| {
                let counter = Rc::clone(&disposed_calls);
                terminal.on_event(UnsyncCallback::new(move |_| {
                    counter.set(counter.get() + 1);
                }));
            });

            let live_events = Rc::new(RefCell::new(Vec::new()));
            let events_sink = Rc::clone(&live_events);
            terminal.on_event(UnsyncCallback::new(move |event| {
                events_sink.borrow_mut().push(event);
            }));

            // Dispose the child scope before the status transition is observed by Terminal.
            child_owner.cleanup();

            // Trigger an Exited status transition without flushing the UI reactive executor,
            // so `Terminal::shutdown` observes `Running -> Exited` synchronously.
            session.write(b"go\n").expect("trigger shell exit");
            runtime.block_on(session.wait_stopped());

            runtime.block_on(terminal.shutdown());

            assert_eq!(
                disposed_calls.get(),
                0,
                "disposed child Owner callback must be skipped cleanly"
            );
            let observed_events = live_events.borrow();
            assert!(
                matches!(
                    observed_events.as_slice(),
                    [Event::Closed(Some(7) | None), Event::Changed]
                ),
                "live Owner callback must still observe final shutdown transition: {observed_events:?}"
            );
        });
        root_owner.cleanup();
    }

    #[test]
    fn shutdown_after_root_owner_cleanup_completes_without_panicking() {
        zgui::reactive::install().expect("install zgui reactive runtime");
        let supervisor = Supervisor::new(false).expect("test Tokio supervisor");
        let directory = tempfile::tempdir().expect("tempdir");
        let runtime = supervisor
            .scope()
            .runtime_handle()
            .expect("supervisor runtime handle")
            .clone();
        let root_owner = Owner::new();

        let terminal = root_owner.with(|| {
            let terminal = Terminal::spawn(
                SpawnOptions {
                    cwd: directory.path().to_owned(),
                    ..SpawnOptions::default()
                },
                Preferences {
                    shell: Some(posix_shell()),
                    shell_args: vec!["-c".into(), "read go; exit 0".into()],
                    ..Default::default()
                },
                &supervisor.scope(),
            )
            .expect("spawn terminal");

            terminal.on_event(UnsyncCallback::new(|_| {}));
            terminal
        });

        let session = runtime
            .block_on(terminal.ready())
            .expect("terminal session ready");
        sync_from_service(&terminal.0);

        session.write(b"go\n").expect("trigger shell exit");
        runtime.block_on(session.wait_stopped());

        // Cleaning up the root owner disposes both `TerminalInner.owner` and the registered
        // `UnsyncCallback` before `Terminal::shutdown` runs (mirroring window teardown order).
        root_owner.cleanup();
        runtime.block_on(terminal.shutdown());
    }
}
