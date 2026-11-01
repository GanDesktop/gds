//! Bounded diagnostics for one application process, independent of a UI or async runtime.
mod store;

pub use store::{Entry, Level, Snapshot, Store};

use tracing_subscriber::{EnvFilter, prelude::*};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid default tracing filter: {0}")]
    Filter(#[from] tracing_subscriber::filter::ParseError),
    #[error("cannot install tracing subscriber: {0}")]
    Subscriber(#[from] tracing_subscriber::util::TryInitError),
}

/// Install stderr diagnostics and optionally collect logs for the GUI viewer.
/// RUST_LOG overrides the defaults. File-picker stdout stays reserved for JSON.
///
/// # Errors
/// Reports an invalid default filter or an already-installed global subscriber.
pub fn init_tracing(default_filter: &str, enable_logs: bool) -> Result<Option<Store>, Error> {
    let store = enable_logs.then(Store::new);
    let console = EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into());
    let memory = if let Some(store) = &store {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or(EnvFilter::try_new(default_filter)?);
        Some(store.layer().with_filter(filter))
    } else {
        None
    };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(console),
        )
        .with(memory)
        .try_init()?;
    Ok(store)
}
