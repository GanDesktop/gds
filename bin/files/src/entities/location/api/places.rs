use std::path::Path;

use gds::service::storage::{self, Database, MigrationFile, MigrationSet, Options};
use toasty::models;

#[cfg(test)]
use super::super::model::address::Location;
use super::super::model::places::{Change, Places};
use accessor::Accessor;
use record::{PlacesMeta, QuickAccessLocation, SavedLocation, ViewSettings};

pub(crate) use user_dirs::{UserDirsSource, XdgUserDirs};

mod accessor;
mod record;
mod user_dirs;

const MIGRATIONS: MigrationSet = MigrationSet::new(&[
    MigrationFile::new(
        1,
        "001-places.sql",
        include_str!("places/assets/001-places.sql"),
    ),
    MigrationFile::new(
        2,
        "002-quick-access.sql",
        include_str!("places/assets/002-quick-access.sql"),
    ),
    // Databases created before migrations 001..005 were squashed into 001 already
    // recorded id = 2..5 in `__toasty_migrations`, causing Toasty to skip id = 2.
    // Re-running with IF NOT EXISTS at id = 6 heals those databases idempotently.
    MigrationFile::new(
        6,
        "006-quick-access-compat.sql",
        include_str!("places/assets/006-quick-access-compat.sql"),
    ),
]);

/// Result of syncing persisted Quick Access entries with XDG user directories.
#[derive(Clone, Debug)]
pub(crate) struct SyncResult {
    pub(crate) places: Places,
    pub(crate) changed: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid saved remote location: {0}")]
    Remote(String),
    #[error(transparent)]
    Storage(#[from] storage::Error),
    #[error(transparent)]
    Database(#[from] toasty::Error),
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
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
    pub(crate) async fn open_at(
        tasks: &gds::service::task_manager::Scope,
        path: &Path,
    ) -> Result<Self, Error> {
        let Some(source) = UserDirsSource::current() else {
            return Self::open_at_with_xdg_dirs(tasks, path, None, &XdgUserDirs::default()).await;
        };
        let xdg_dirs = match source.load() {
            Ok(dirs) => dirs,
            Err(error) => {
                tracing::warn!(
                    path = %source.config_path.display(),
                    %error,
                    "cannot read XDG user-dirs.dirs; falling back to default names"
                );
                XdgUserDirs::resolve(&source.home, None)
            }
        };
        Self::open_at_with_xdg_dirs(tasks, path, Some(source.home.as_path()), &xdg_dirs).await
    }

    /// Open a store and seed or reconcile Quick Access entries using an
    /// explicit XDG user directory snapshot.
    ///
    /// # Errors
    /// Reports database, migration, or serialization failures.
    pub(crate) async fn open_at_with_xdg_dirs(
        tasks: &gds::service::task_manager::Scope,
        path: &Path,
        home: Option<&Path>,
        xdg_dirs: &XdgUserDirs,
    ) -> Result<Self, Error> {
        let mut db = Self::open_database(tasks, path).await?;
        let mut tx = db
            .transaction_builder()
            .mode(storage::TransactionMode::Immediate)
            .begin()
            .await?;
        if let Err(error) = Accessor::new(&mut tx)
            .initialize_with_xdg_dirs(home, xdg_dirs)
            .await
        {
            tx.rollback().await?;
            return Err(error);
        }
        tx.commit().await?;
        Ok(Self { db })
    }

    /// # Errors
    /// Reports database or migration failures.
    #[cfg(test)]
    pub(crate) async fn open_at_with_quick_access(
        tasks: &gds::service::task_manager::Scope,
        path: &Path,
        default_quick_access: &[Location],
    ) -> Result<Self, Error> {
        let mut db = Self::open_database(tasks, path).await?;
        let mut tx = db
            .transaction_builder()
            .mode(storage::TransactionMode::Immediate)
            .begin()
            .await?;
        if let Err(error) = Accessor::new(&mut tx)
            .initialize(default_quick_access)
            .await
        {
            tx.rollback().await?;
            return Err(error);
        }
        tx.commit().await?;
        Ok(Self { db })
    }

    async fn open_database(
        tasks: &gds::service::task_manager::Scope,
        path: &Path,
    ) -> Result<Database, Error> {
        let db = Options::new(MIGRATIONS)
            .models(models!(
                SavedLocation,
                QuickAccessLocation,
                PlacesMeta,
                ViewSettings
            ))
            .open(tasks, path)
            .await?;
        Ok(db)
    }

    /// Reconcile tracked XDG user directories against `xdg_dirs` in place.
    ///
    /// # Errors
    /// Reports database or serialization failures; failed edits roll back.
    pub(crate) async fn sync_xdg_user_dirs(
        &self,
        home: Option<&Path>,
        xdg_dirs: &XdgUserDirs,
    ) -> Result<SyncResult, Error> {
        let mut db = self.db.clone();
        let mut tx = db
            .transaction_builder()
            .mode(storage::TransactionMode::Immediate)
            .begin()
            .await?;
        let result = Accessor::new(&mut tx)
            .sync_xdg_user_dirs(home, xdg_dirs)
            .await;
        match result {
            Ok(outcome) => {
                tx.commit().await?;
                Ok(outcome)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
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

#[cfg(test)]
mod tests;
