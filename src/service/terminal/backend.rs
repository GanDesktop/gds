//! PTY transport. Unix uses one nonblocking worker, not Alacritty's tty/event loop.

#[cfg(unix)]
mod shell;
#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub(crate) use unix::{Backend, Wake};
