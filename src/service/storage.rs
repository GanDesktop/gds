//! Shared SQLite setup. Toasty owns pooling, connection tasks and transactions;
//! applications own their paths, schemas and queries.

use std::path::Path;

pub use toasty::migration::{MigrationFile, MigrationSet};
use toasty::sql;
pub use toasty::{Connection, Db as Database, Executor, ModelSet, Transaction};
pub use toasty_core::driver::operation::TransactionMode;

mod paths;
#[cfg(test)]
mod tests;

const BUSY_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] toasty::Error),
}

pub struct Options {
    pub models: ModelSet,
    pub migrations: MigrationSet,
    /// Also require the existing parent directory to be owner-only.
    pub private_directory: bool,
    /// Enable FULL auto-vacuum before creating a new database's tables.
    pub reclaim_pages: bool,
    pub wal: bool,
}

impl Options {
    pub fn new(migrations: MigrationSet) -> Self {
        Self {
            models: ModelSet::default(),
            migrations,
            private_directory: false,
            reclaim_pages: false,
            wal: true,
        }
    }

    pub fn models(mut self, models: ModelSet) -> Self {
        self.models = models;
        self
    }

    /// Open the native Toasty pool and apply the application's migrations.
    /// Call from the application's multi-thread Tokio runtime.
    ///
    /// # Errors
    /// Reports unsafe paths and database or migration failures.
    pub async fn open(self, path: impl AsRef<Path>) -> Result<Database, Error> {
        let path = path.as_ref();
        paths::prepare(path, self.private_directory).await?;
        let _initialization = paths::lock(path).await?;
        let mut db = Database::builder()
            .models(self.models)
            .max_pool_size(1)
            .pool_health_check_interval(None)
            .build(toasty_driver_sqlite::Sqlite::open(path))
            .await?;
        // SQLite pragmas belong to the physical connection. This local database uses
        // one pool slot, shared by every clone of the returned native Toasty handle.
        sql::query(format!("PRAGMA busy_timeout = {BUSY_TIMEOUT_MS}"))
            .exec(&mut db)
            .await?;
        sql::query("PRAGMA foreign_keys = ON").exec(&mut db).await?;
        if self.reclaim_pages {
            sql::query("PRAGMA auto_vacuum = FULL")
                .exec(&mut db)
                .await?;
        }
        self.migrations.apply(&db).await?;
        if self.wal {
            sql::query("PRAGMA journal_mode = WAL")
                .exec(&mut db)
                .await?;
        }
        Ok(db)
    }
}
