//! A local PTY session, independent of either application's UI/runtime.
//!
//! The last UI handle requests shutdown. The I/O worker owns only `Shared`, not
//! the handle, so it cannot accidentally keep itself alive forever.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    grid::Dimensions,
    sync::FairMutex,
    term::{Config, Osc52},
    vte::ansi::Rgb,
};
use tokio::sync::{oneshot, watch};

use super::backend;

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_COLOR_QUERIES: usize = 1024;
const MAX_SCROLLBACK: usize = 100_000;
const MAX_HISTORY_CELLS: usize = 2_000_000;
const MAX_GRID_CELLS: usize = 1_000_000;
const MAX_TITLE_CHARS: usize = 1024;

#[derive(Clone, Debug)]
pub struct SpawnOptions {
    pub cwd: PathBuf,
    pub shell: Option<PathBuf>,
    /// Literal argv entries, never shell command text.
    pub args: Vec<String>,
    pub login: bool,
    pub columns: u16,
    pub rows: u16,
    pub scrollback: usize,
    /// Initial theme colors are available to shell startup queries before a view paints.
    pub palette: Option<[Rgb; alacritty_terminal::term::color::COUNT]>,
}

impl Default for SpawnOptions {
    fn default() -> Self {
        Self {
            // An unspecified directory must fail, never become HOME.
            cwd: PathBuf::new(),
            shell: None,
            args: Vec::new(),
            login: false,
            columns: 80,
            rows: 24,
            scrollback: 10_000,
            palette: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Stopping,
    Exited { code: u32, signal: Option<String> },
    Failed(String),
}

impl Status {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running | Self::Stopping)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("The terminal working directory must be an absolute, accessible directory: {0}")]
    InvalidDirectory(PathBuf),
    #[error("No executable user shell was found")]
    ShellUnavailable,
    #[error("The configured shell is not an absolute executable file: {0}")]
    InvalidShell(PathBuf),
    #[error(
        "Automatic login arguments are not supported for {0}; configure explicit arguments instead"
    )]
    UnsupportedLogin(PathBuf),
    #[error("Terminal dimensions must be at least 2 columns by 1 row and at most 1,000,000 cells")]
    InvalidSize,
    #[error("Terminal input exceeds the 4 MiB pending-input limit")]
    InputQueueFull,
    #[error("The terminal has stopped")]
    Closed,
    #[error(
        "Cannot verify empty shell input; open a new terminal in the desired folder or type cd manually"
    )]
    ShellBusy,
    #[error("This shell does not support the switch-folder command")]
    UnsupportedDirectoryCommand,
    #[cfg(not(unix))]
    #[error("The local PTY backend is not supported on this platform")]
    UnsupportedPlatform,
    #[error("PTY operation failed: {0:#}")]
    Pty(#[from] anyhow::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Size {
    pub columns: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Size {
    fn new(columns: u16, rows: u16, cell_width: u16, cell_height: u16) -> Result<Self, Error> {
        if columns < 2 || rows == 0 || usize::from(columns) * usize::from(rows) > MAX_GRID_CELLS {
            return Err(Error::InvalidSize);
        }
        Ok(Self {
            columns,
            rows,
            cell_width,
            cell_height,
        })
    }

    pub(crate) fn term_config(self, scrollback: usize) -> Config {
        Config {
            scrolling_history: scrollback
                .min(MAX_SCROLLBACK)
                .min(MAX_HISTORY_CELLS / usize::from(self.columns)),
            osc52: Osc52::Disabled,
            // Do not advertise a keyboard protocol until the view implements it.
            kitty_keyboard: false,
            ..Config::default()
        }
    }

    pub(crate) fn window_size(self) -> WindowSize {
        WindowSize {
            num_lines: self.rows,
            num_cols: self.columns,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }

    pub(crate) fn pty_size(self) -> portable_pty::PtySize {
        portable_pty::PtySize {
            rows: self.rows,
            cols: self.columns,
            pixel_width: self.columns.saturating_mul(self.cell_width),
            pixel_height: self.rows.saturating_mul(self.cell_height),
        }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }
    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }
    fn columns(&self) -> usize {
        usize::from(self.columns)
    }
}

pub(crate) struct ColorQuery {
    pub index: usize,
    pub format: Arc<dyn Fn(Rgb) -> String + Send + Sync>,
}

pub(crate) struct DirectoryChange {
    pub result: oneshot::Sender<Result<(), Error>>,
}

#[derive(Default)]
pub(crate) struct Control {
    pub directory_change: Option<DirectoryChange>,
    pub input: VecDeque<Vec<u8>>,
    pub responses: VecDeque<Vec<u8>>,
    pub color_queries: VecDeque<ColorQuery>,
    // Includes the worker's partially written message.
    pub input_bytes: usize,
    pub response_bytes: usize,
    pub resize: Option<Size>,
}

pub(crate) struct Metadata {
    pub status: Status,
    pub title: Option<String>,
    pub size: Size,
    pub error: Option<String>,
    pub palette: [Rgb; alacritty_terminal::term::color::COUNT],
}

pub(crate) struct State {
    pub control: Mutex<Control>,
    pub metadata: Mutex<Metadata>,
    pub shutdown: AtomicBool,
    /// Changes coalesce; true means native cleanup has completed.
    pub updates: watch::Sender<bool>,
    #[cfg(unix)]
    pub wake: backend::Wake,
}

impl State {
    pub(crate) fn notify(&self) {
        // A notification is a dirty bit, not a queue of output copies.
        self.updates.send_modify(|_| {});
    }

    pub(crate) fn wake(&self) {
        #[cfg(unix)]
        self.wake.notify();
    }

    pub(crate) fn stop(&self) {
        if !self.shutdown.swap(true, Ordering::AcqRel) {
            let mut metadata = lock(&self.metadata);
            if metadata.status == Status::Running {
                metadata.status = Status::Stopping;
            }
            drop(metadata);
            self.wake();
            self.notify();
        }
    }

    pub(crate) fn fail(&self, error: impl ToString) {
        let mut metadata = lock(&self.metadata);
        if metadata.error.is_none() {
            metadata.error = Some(error.to_string());
        }
        drop(metadata);
        self.stop();
    }

    pub(crate) fn respond(&self, text: String) {
        let mut control = lock(&self.control);
        if control.response_bytes.saturating_add(text.len()) > MAX_RESPONSE_BYTES {
            drop(control);
            self.fail("Terminal query response queue exceeded its limit");
            return;
        }
        control.response_bytes += text.len();
        control.responses.push_back(text.into_bytes());
        drop(control);
        self.wake();
    }
}

#[derive(Default)]
struct Registry {
    shutting_down: bool,
    states: Vec<Weak<State>>,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

/// Marks completion only after the backend has dropped its PTY handles and reaped.
/// During startup it is held on the spawning thread, so failed/in-flight spawns
/// participate in the same application shutdown barrier.
pub(crate) struct Completion(Arc<State>);

impl Completion {
    fn register(state: Arc<State>) -> Result<Self, Error> {
        let mut registry = lock(REGISTRY.get_or_init(Default::default));
        if registry.shutting_down {
            return Err(Error::Closed);
        }
        registry.states.retain(|state| state.strong_count() != 0);
        registry.states.push(Arc::downgrade(&state));
        Ok(Self(state))
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        let mut metadata = lock(&self.0.metadata);
        if metadata.status.is_running() {
            metadata.status =
                Status::Failed("Terminal startup or I/O worker ended unexpectedly".into());
        }
        drop(metadata);
        self.0.updates.send_replace(true);
    }
}

/// Permanently stop new spawns and asynchronously wait for all registered PTYs.
/// Register this future with the application's quit hook, not a window's hide
/// handler. It includes sessions currently inside the blocking spawn operation.
pub async fn shutdown_all() {
    let states = {
        let mut registry = lock(REGISTRY.get_or_init(Default::default));
        registry.shutting_down = true;
        registry
            .states
            .iter()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>()
    };
    for state in &states {
        state.stop();
    }
    for state in states {
        let _ = state.updates.subscribe().wait_for(|stopped| *stopped).await;
    }
}

pub(crate) fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    // A UI panic must not prevent the last handle from requesting cleanup.
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The listener type is exposed only to allow closures over Alacritty's `Term`.
#[derive(Clone)]
pub struct Listener {
    state: Arc<State>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.state.respond(text),
            Event::TextAreaSizeRequest(format) => {
                let size = lock(&self.state.metadata).size.window_size();
                self.state.respond(format(size));
            }
            Event::ColorRequest(index, format) => {
                let mut control = lock(&self.state.control);
                if control.color_queries.len() >= MAX_COLOR_QUERIES {
                    drop(control);
                    self.state
                        .fail("Terminal color query queue exceeded its limit");
                } else {
                    control
                        .color_queries
                        .push_back(ColorQuery { index, format });
                    drop(control);
                    self.state.wake();
                }
            }
            Event::Title(title) => {
                lock(&self.state.metadata).title = Some(
                    title
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(MAX_TITLE_CHARS)
                        .collect(),
                );
                self.state.notify();
            }
            Event::ResetTitle => {
                lock(&self.state.metadata).title = None;
                self.state.notify();
            }
            // Neither application is allowed clipboard access through OSC 52.
            Event::ClipboardLoad(..) | Event::ClipboardStore(..) => {}
            Event::Exit => self.state.stop(),
            _ => self.state.notify(),
        }
    }
}

// Conventional xterm palette for protocol queries; no clipboard/UI callbacks.
fn default_color(index: usize) -> Rgb {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];
    let [r, g, b] = match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let color = index - 16;
            let level = |value: usize| {
                if value == 0 {
                    0
                } else {
                    (55 + 40 * value) as u8
                }
            };
            [level(color / 36), level(color / 6 % 6), level(color % 6)]
        }
        232..=255 => [8 + (index as u8 - 232) * 10; 3],
        257 => [0, 0, 0], // NamedColor::Background
        _ => [229, 229, 229],
    };
    Rgb { r, g, b }
}

pub(crate) struct Shared {
    pub term: FairMutex<Term<Listener>>,
    pub state: Arc<State>,
    pub scrollback: usize,
}

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
    pub fn spawn(options: SpawnOptions) -> Result<Self, Error> {
        #[cfg(not(unix))]
        {
            let _ = options;
            Err(Error::UnsupportedPlatform)
        }
        #[cfg(unix)]
        {
            let size = Size::new(options.columns, options.rows, 0, 0)?;
            let (wake, wake_reader) = backend::Wake::pair().map_err(anyhow::Error::from)?;
            let (updates, _) = watch::channel(false);
            let state = Arc::new(State {
                control: Mutex::new(Control::default()),
                metadata: Mutex::new(Metadata {
                    status: Status::Running,
                    title: None,
                    size,
                    error: None,
                    palette: options
                        .palette
                        .unwrap_or_else(|| std::array::from_fn(default_color)),
                }),
                shutdown: AtomicBool::new(false),
                updates,
                wake,
            });
            let completion = Completion::register(state.clone())?;
            let config = size.term_config(options.scrollback);
            let term = Term::new(
                config,
                &size,
                Listener {
                    state: state.clone(),
                },
            );
            let shared = Arc::new(Shared {
                term: FairMutex::new(term),
                state,
                scrollback: options.scrollback.min(MAX_SCROLLBACK),
            });
            let backend =
                backend::Backend::open(&options, size, wake_reader)?.with_completion(completion);
            if shared.state.shutdown.load(Ordering::Acquire) {
                return Err(Error::Closed);
            }
            let worker_shared = shared.clone();
            // Backend::drop also reaps if thread creation fails after spawning.
            std::thread::Builder::new()
                .name("gds-terminal-io".into())
                .spawn(move || backend.run(worker_shared))
                .map_err(anyhow::Error::from)?;
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

    /// Set query fallback colors to match the view; OSC color overrides remain intact.
    pub fn set_palette(&self, palette: [Rgb; alacritty_terminal::term::color::COUNT]) {
        lock(&self.0.shared.state.metadata).palette = palette;
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
