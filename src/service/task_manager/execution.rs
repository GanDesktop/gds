//! Completion handles keep shutdown rejection distinct from executor failure.
use super::Closed;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Closed(#[from] Closed),
    #[error("task scope has no I/O executor")]
    NoExecutor,
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
}

/// Dropping detaches registered work unless `cancel_on_drop` was selected.
pub struct Task<T> {
    task: Option<tokio::task::JoinHandle<Result<T, Error>>>,
    error: Option<Error>,
    abort_on_drop: bool,
}

impl<T> Task<T> {
    pub(super) fn running(task: tokio::task::JoinHandle<Result<T, Error>>) -> Self {
        Self {
            task: Some(task),
            error: None,
            abort_on_drop: false,
        }
    }
    pub(super) fn rejected(error: Error) -> Self {
        Self {
            task: None,
            error: Some(error),
            abort_on_drop: false,
        }
    }
    /// Use only for disposable work whose future owns all cleanup through RAII.
    pub fn cancel_on_drop(mut self) -> Self {
        self.abort_on_drop = true;
        self
    }
    pub fn abort(&self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
impl<T> Future for Task<T> {
    type Output = Result<T, Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(error) = self.error.take() {
            return Poll::Ready(Err(error));
        }
        Pin::new(
            self.task
                .as_mut()
                .expect("completed task must not be polled again"),
        )
        .poll(cx)
        .map(|result| result.map_err(Error::from).and_then(std::convert::identity))
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if self.abort_on_drop {
            self.abort();
        }
    }
}
