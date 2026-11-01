//! Read complete preferences and palette snapshots before changing live appearance.
use super::preferences::{self, Effective, Preferences};
use crate::shared::config::{Document as SettingsDocument, Store};
use anyhow::Result;
use gpui_kit::WindowAppearance;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct Document {
    pub preferences: Preferences,
    settings: SettingsDocument,
}

#[derive(Clone)]
pub(super) struct Loader {
    pub store: Store,
}

#[derive(Clone, PartialEq)]
pub(super) struct Snapshot {
    preferences: Preferences,
    palette: gds::appearance::theme::Prepared,
}

pub(super) struct Resolved {
    pub effective: Effective,
    pub palette: gds::appearance::theme::Prepared,
}

#[derive(Default)]
pub(super) struct Reload {
    previous: Option<Snapshot>,
    generation: u64,
}

impl Reload {
    pub fn invalidate_on_save(&mut self, generation: u64) {
        if self.generation != generation {
            self.previous = None;
            self.generation = generation;
        }
    }

    /// # Errors
    /// Invalid candidates leave the last successfully resolved snapshot unchanged.
    pub fn update(&mut self, snapshot: Snapshot) -> Result<Option<Resolved>> {
        if self.previous.as_ref() == Some(&snapshot) {
            return Ok(None);
        }
        let resolved = snapshot.resolve()?;
        self.previous = Some(snapshot);
        Ok(Some(resolved))
    }
}

impl Loader {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// # Errors
    /// Reports configuration or desktop palette read/validation failures.
    pub fn read(&self, dark: bool) -> Result<Snapshot> {
        self.snapshot(self.document()?.preferences, dark)
    }

    /// # Errors
    /// Reports unreadable or malformed preferences without replacing them.
    pub fn document(&self) -> Result<Document> {
        let settings = self.store.load()?;
        Ok(Document {
            preferences: settings.settings.appearance.clone(),
            settings,
        })
    }

    fn snapshot(&self, preferences: Preferences, dark: bool) -> Result<Snapshot> {
        preferences::resolve(&preferences)?;
        let platform = if dark {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        };
        let palette = preferences::appearance(&preferences).prepare(platform)?;
        Ok(Snapshot {
            preferences,
            palette,
        })
    }

    /// # Errors
    /// Rejects invalid preferences, unavailable palettes, external edits and refused writes.
    pub fn save(
        &self,
        document: &mut Document,
        preferences: Preferences,
        dark: bool,
    ) -> Result<Resolved> {
        let resolved = self.snapshot(preferences.clone(), dark)?.resolve()?;
        self.store
            .save_appearance(&mut document.settings, preferences.clone())?;
        document.preferences = preferences;
        Ok(resolved)
    }
}

impl Snapshot {
    pub fn source_paths(&self) -> Vec<std::path::PathBuf> {
        self.palette.source_paths()
    }

    pub(super) fn resolve(&self) -> Result<Resolved> {
        Ok(Resolved {
            effective: preferences::resolve(&self.preferences)?,
            palette: self.palette.clone(),
        })
    }
}
