#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid remote address: {0}")]
    Address(String),
    #[error("authentication failed")]
    Authentication,
    #[error("the connection was cancelled")]
    Cancelled,
    #[error("this location belongs to a different connection")]
    WrongConnection,
    #[error("unsupported remote operation: {0}")]
    Unsupported(&'static str),
    #[error(
        "cannot list SMB shares on this server; enter the share directly as smb://server/share"
    )]
    ShareList(#[source] anyhow::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{operation}: {source}")]
    Protocol {
        operation: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

impl Error {
    pub(super) fn protocol(operation: &'static str, source: impl Into<anyhow::Error>) -> Self {
        Self::Protocol {
            operation,
            source: source.into(),
        }
    }
}
