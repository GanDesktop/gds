//! Remote transfer orchestration; protocol primitives live in gds::service::remote.
mod resource;
mod transfer;
pub(crate) use resource::Resource;
pub(crate) use transfer::{Progress, Report, copy, delete};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Remote(#[from] gds::service::remote::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("operation cancelled")]
    Cancelled,
    #[error("unsupported transfer: {0}")]
    Unsupported(&'static str),
    #[error("{0}")]
    Failed(String),
}
