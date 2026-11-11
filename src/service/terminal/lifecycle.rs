//! Session startup and shutdown ownership, independent of a UI executor or window.
use super::{
    ProcessStatus, Session, SpawnOptions,
    session::{QueryPalette, lock},
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

/// Completion of a requested shutdown, including an in-flight launch.
pub type Shutdown = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Running,
    Stopping,
    Closed,
    Exited { code: u32, signal: Option<String> },
    Failed(String),
}
impl Status {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

enum Phase {
    Starting,
    Ready(Session),
    Failed(String),
    Cancelled,
}
struct Lifecycle {
    phase: Phase,
    closing: bool,
    closed: bool,
}
struct State {
    lifecycle: Mutex<Lifecycle>,
    ready: watch::Sender<bool>,
    palette: QueryPalette,
}
impl State {
    fn stop(&self) {
        let session = {
            let mut state = lock(&self.lifecycle);
            state.closing = true;
            match &state.phase {
                Phase::Ready(session) => Some(session.clone()),
                _ => None,
            }
        };
        if let Some(session) = session {
            session.terminate();
        }
    }
    fn session(&self) -> Option<Session> {
        match &lock(&self.lifecycle).phase {
            Phase::Ready(session) => Some(session.clone()),
            _ => None,
        }
    }
}
struct Handle {
    cwd: PathBuf,
    state: Arc<State>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.state.stop();
    }
}

/// One terminal process. Its final owner requests cleanup even while startup is pending.
#[derive(Clone)]
pub struct Terminal(Arc<Handle>);

/// A blocking startup job. Run on a background executor; dropping it cancels startup.
pub struct Launch {
    tasks: crate::service::task_manager::Scope,
    state: Arc<State>,
    options: Option<SpawnOptions>,
}
impl Launch {
    /// Start the native session. Startup errors are available through [`Terminal::status`].
    pub fn run(mut self) {
        if lock(&self.state.lifecycle).closing {
            lock(&self.state.lifecycle).phase = Phase::Cancelled;
            return;
        }
        let options = self.options.take().expect("a launch is consumed only once");
        let result = Session::spawn_with_palette(&self.tasks, options, self.state.palette.clone());
        let mut state = lock(&self.state.lifecycle);
        state.phase = match result {
            Ok(session) => {
                if state.closing {
                    session.terminate();
                }
                Phase::Ready(session)
            }
            Err(error) => Phase::Failed(error.to_string()),
        };
    }
}
impl Drop for Launch {
    fn drop(&mut self) {
        let mut state = lock(&self.state.lifecycle);
        if matches!(state.phase, Phase::Starting) {
            state.phase = if state.closing {
                Phase::Cancelled
            } else {
                Phase::Failed("Terminal startup was cancelled".into())
            };
        }
        self.state.ready.send_replace(true);
    }
}

impl Terminal {
    /// Prepare ownership before scheduling the native launch on a background executor.
    pub fn prepare(
        tasks: crate::service::task_manager::Scope,
        options: SpawnOptions,
    ) -> (Self, Launch) {
        let (ready, _) = watch::channel(false);
        let state = Arc::new(State {
            lifecycle: Mutex::new(Lifecycle {
                phase: Phase::Starting,
                closing: false,
                closed: false,
            }),
            ready,
            palette: QueryPalette::new(options.palette),
        });
        let terminal = Self(Arc::new(Handle {
            cwd: options.cwd.clone(),
            state: state.clone(),
        }));
        (
            terminal,
            Launch {
                tasks,
                state,
                options: Some(options),
            },
        )
    }
    /// Replace theme query fallbacks for both starting and already-running sessions.
    /// The parser's OSC overrides are independent and survive until the program resets them.
    pub fn set_palette(
        &self,
        palette: [alacritty_terminal::vte::ansi::Rgb; alacritty_terminal::term::color::COUNT],
    ) {
        self.0.state.palette.set(palette);
    }

    pub fn cwd(&self) -> &Path {
        &self.0.cwd
    }
    pub fn is_closing(&self) -> bool {
        lock(&self.0.state.lifecycle).closing
    }
    pub fn status(&self) -> Status {
        let state = lock(&self.0.state.lifecycle);
        if state.closed {
            return Status::Closed;
        }
        match &state.phase {
            Phase::Starting if state.closing => Status::Stopping,
            Phase::Starting => Status::Starting,
            Phase::Cancelled => Status::Stopping,
            Phase::Failed(error) => Status::Failed(error.clone()),
            Phase::Ready(session) => match session.status() {
                ProcessStatus::Running => Status::Running,
                ProcessStatus::Stopping => Status::Stopping,
                ProcessStatus::Exited { code, signal } => Status::Exited { code, signal },
                ProcessStatus::Failed(error) => Status::Failed(error),
            },
        }
    }
    pub fn title(&self) -> Option<String> {
        self.session().and_then(|session| session.title())
    }
    /// The session exposes its emulator state, input operations and coalesced update stream.
    pub fn session(&self) -> Option<Session> {
        self.0.state.session()
    }
    /// Wait for startup without extending ownership of the terminal process.
    pub fn ready(&self) -> impl Future<Output = Option<Session>> + Send + 'static + use<> {
        let state = self.0.state.clone();
        async move {
            let _ = state.ready.subscribe().wait_for(|ready| *ready).await;
            let lifecycle = lock(&state.lifecycle);
            match &lifecycle.phase {
                Phase::Ready(session) if !lifecycle.closing => Some(session.clone()),
                _ => None,
            }
        }
    }
    /// Request cleanup immediately. Every returned future waits for the same native cleanup.
    pub fn shutdown(&self) -> Shutdown {
        self.0.state.stop();
        let state = self.0.state.clone();
        Box::pin(async move {
            let _ = state.ready.subscribe().wait_for(|ready| *ready).await;
            if let Some(session) = state.session() {
                session.terminate();
                session.wait_stopped().await;
            }
            lock(&state.lifecycle).closed = true;
        })
    }
}

#[cfg(test)]
mod tests;
