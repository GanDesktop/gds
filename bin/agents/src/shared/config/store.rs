//! Agents owns its frontend document; backend configuration is accessed over RPC.

use std::path::{Path, PathBuf};

use gds::persistence::File;
use serde::{Deserialize, Serialize};

use super::{Frontend, appearance::Appearance};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub appearance: Appearance,
    pub frontend: Frontend,
    pub terminal: gds::ui::terminal::Preferences,
}

#[derive(Clone, Debug)]
pub(crate) struct Document {
    pub settings: Settings,
    bytes: Option<Vec<u8>>,
}

#[derive(Clone)]
pub(crate) struct Store(File);

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    File(#[from] gds::persistence::Error),
    #[error("invalid frontend settings at {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },
    #[error("settings {section} changed externally at {path}; reload before saving")]
    Conflict {
        path: PathBuf,
        section: &'static str,
    },
}

impl Store {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        let path = path.unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|dirs| dirs.config_dir().to_owned())
                .unwrap_or_else(|| PathBuf::from(".config"))
                .join("gds/app/agents/settings.json")
        });
        Self(File::new(path))
    }

    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }

    fn invalid(&self, error: impl std::fmt::Display) -> Error {
        Error::Invalid {
            path: self.path().into(),
            reason: error.to_string(),
        }
    }

    /// # Errors
    /// Reports unreadable documents or invalid frontend sections. Legacy daemon fields are ignored.
    pub(crate) fn load(&self) -> Result<Document, Error> {
        let bytes = self.0.read()?;
        let settings: Settings = match &bytes {
            Some(bytes) => serde_json::from_slice(bytes).map_err(|error| self.invalid(error))?,
            None => Settings::default(),
        };
        self.validate(&settings)?;
        Ok(Document { settings, bytes })
    }

    fn validate(&self, settings: &Settings) -> Result<(), Error> {
        settings
            .appearance
            .validate()
            .map_err(|error| self.invalid(error))?;
        settings
            .frontend
            .validate()
            .map_err(|error| self.invalid(error))?;
        settings
            .terminal
            .validate()
            .map_err(|error| self.invalid(error))
    }

    /// # Errors
    /// Rejects invalid appearance, stale sections, managed files and concurrent edits.
    pub(crate) fn save_appearance(
        &self,
        document: &mut Document,
        appearance: Appearance,
    ) -> Result<(), Error> {
        self.save(document, Section::Appearance(appearance))
    }

    /// # Errors
    /// Rejects invalid profiles, stale sections, managed files and concurrent edits.
    pub(crate) fn save_frontend(
        &self,
        document: &mut Document,
        frontend: Frontend,
    ) -> Result<(), Error> {
        self.save(document, Section::Frontend(frontend))
    }

    /// # Errors
    /// Rejects stale terminal sections, managed files and concurrent edits.
    pub(crate) fn save_terminal(
        &self,
        document: &mut Document,
        terminal: gds::ui::terminal::Preferences,
    ) -> Result<(), Error> {
        self.save(document, Section::Terminal(terminal))
    }

    fn save(&self, document: &mut Document, section: Section) -> Result<(), Error> {
        let mut current = self.load()?;
        let name = match section {
            Section::Appearance(appearance) => {
                if current.settings.appearance != document.settings.appearance {
                    return Err(Error::Conflict {
                        path: self.path().into(),
                        section: "appearance",
                    });
                }
                current.settings.appearance = appearance;
                "appearance"
            }
            Section::Frontend(frontend) => {
                if current.settings.frontend != document.settings.frontend {
                    return Err(Error::Conflict {
                        path: self.path().into(),
                        section: "frontend",
                    });
                }
                current.settings.frontend = frontend;
                "frontend"
            }
            Section::Terminal(terminal) => {
                if current.settings.terminal != document.settings.terminal {
                    return Err(Error::Conflict {
                        path: self.path().into(),
                        section: "terminal",
                    });
                }
                current.settings.terminal = terminal;
                "terminal"
            }
        };
        self.validate(&current.settings)?;
        let mut raw: serde_json::Value = match &current.bytes {
            Some(bytes) => serde_json::from_slice(bytes).map_err(|error| self.invalid(error))?,
            None => serde_json::json!({}),
        };
        let value = match name {
            "appearance" => serde_json::to_value(&current.settings.appearance),
            "terminal" => serde_json::to_value(&current.settings.terminal),
            _ => serde_json::to_value(&current.settings.frontend),
        }
        .map_err(|error| self.invalid(error))?;
        raw.as_object_mut()
            .ok_or_else(|| self.invalid("expected an object"))?
            .insert(name.into(), value);
        let mut encoded = serde_json::to_vec_pretty(&raw).map_err(|error| self.invalid(error))?;
        encoded.push(b'\n');
        self.0.write(current.bytes.as_deref(), &encoded)?;
        current.bytes = Some(encoded);
        *document = current;
        Ok(())
    }
}

enum Section {
    Appearance(Appearance),
    Frontend(Frontend),
    Terminal(gds::ui::terminal::Preferences),
}

#[cfg(test)]
mod tests;
