use std::path::Path;

use gds::service::storage::{self, Database, MigrationFile, MigrationSet, Options};
use toasty::models;

use super::super::model::places::{Change, Places};
use accessor::Accessor;
use record::{Location, SavedLocation, ViewSettings};

mod accessor;
mod record;

const MIGRATIONS: MigrationSet = MigrationSet::new(&[
    MigrationFile::new(
        1,
        "001-places.sql",
        include_str!("places/assets/001-places.sql"),
    ),
    MigrationFile::new(
        2,
        "002-location-views.sql",
        include_str!("places/assets/002-location-views.sql"),
    ),
    MigrationFile::new(
        3,
        "003-remote-locations.sql",
        include_str!("places/assets/003-remote-locations.sql"),
    ),
    MigrationFile::new(
        4,
        "004-unified-places.sql",
        include_str!("places/assets/004-unified-places.sql"),
    ),
]);

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid saved remote location: {0}")]
    Remote(String),
    #[error(transparent)]
    Storage(#[from] storage::Error),
    #[error(transparent)]
    Database(#[from] toasty::Error),
    #[error(transparent)]
    Task(#[from] tokio::task::JoinError),
    #[error("invalid places data: {0}")]
    Data(#[from] serde_json::Error),
}

#[derive(Clone)]
pub(crate) struct Store {
    db: Database,
}

impl Store {
    /// # Errors
    /// Reports database or migration failures.
    pub(crate) async fn open_at(path: &Path) -> Result<Self, Error> {
        let mut db = Options::new(MIGRATIONS)
            .models(models!(Location, SavedLocation, ViewSettings))
            .open(path)
            .await?;
        let mut tx = db
            .transaction_builder()
            .mode(storage::TransactionMode::Immediate)
            .begin()
            .await?;
        if let Err(error) = Accessor::new(&mut tx).initialize().await {
            tx.rollback().await?;
            return Err(error);
        }
        tx.commit().await?;
        Ok(Self { db })
    }

    /// # Errors
    /// Reports unavailable storage or invalid persisted records.
    pub(crate) async fn read(&self) -> Result<Places, Error> {
        let mut db = self.db.clone();
        let mut tx = db.transaction().await?;
        let result = Accessor::new(&mut tx).snapshot().await;
        match result {
            Ok(places) => {
                tx.commit().await?;
                Ok(places)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    /// # Errors
    /// Reports database or serialization failures; failed edits roll back.
    pub(crate) async fn save(&self, changes: Vec<Change>) -> Result<Places, Error> {
        let mut db = self.db.clone();
        let mut tx = db
            .transaction_builder()
            .mode(storage::TransactionMode::Immediate)
            .begin()
            .await?;
        let result = Accessor::new(&mut tx).save(&changes).await;
        match result {
            Ok(places) => {
                tx.commit().await?;
                Ok(places)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}
