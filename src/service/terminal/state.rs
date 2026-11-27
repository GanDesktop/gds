//! Shared terminal session state, limits, and error/option types.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
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

pub(super) const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_COLOR_QUERIES: usize = 1024;
pub(super) const MAX_SCROLLBACK: usize = 100_000;
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
    pub(crate) fn new(
        columns: u16,
        rows: u16,
        cell_width: u16,
        cell_height: u16,
    ) -> Result<Self, Error> {
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
}

/// Shared before native startup so a theme change never races a copied launch snapshot.
#[derive(Clone)]
pub(crate) struct QueryPalette(Arc<Mutex<[Rgb; alacritty_terminal::term::color::COUNT]>>);

impl QueryPalette {
    pub(crate) fn new(colors: Option<[Rgb; alacritty_terminal::term::color::COUNT]>) -> Self {
        Self(Arc::new(Mutex::new(
            colors.unwrap_or_else(|| std::array::from_fn(default_color)),
        )))
    }

    pub(crate) fn set(&self, colors: [Rgb; alacritty_terminal::term::color::COUNT]) {
        *lock(&self.0) = colors;
    }

    pub(crate) fn color(&self, index: usize) -> Rgb {
        lock(&self.0)[index]
    }
}

pub(crate) struct State {
    pub control: Mutex<Control>,
    pub metadata: Mutex<Metadata>,
    pub palette: QueryPalette,
    pub shutdown: AtomicBool,
    /// Changes coalesce; true means native cleanup has completed.
    pub updates: watch::Sender<bool>,
    #[cfg(unix)]
    pub wake: tokio::sync::Notify,
}

impl State {
    pub(crate) fn notify(&self) {
        // A notification is a dirty bit, not a queue of output copies.
        self.updates.send_modify(|_| {});
    }

    pub(crate) fn wake(&self) {
        #[cfg(unix)]
        self.wake.notify_one();
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

/// Marks completion only after the backend has dropped its PTY handles and reaped.
/// During startup it is held on the spawning thread so failed or cancelled
/// spawns still report completion before unwinding.
pub(crate) struct Completion(Arc<State>);

impl Completion {
    pub(crate) fn new(state: Arc<State>) -> Self {
        Self(state)
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

impl Listener {
    pub(crate) fn new(state: Arc<State>) -> Self {
        Self { state }
    }
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
