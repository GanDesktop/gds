//! PTY transport: Tokio readiness on one worker, Alacritty parser/grid.
//! Host queue limits, query ordering and session cleanup stay outside Alacritty's event loop.

#[cfg(unix)]
mod shell;
#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub(crate) use unix::Backend;
