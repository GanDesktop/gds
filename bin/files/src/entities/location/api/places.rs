use std::path::Path;

use gds::service::storage::{self, Database, MigrationFile, MigrationSet, Options};
use toasty::models;

use super::super::model::{
    address::Location,
    places::{Change, Places},
};
use accessor::Accessor;
use record::{PlacesMeta, QuickAccessLocation, SavedLocation, ViewSettings};

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
        "002-quick-access.sql",
        include_str!("places/assets/002-quick-access.sql"),
    ),
]);

fn resolve_default_quick_access(
    home: &Path,
    configured: [Option<&Path>; 4],
    use_default_names: bool,
) -> Vec<Location> {
    const FALLBACK_NAMES: [&str; 4] = ["Desktop", "Downloads", "Pictures", "Videos"];
    let mut out = Vec::new();
    for (dir, fallback_name) in configured.into_iter().zip(FALLBACK_NAMES) {
        let path = match dir {
            Some(dir) => dir.to_path_buf(),
            None if use_default_names => home.join(fallback_name),
            None => continue,
        };
        if path == home || !path.is_dir() {
            continue;
        }
        let location = Location::from(path.as_path());
        if !out.contains(&location) {
            out.push(location);
        }
    }
    out
}

fn default_quick_access_locations() -> Vec<Location> {
    let use_default_names = directories::BaseDirs::new().is_some_and(|base| {
        base.config_dir()
            .join("user-dirs.dirs")
            .try_exists()
            .is_ok_and(|exists| !exists)
    });
    if let Some(dirs) = directories::UserDirs::new() {
        return resolve_default_quick_access(
            dirs.home_dir(),
            [
                dirs.desktop_dir(),
                dirs.download_dir(),
                dirs.picture_dir(),
                dirs.video_dir(),
            ],
            use_default_names,
        );
    }
    let Some(base) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    resolve_default_quick_access(base.home_dir(), [None, None, None, None], use_default_names)
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
        let defaults = default_quick_access_locations();
        Self::open_at_with_quick_access(tasks, path, &defaults).await
    }

    /// # Errors
    /// Reports database or migration failures.
    pub(crate) async fn open_at_with_quick_access(
        tasks: &gds::service::task_manager::Scope,
        path: &Path,
        default_quick_access: &[Location],
    ) -> Result<Self, Error> {
        let mut db = Options::new(MIGRATIONS)
            .models(models!(
                SavedLocation,
                QuickAccessLocation,
                PlacesMeta,
                ViewSettings
            ))
            .open(tasks, path)
            .await?;
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
