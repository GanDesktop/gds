//! Remote transfer orchestration; protocol primitives live in gds::service::remote.
mod resource;
mod transfer;
pub(crate) use resource::Resource;
pub(crate) use transfer::{CopyOptions, DEFAULT_BUFFER_SIZE, Progress, Report, copy, delete};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Remote(#[from] gds::service::remote::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error(transparent)]
    Transfer(#[from] super::transfer::Error),
    #[error("operation cancelled")]
    Cancelled,
    #[error("unsupported transfer: {0}")]
    Unsupported(&'static str),
    #[error("{operation}; cannot remove remote staging {stage}: {cleanup}")]
    StagingCleanup {
        stage: gds::service::remote::Address,
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error("{operation}; cannot remove local staging {path}: {cleanup}")]
    LocalStagingCleanup {
        path: std::path::PathBuf,
        #[source]
        operation: Box<Self>,
        cleanup: std::io::Error,
    },
    #[error("{primary}; stream close failed: {close}")]
    StreamClose {
        #[source]
        primary: Box<Self>,
        close: Box<Self>,
    },
}

impl Error {
    pub(super) fn combine_close(self, close: Result<(), Self>) -> Self {
        match close {
            Ok(()) => self,
            Err(close) => Self::StreamClose {
                primary: Box::new(self),
                close: Box::new(close),
            },
        }
    }
}
