use std::path::Path;

use gds::service::storage::Transaction;
use toasty::Json;

use super::super::super::model::places::{Change, Places, RECENT_LIMIT};
use super::super::super::model::view::LocationView;
use super::{
    Error, SyncResult,
    record::{
        self, Kind, PlacesMeta, QuickAccessLocation, SavedLocation, ViewKey,
        ViewSettings as ViewRecord,
    },
    user_dirs::{QuickAccessEntry, XdgUserDirs, reconcile_quick_access},
};
use crate::entities::location::{Location, ViewScope, ViewSettings};

const QUICK_ACCESS_SEEDED_KEY: &str = "quick_access_seeded";
const XDG_USER_DIRS_SNAPSHOT_KEY: &str = "xdg_user_dirs_snapshot";

pub(super) struct Accessor<'tx, 'db> {
    tx: &'tx mut Transaction<'db>,
}

impl<'tx, 'db> Accessor<'tx, 'db> {
    pub(super) fn new(tx: &'tx mut Transaction<'db>) -> Self {
        Self { tx }
    }

    /// # Errors
    /// Reports database failures while creating the application defaults.
    #[cfg(test)]
    pub(super) async fn initialize(
        &mut self,
        default_quick_access: &[Location],
    ) -> Result<(), Error> {
        self.ensure_default_view_settings().await?;

        let seeded = PlacesMeta::filter_by_key(QUICK_ACCESS_SEEDED_KEY.to_owned())
            .first()
            .exec(self.tx)
            .await?;
        if seeded.is_none() {
            self.seed_quick_access(default_quick_access).await?;
        }
        Ok(())
    }

    /// Initialize defaults and seed or reconcile Quick Access entries against
    /// the current XDG user directory mapping.
    ///
    /// # Errors
    /// Reports database or serialization failures while seeding or syncing.
    pub(super) async fn initialize_with_xdg_dirs(
        &mut self,
        home: Option<&Path>,
        xdg_dirs: &XdgUserDirs,
    ) -> Result<(), Error> {
        self.ensure_default_view_settings().await?;

        let seeded = PlacesMeta::filter_by_key(QUICK_ACCESS_SEEDED_KEY.to_owned())
            .first()
            .exec(self.tx)
            .await?;
        if seeded.is_none() {
            let defaults = xdg_dirs.initial_quick_access_locations();
            self.seed_quick_access(&defaults).await?;
            self.save_xdg_user_dirs_snapshot(xdg_dirs).await?;
        } else {
            let _ = self.sync_xdg_user_dirs_inner(home, xdg_dirs).await?;
        }
        Ok(())
    }

    /// Reconcile tracked XDG user directories in an already-initialized store
    /// and return the resulting snapshot alongside whether Quick Access changed.
    ///
    /// # Errors
    /// Reports database or serialization failures while syncing.
    pub(super) async fn sync_xdg_user_dirs(
        &mut self,
        home: Option<&Path>,
        xdg_dirs: &XdgUserDirs,
    ) -> Result<SyncResult, Error> {
        let seeded = PlacesMeta::filter_by_key(QUICK_ACCESS_SEEDED_KEY.to_owned())
            .first()
            .exec(self.tx)
            .await?;
        let changed = if seeded.is_some() {
            self.sync_xdg_user_dirs_inner(home, xdg_dirs).await?
        } else {
            false
        };
        let places = self.snapshot().await?;
        Ok(SyncResult { places, changed })
    }

    async fn ensure_default_view_settings(&mut self) -> Result<(), Error> {
        let defaults = ViewSettings::defaults();
        let key = ViewKey::new(&ViewScope::Defaults)?;
        ViewRecord::upsert_by_kind_and_identity_and_path(key.kind, key.identity, key.path)
            .columns(defaults.columns.map(Json))
            .widths(Some(Json(defaults.widths)))
            .sort(defaults.sort.map(Json))
            .details(defaults.details)
            .view_mode(defaults.mode.map(Into::into))
            .or_ignore()
            .exec(self.tx)
            .await?;
        Ok(())
    }

    async fn seed_quick_access(&mut self, default_quick_access: &[Location]) -> Result<(), Error> {
        for (position, location) in default_quick_access.iter().enumerate() {
            let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
            QuickAccessLocation::upsert_by_location_kind_and_identity_and_path(
                key.kind,
                key.identity,
                key.path,
            )
            .position(position as i64)
            .or_ignore()
            .exec(self.tx)
            .await?;
        }
        PlacesMeta::upsert_by_key(QUICK_ACCESS_SEEDED_KEY.to_owned())
            .value("1".to_owned())
            .or_ignore()
            .exec(self.tx)
            .await?;
        Ok(())
    }

    async fn sync_xdg_user_dirs_inner(
        &mut self,
        home: Option<&Path>,
        xdg_dirs: &XdgUserDirs,
    ) -> Result<bool, Error> {
        let previous = match PlacesMeta::filter_by_key(XDG_USER_DIRS_SNAPSHOT_KEY.to_owned())
            .first()
            .exec(self.tx)
            .await?
        {
            Some(meta) => Some(serde_json::from_str::<XdgUserDirs>(&meta.value)?),
            None => None,
        };
        if previous.as_ref() == Some(xdg_dirs) {
            return Ok(false);
        }

        let existing = self.quick_access_entries().await?;
        let changed = if let Some(updated) =
            reconcile_quick_access(&existing, previous.as_ref(), xdg_dirs, home)
        {
            self.replace_quick_access_entries(&existing, &updated)
                .await?;
            true
        } else {
            false
        };

        self.save_xdg_user_dirs_snapshot(xdg_dirs).await?;
        Ok(changed)
    }

    async fn save_xdg_user_dirs_snapshot(&mut self, xdg_dirs: &XdgUserDirs) -> Result<(), Error> {
        let encoded = serde_json::to_string(xdg_dirs)?;
        PlacesMeta::upsert_by_key(XDG_USER_DIRS_SNAPSHOT_KEY.to_owned())
            .value(encoded)
            .exec(self.tx)
            .await?;
        Ok(())
    }

    async fn quick_access_entries(&mut self) -> Result<Vec<QuickAccessEntry>, Error> {
        let mut entries = Vec::new();
        for row in QuickAccessLocation::all()
            .order_by(QuickAccessLocation::fields().position().asc())
            .exec(self.tx)
            .await?
        {
            let scope = ViewKey {
                kind: row.location_kind,
                identity: row.identity,
                path: row.path,
            }
            .into_scope()?;
            if let ViewScope::Location(location) = scope {
                entries.push(QuickAccessEntry {
                    location,
                    position: row.position,
                });
            }
        }
        Ok(entries)
    }

    async fn replace_quick_access_entries(
        &mut self,
        existing: &[QuickAccessEntry],
        updated: &[QuickAccessEntry],
    ) -> Result<(), Error> {
        for old in existing {
            if !updated.iter().any(|entry| entry.location == old.location) {
                let key = ViewKey::new(&ViewScope::Location(old.location.clone()))?;
                QuickAccessLocation::filter_by_location_kind_and_identity_and_path(
                    key.kind,
                    key.identity,
                    key.path,
                )
                .delete()
                .exec(self.tx)
                .await?;
            }
        }
        for next in updated {
            let unchanged = existing
                .iter()
                .any(|old| old.location == next.location && old.position == next.position);
            if unchanged {
                continue;
            }
            let key = ViewKey::new(&ViewScope::Location(next.location.clone()))?;
            QuickAccessLocation::upsert_by_location_kind_and_identity_and_path(
                key.kind,
                key.identity,
                key.path,
            )
            .position(next.position)
            .exec(self.tx)
            .await?;
        }
        Ok(())
    }

    /// # Errors
    /// Reports database or serialization failures.
    pub(super) async fn save(&mut self, changes: &[Change]) -> Result<Places, Error> {
        for change in changes {
            self.write_change(change).await?;
        }
        self.snapshot().await
    }

    /// # Errors
    /// Reports database or decoding failures.
    pub(super) async fn snapshot(&mut self) -> Result<Places, Error> {
        let mut places = Places::default();
        for row in SavedLocation::all()
            .order_by(SavedLocation::fields().position().asc())
            .exec(self.tx)
            .await?
        {
            let scope = ViewKey {
                kind: row.location_kind,
                identity: row.identity,
                path: row.path,
            }
            .into_scope()?;
            if let ViewScope::Location(location) = scope {
                match row.kind {
                    Kind::Favorite => places.favorites.push(location),
                    Kind::Recent => places.recent.push(location),
                    Kind::Pin => places.pinned.push(location),
                }
            }
        }
        places.recent.reverse();
        places.recent.truncate(RECENT_LIMIT);
        for entry in self.quick_access_entries().await? {
            places.quick_access.push(entry.location);
        }
        for row in ViewRecord::all()
            .order_by(ViewRecord::fields().identity().asc())
            .order_by(ViewRecord::fields().path().asc())
            .exec(self.tx)
            .await?
        {
            let settings = ViewSettings {
                columns: row.columns.map(|Json(value)| value),
                widths: row.widths.map(|Json(value)| value).unwrap_or_default(),
                sort: row.sort.map(|Json(value)| value),
                details: row.details,
                mode: row.view_mode.map(Into::into),
            };
            match (ViewKey {
                kind: row.kind,
                identity: row.identity,
                path: row.path,
            })
            .into_scope()?
            {
                ViewScope::Defaults => places.defaults = settings,
                ViewScope::Location(location) => {
                    places.views.push(LocationView { location, settings })
                }
            }
        }
        Ok(places)
    }

    async fn write_change(&mut self, change: &Change) -> Result<(), Error> {
        match change {
            Change::Favorite { location, present } => {
                self.save_location(Kind::Favorite, location, *present)
                    .await?;
            }
            Change::Pin { location, present } => {
                self.save_location(Kind::Pin, location, *present).await?
            }
            Change::PinOrder(order) => {
                let mut locations = self.snapshot().await?.pinned;
                crate::entities::location::model::places::reorder_existing(&mut locations, order);
                for (position, location) in locations.iter().enumerate() {
                    let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
                    SavedLocation::upsert_by_kind_and_location_kind_and_identity_and_path(
                        Kind::Pin,
                        key.kind,
                        key.identity,
                        key.path,
                    )
                    .position(position as i64)
                    .exec(self.tx)
                    .await?;
                }
            }
            Change::QuickAccess { location, present } => {
                self.save_quick_access(location, *present).await?;
            }
            Change::QuickAccessOrder(order) => {
                let mut locations = self.snapshot().await?.quick_access;
                crate::entities::location::model::places::reorder_existing(&mut locations, order);
                for (position, location) in locations.iter().enumerate() {
                    let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
                    QuickAccessLocation::upsert_by_location_kind_and_identity_and_path(
                        key.kind,
                        key.identity,
                        key.path,
                    )
                    .position(position as i64)
                    .exec(self.tx)
                    .await?;
                }
            }
            Change::ClearRecent => {
                SavedLocation::filter(SavedLocation::fields().kind().eq(Kind::Recent))
                    .delete()
                    .exec(self.tx)
                    .await?;
            }
            Change::Recent(location) => {
                self.save_location(Kind::Recent, location, true).await?;
                if let Some(cutoff) =
                    SavedLocation::filter(SavedLocation::fields().kind().eq(Kind::Recent))
                        .order_by(SavedLocation::fields().position().desc())
                        .limit(1)
                        .offset(RECENT_LIMIT)
                        .select(SavedLocation::fields().position())
                        .first()
                        .exec(self.tx)
                        .await?
                {
                    SavedLocation::filter(SavedLocation::fields().kind().eq(Kind::Recent))
                        .filter(SavedLocation::fields().position().le(cutoff))
                        .delete()
                        .exec(self.tx)
                        .await?;
                }
            }
            Change::Columns { scope, layout } => {
                let key = ViewKey::new(scope)?;
                ViewRecord::upsert_by_kind_and_identity_and_path(key.kind, key.identity, key.path)
                    .columns(layout.clone().map(Json))
                    .exec(self.tx)
                    .await?;
            }
            Change::ViewMode { location, mode } => {
                let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
                ViewRecord::upsert_by_kind_and_identity_and_path(key.kind, key.identity, key.path)
                    .view_mode(Some(record::View::from(*mode)))
                    .exec(self.tx)
                    .await?;
            }
            Change::Widths { location, widths } => {
                let scope = ViewScope::Location(location.clone());
                let key = ViewKey::new(&scope)?;
                let mut saved = ViewRecord::filter_by_kind_and_identity_and_path(
                    key.kind,
                    key.identity,
                    key.path,
                )
                .first()
                .exec(self.tx)
                .await?
                .and_then(|row| row.widths.map(|Json(widths)| widths))
                .unwrap_or_default();
                saved.extend(widths.clone());
                let key = ViewKey::new(&scope)?;
                ViewRecord::upsert_by_kind_and_identity_and_path(key.kind, key.identity, key.path)
                    .widths(Some(Json(saved)))
                    .exec(self.tx)
                    .await?;
            }
            Change::Sort { location, sort } => {
                let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
                ViewRecord::upsert_by_kind_and_identity_and_path(key.kind, key.identity, key.path)
                    .sort(Some(Json(*sort)))
                    .exec(self.tx)
                    .await?;
            }
            Change::Details { location, visible } => {
                let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
                ViewRecord::upsert_by_kind_and_identity_and_path(key.kind, key.identity, key.path)
                    .details(Some(*visible))
                    .exec(self.tx)
                    .await?;
            }
        }
        Ok(())
    }

    async fn save_location(
        &mut self,
        kind: Kind,
        location: &crate::entities::location::Location,
        present: bool,
    ) -> Result<(), Error> {
        let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
        if !present {
            SavedLocation::filter_by_kind_and_location_kind_and_identity_and_path(
                kind,
                key.kind,
                key.identity,
                key.path,
            )
            .delete()
            .exec(self.tx)
            .await?;
            return Ok(());
        }
        let position = SavedLocation::filter(SavedLocation::fields().kind().eq(kind))
            .order_by(SavedLocation::fields().position().desc())
            .select(SavedLocation::fields().position())
            .first()
            .exec(self.tx)
            .await?
            .unwrap_or(0)
            + 1;
        let insert = SavedLocation::upsert_by_kind_and_location_kind_and_identity_and_path(
            kind,
            key.kind,
            key.identity,
            key.path,
        )
        .position(position);
        if kind != Kind::Recent {
            insert.or_ignore().exec(self.tx).await?;
        } else {
            insert.exec(self.tx).await?;
        }
        Ok(())
    }

    async fn save_quick_access(&mut self, location: &Location, present: bool) -> Result<(), Error> {
        let key = ViewKey::new(&ViewScope::Location(location.clone()))?;
        if !present {
            QuickAccessLocation::filter_by_location_kind_and_identity_and_path(
                key.kind,
                key.identity,
                key.path,
            )
            .delete()
            .exec(self.tx)
            .await?;
            return Ok(());
        }
        let position = QuickAccessLocation::all()
            .order_by(QuickAccessLocation::fields().position().desc())
            .select(QuickAccessLocation::fields().position())
            .first()
            .exec(self.tx)
            .await?
            .unwrap_or(-1)
            + 1;
        QuickAccessLocation::upsert_by_location_kind_and_identity_and_path(
            key.kind,
            key.identity,
            key.path,
        )
        .position(position)
        .or_ignore()
        .exec(self.tx)
        .await?;
        Ok(())
    }
}
