use std::ffi::OsStr;

use gds::service::storage::Transaction;
use toasty::Json;

use super::super::super::model::places::{Change, Places, RECENT_LIMIT};
use super::super::super::model::view::LocationView;
use super::{
    Error,
    record::{self, Kind, Location, SavedLocation, ViewKey, ViewSettings as ViewRecord},
};
use crate::entities::location::{ViewScope, ViewSettings};

pub(super) struct Accessor<'tx, 'db> {
    tx: &'tx mut Transaction<'db>,
}

impl<'tx, 'db> Accessor<'tx, 'db> {
    pub(super) fn new(tx: &'tx mut Transaction<'db>) -> Self {
        Self { tx }
    }

    /// # Errors
    /// Reports database failures while creating the application defaults.
    pub(super) async fn initialize(&mut self) -> Result<(), Error> {
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
                    Kind::Pin => unreachable!("pins use filesystem records"),
                }
            }
        }
        places.recent.reverse();
        places.recent.truncate(RECENT_LIMIT);
        for row in Location::filter(Location::fields().kind().eq(Kind::Pin))
            .order_by(Location::fields().position().asc())
            .exec(self.tx)
            .await?
        {
            places.pinned.push(serde_json::from_slice(&row.path)?);
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
            Change::Pin { path, present } => self.membership(Kind::Pin, path, *present).await?,
            Change::PinOrder(order) => {
                let records = Location::filter(Location::fields().kind().eq(Kind::Pin))
                    .order_by(Location::fields().position().asc())
                    .exec(self.tx)
                    .await?;
                let mut paths = Vec::new();
                for row in records {
                    paths.push(serde_json::from_slice(&row.path)?);
                }
                crate::entities::location::model::places::reorder_existing(&mut paths, order);
                for (position, path) in paths.iter().enumerate() {
                    Location::upsert_by_kind_and_path(Kind::Pin, path_key(path)?)
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
        if kind == Kind::Favorite {
            insert.or_ignore().exec(self.tx).await?;
        } else {
            insert.exec(self.tx).await?;
        }
        Ok(())
    }

    async fn membership(&mut self, kind: Kind, path: &OsStr, present: bool) -> Result<(), Error> {
        let key = path_key(path)?;
        if present {
            let position = self.next_position(kind).await?;
            Location::upsert_by_kind_and_path(kind, key)
                .position(position)
                .or_ignore()
                .exec(self.tx)
                .await?;
        } else {
            Location::filter_by_kind_and_path(kind, key)
                .delete()
                .exec(self.tx)
                .await?;
        }
        Ok(())
    }

    async fn next_position(&mut self, kind: Kind) -> Result<i64, Error> {
        let last = Location::filter(Location::fields().kind().eq(kind))
            .order_by(Location::fields().position().desc())
            .select(Location::fields().position())
            .first()
            .exec(self.tx)
            .await?;
        Ok(last.map_or(1, |position| position + 1))
    }
}

fn path_key(path: &OsStr) -> Result<Vec<u8>, Error> {
    Ok(serde_json::to_vec(path)?)
}
