//! Disposable, connection-scoped history. Only safe server windows establish
//! page boundaries; context never fills holes in replayable history.

use std::path::Path;

use bas::{
    model::{Event, SessionId},
    service::{HistoryWindow, tls::ConnectionOptions},
};
use gds::service::storage;

mod record;
mod store;
#[cfg(test)]
mod tests;

const DEFAULT_BYTE_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Storage(#[from] storage::Error),
    #[error(transparent)]
    Database(#[from] toasty::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid history cache: {0}")]
    Invalid(&'static str),
}

/// Toasty-backed history storage; no histories are retained by this handle.
#[derive(Clone)]
pub struct Cache {
    store: store::Store,
}

impl Cache {
    /// Opens the private GUI cache, never the daemon database.
    ///
    /// # Errors
    /// Returns path, migration, database, or connection startup failures.
    pub async fn open(options: &ConnectionOptions) -> Result<Self, Error> {
        let root = dirs::cache_dir().ok_or(Error::Invalid("no cache directory"))?;
        Self::open_at(
            root.join("gds/agents/history-v1/history.sqlite"),
            options,
            DEFAULT_BYTE_LIMIT,
        )
        .await
    }

    /// The limit bounds serialized content plus row overhead across all namespaces.
    /// SQLite has additional fixed schema/index overhead, and reclaims freed pages.
    ///
    /// # Errors
    /// Rejects unsafe paths and reports migration or database errors.
    pub async fn open_at(
        path: impl AsRef<Path>,
        options: &ConnectionOptions,
        byte_limit: u64,
    ) -> Result<Self, Error> {
        Ok(Self {
            store: store::Store::open(path.as_ref(), options, byte_limit).await?,
        })
    }

    /// Loads one safe newest window and its contiguous live extension.
    /// Its `cursor` is the retained replay frontier, which can trail metadata
    /// observed during an older-page fetch. Resume/catch up from `cursor`, not
    /// `session.meta.revision`.
    /// # Errors
    /// Returns storage or corrupt-cache errors.
    pub async fn load_latest(&self, session: SessionId) -> Result<Option<HistoryWindow>, Error> {
        self.store.load(session, None).await
    }

    /// Loads a saved page or covering-page prefix ending at `before - 1`.
    /// # Errors
    /// Returns storage or corrupt-cache errors.
    pub async fn load_before(
        &self,
        session: SessionId,
        before: u64,
    ) -> Result<Option<HistoryWindow>, Error> {
        self.store.load(session, Some(before)).await
    }

    /// Atomically retains a safe window, without moving the newest frontier back.
    /// Oversized sessions are discarded rather than interrupting the live flow.
    /// # Errors
    /// Returns invalid-window, storage or corrupt-cache errors.
    pub async fn store_window(&self, window: &HistoryWindow) -> Result<(), Error> {
        self.store.store(window).await
    }

    /// Ignores transient events and duplicates; stops at the first replay gap.
    /// # Errors
    /// Returns mismatched-session, storage or corrupt-cache errors.
    pub async fn append(&self, session: SessionId, events: &[Event]) -> Result<(), Error> {
        self.store.append(session, events).await
    }

    /// Removes every retained page and event for this connection's session.
    /// # Errors
    /// Returns database errors.
    pub async fn remove(&self, session: SessionId) -> Result<(), Error> {
        self.store.remove(session).await
    }
}
