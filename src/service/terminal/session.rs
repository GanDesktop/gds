//! A local PTY session, independent of either application's UI/runtime.
//!
//! The last UI handle requests shutdown. The I/O worker owns only `Shared`, not
//! the handle, so it cannot accidentally keep itself alive forever.

use std::sync::{Arc, atomic::Ordering};
#[cfg(unix)]
use std::sync::{Mutex, atomic::AtomicBool};

#[cfg(unix)]
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::{Term, vte::ansi::Rgb};
use tokio::sync::{oneshot, watch};

#[cfg(unix)]
use super::backend;
#[cfg(test)]
use super::state::MAX_RESPONSE_BYTES;
#[cfg(unix)]
use super::state::{Completion, Control, MAX_SCROLLBACK, Metadata, State};
use super::state::{
    DirectoryChange, Error, Listener, MAX_INPUT_BYTES, QueryPalette, Shared, Size, SpawnOptions,
    Status, lock,
};

struct Handle {
    shared: Arc<Shared>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shared.state.stop();
    }
}

#[derive(Clone)]
pub struct Session(Arc<Handle>);

impl Session {
    /// Start a shell and I/O worker. Call this on a background executor.
    ///
    /// # Errors
    /// Returns a validation, shell resolution, PTY, or worker creation error.
    pub fn spawn(
        tasks: &crate::service::task_manager::Scope,
        options: SpawnOptions,
    ) -> Result<Self, Error> {
        let palette = QueryPalette::new(options.palette);
        Self::spawn_with_palette(tasks, options, palette)
    }

    pub(crate) fn spawn_with_palette(
        tasks: &crate::service::task_manager::Scope,
        options: SpawnOptions,
        palette: QueryPalette,
    ) -> Result<Self, Error> {
        #[cfg(not(unix))]
        {
            let _ = (tasks, options, palette);
            Err(Error::UnsupportedPlatform)
        }
        #[cfg(unix)]
        {
            if tasks.token().is_cancelled() {
                return Err(Error::Closed);
            }
            // Hold a scope admission token across Backend::open so scope.shutdown()
            // waits for and reaps a session cancelled mid-spawn.
            let _spawn = tasks
                .register_named(
                    "Spawn terminal",
                    crate::service::task_manager::Execution::Blocking,
                )
                .map_err(|_| Error::Closed)?;
            let size = Size::new(options.columns, options.rows, 0, 0)?;
            let (updates, _) = watch::channel(false);
            let state = Arc::new(State {
                control: Mutex::new(Control::default()),
                metadata: Mutex::new(Metadata {
                    status: Status::Running,
                    title: None,
                    size,
                    error: None,
                }),
                palette,
                shutdown: AtomicBool::new(false),
                updates,
                wake: tokio::sync::Notify::new(),
            });
            let completion = Completion::new(state.clone());
            let config = size.term_config(options.scrollback);
            let term = Term::new(config, &size, Listener::new(state.clone()));
            let shared = Arc::new(Shared {
                term: FairMutex::new(term),
                state,
                scrollback: options.scrollback.min(MAX_SCROLLBACK),
            });
            let backend = backend::Backend::open(&options, size)?.with_completion(completion);
            if tasks.token().is_cancelled() || shared.state.shutdown.load(Ordering::Acquire) {
                drop(backend);
                return Err(Error::Closed);
            }
            let worker_shared = shared.clone();
            // Backend::drop also reaps if thread creation fails after spawning.
            let worker_tasks = tasks.clone();
            if let Err(error) = tasks.spawn_thread_named(
                "Run terminal I/O",
                std::thread::Builder::new().name("gds-terminal-io".into()),
                move || backend.run(worker_shared, worker_tasks),
            ) {
                if tasks.token().is_cancelled()
                    || error
                        .get_ref()
                        .is_some_and(|inner| inner.is::<crate::service::task_manager::Closed>())
                {
                    return Err(Error::Closed);
                }
                return Err(anyhow::Error::from(error).into());
            }
            Ok(Self(Arc::new(Handle { shared })))
        }
    }

    /// Subscribe to coalesced updates; true means native cleanup has completed.
    /// The UI should not keep a strong Session in the task awaiting this channel.
    pub fn updates(&self) -> watch::Receiver<bool> {
        self.0.shared.state.updates.subscribe()
    }

    /// Wait for cleanup without initiating it. Call `terminate` first when closing.
    pub async fn wait_stopped(&self) {
        let _ = self.updates().wait_for(|stopped| *stopped).await;
    }

    /// Borrow the grid briefly for selection, scrolling, or copying visible cells.
    /// Do not perform I/O, render, or re-enter this session from the closure.
    pub fn with_term<R>(&self, f: impl FnOnce(&mut Term<Listener>) -> R) -> R {
        f(&mut self.0.shared.term.lock())
    }

    /// Use the previous frame when the parser currently holds the grid lock.
    pub fn try_with_term<R>(&self, f: impl FnOnce(&mut Term<Listener>) -> R) -> Option<R> {
        self.0
            .shared
            .term
            .try_lock_unfair()
            .map(|mut term| f(&mut term))
    }

    /// Queue literal bytes without blocking on PTY I/O or retaining an input log.
    ///
    /// # Errors
    /// Returns `Closed` after shutdown or `InputQueueFull` under backpressure.
    /// A failed enqueue is atomic: none of these bytes were accepted.
    pub fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        let state = &self.0.shared.state;
        if state.shutdown.load(Ordering::Acquire) || !self.is_running() {
            return Err(Error::Closed);
        }
        let mut control = lock(&state.control);
        if state.shutdown.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if control.input_bytes.saturating_add(bytes.len()) > MAX_INPUT_BYTES {
            return Err(Error::InputQueueFull);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        control.input_bytes += bytes.len();
        control.input.push_back(bytes.to_vec());
        drop(control);
        state.wake();
        Ok(())
    }

    /// Request an in-place directory change. Currently fails closed: without
    /// trusted shell integration we cannot prove its editing buffer is empty.
    /// No command, Enter, interrupt or line-clearing sequence is injected.
    /// Open a new terminal at the desired directory or type cd manually instead.
    ///
    /// # Errors
    /// Rejects invalid/unrepresentable paths, unsupported shells, unverified
    /// shell input (`ShellBusy`), or a stopped terminal.
    pub async fn change_directory(&self, path: &std::path::Path) -> Result<(), Error> {
        let command = super::directory::command(path)?;
        let (result, received) = oneshot::channel();
        let state = &self.0.shared.state;
        {
            let mut control = lock(&state.control);
            if state.shutdown.load(Ordering::Acquire) {
                return Err(Error::Closed);
            }
            if control.directory_change.is_some() || control.input_bytes != 0 {
                return Err(Error::ShellBusy);
            }
            if command.len() > MAX_INPUT_BYTES {
                return Err(Error::InputQueueFull);
            }
            control.directory_change = Some(DirectoryChange { result });
        }
        state.wake();
        received.await.unwrap_or(Err(Error::Closed))
    }

    /// Queue a coalesced emulator/kernel resize, with cell pixels for queries.
    ///
    /// # Errors
    /// Returns `InvalidSize` or `Closed`; asynchronous ioctl failures set status.
    pub fn resize_with_pixels(
        &self,
        columns: u16,
        rows: u16,
        cell_width: u16,
        cell_height: u16,
    ) -> Result<(), Error> {
        let size = Size::new(columns, rows, cell_width, cell_height)?;
        let state = &self.0.shared.state;
        if state.shutdown.load(Ordering::Acquire) || !self.is_running() {
            return Err(Error::Closed);
        }
        let mut control = lock(&state.control);
        if state.shutdown.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if control.resize == Some(size) {
            return Ok(());
        }
        control.resize = Some(size);
        drop(control);
        state.wake();
        Ok(())
    }

    /// Update theme query fallbacks without modifying program-owned OSC overrides.
    pub fn set_palette(&self, palette: [Rgb; alacritty_terminal::term::color::COUNT]) {
        self.0.shared.state.palette.set(palette);
    }

    pub fn terminate(&self) {
        self.0.shared.state.stop();
    }
    pub fn status(&self) -> Status {
        lock(&self.0.shared.state.metadata).status.clone()
    }
    pub fn is_running(&self) -> bool {
        self.status().is_running()
    }
    pub fn title(&self) -> Option<String> {
        lock(&self.0.shared.state.metadata).title.clone()
    }
}

#[cfg(test)]
mod tests;
