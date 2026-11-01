use std::path::Path;

use bas::{
    model::{Event, SessionId},
    service::{HistoryWindow, tls::ConnectionOptions},
};
use gds::service::storage::{
    Database, MigrationFile, MigrationSet, Options, Transaction, TransactionMode,
};
use sha2::{Digest, Sha256};
use toasty::models;

use super::{Error, record};
use accessor::Accessor;

mod accessor;

const MIGRATIONS: MigrationSet = MigrationSet::new(&[MigrationFile::new(
    1,
    "001-history.sql",
    include_str!("store/assets/001-history.sql"),
)]);

#[derive(Clone)]
pub(super) struct Store {
    db: Database,
    namespace: String,
    limit: i64,
}

impl Store {
    /// # Errors
    /// Reports unsafe paths, database or migration failures.
    pub(super) async fn open(
        path: &Path,
        options: &ConnectionOptions,
        limit: u64,
    ) -> Result<Self, Error> {
        let db = Options {
            private_directory: true,
            reclaim_pages: true,
            wal: false,
            ..Options::new(MIGRATIONS)
        }
        .models(models!(record::Session, record::Page, record::Event))
        .open(path)
        .await?;
        let store = Self {
            db,
            namespace: namespace(options)?,
            limit: i64::try_from(limit).unwrap_or(i64::MAX),
        };
        let mut db = store.db.clone();
        let mut tx = db
            .transaction_builder()
            .mode(TransactionMode::Immediate)
            .begin()
            .await?;
        if let Err(error) = store.accessor(&mut tx).evict(None).await {
            tx.rollback().await?;
            return Err(error);
        }
        tx.commit().await?;
        Ok(store)
    }

    /// # Errors
    /// Reports database failures or corrupt history.
    pub(super) async fn load(
        &self,
        id: SessionId,
        before: Option<u64>,
    ) -> Result<Option<HistoryWindow>, Error> {
        if before == Some(0) {
            return Ok(None);
        }
        let mut db = self.db.clone();
        let mut tx = db
            .transaction_builder()
            .mode(TransactionMode::Immediate)
            .begin()
            .await?;
        let result = self.accessor(&mut tx).window(id, before).await;
        match result {
            Ok(window) => {
                tx.commit().await?;
                Ok(window)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    /// # Errors
    /// Reports invalid history, database or serialization failures.
    pub(super) async fn store(&self, window: &HistoryWindow) -> Result<(), Error> {
        let mut db = self.db.clone();
        let mut tx = db
            .transaction_builder()
            .mode(TransactionMode::Immediate)
            .begin()
            .await?;
        let result = self.accessor(&mut tx).store_window(window).await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    /// # Errors
    /// Reports mismatched sessions, database or serialization failures.
    pub(super) async fn append(&self, id: SessionId, events: &[Event]) -> Result<(), Error> {
        let mut db = self.db.clone();
        let mut tx = db
            .transaction_builder()
            .mode(TransactionMode::Immediate)
            .begin()
            .await?;
        let result = self.accessor(&mut tx).append_events(id, events).await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    /// # Errors
    /// Reports database failures.
    pub(super) async fn remove(&self, id: SessionId) -> Result<(), Error> {
        let mut db = self.db.clone();
        let mut tx = db
            .transaction_builder()
            .mode(TransactionMode::Immediate)
            .begin()
            .await?;
        if let Err(error) = self.accessor(&mut tx).remove(id).await {
            tx.rollback().await?;
            return Err(error);
        }
        tx.commit().await?;
        Ok(())
    }

    fn accessor<'tx, 'db>(&'tx self, tx: &'tx mut Transaction<'db>) -> Accessor<'tx, 'db> {
        Accessor::new(tx, &self.namespace, self.limit)
    }
}

fn namespace(options: &ConnectionOptions) -> Result<String, Error> {
    // Hash the enum discriminant and non-lossy path debug representation too:
    // display alone is not injective for non-UTF8 Unix socket paths.
    let key = serde_json::to_vec(&(format!("{:?}", options.target), &options.tls))?;
    Ok(format!("{:x}", Sha256::digest(key)))
}
