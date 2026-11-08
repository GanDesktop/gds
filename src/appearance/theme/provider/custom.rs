//! User palettes share the built-in TOML schema and live under GDS's theme directory.
use super::{Error, Loaded, Mode, builtin::Definition, read};
use directories::BaseDirs;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;

/// Self-description of a custom theme. Only the default name is required.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub name: String,
    pub schema_version: Option<u32>,
    pub author: Option<String>,
    #[serde(default)]
    pub i18n: BTreeMap<String, String>,
}

impl Metadata {
    pub fn display_name(&self, locale: &str) -> &str {
        self.i18n
            .iter()
            .find(|(language, name)| {
                language.eq_ignore_ascii_case(locale) && !name.trim().is_empty()
            })
            .map_or(&self.name, |(_, name)| name)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.name.trim().is_empty() {
            return Err(Error::invalid(
                "metadata.name",
                "a default theme name is required",
            ));
        }
        if self
            .schema_version
            .is_some_and(|version| version != SCHEMA_VERSION)
        {
            return Err(Error::invalid(
                "metadata.schema_version",
                "unsupported schema version",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub metadata: Metadata,
}

/// # Errors
/// Reports an unavailable user configuration directory.
pub fn directory() -> Result<PathBuf, Error> {
    Ok(BaseDirs::new()
        .ok_or(Error::NoUserDirectories)?
        .config_dir()
        .join("gds/themes"))
}

/// # Errors
/// Rejects paths outside the shared theme directory.
pub fn resolve(relative: &Path) -> Result<PathBuf, Error> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::invalid(
            "theme path",
            "expected a relative path inside gds/themes",
        ));
    }
    Ok(directory()?.join(relative))
}

/// Discover TOML palettes, including theme subdirectories.
/// # Errors
/// Reports inaccessible theme directories; a missing directory is an empty catalog.
pub fn catalog() -> Result<Vec<Entry>, Error> {
    catalog_at(&directory()?)
}

fn catalog_at(root: &Path) -> Result<Vec<Entry>, Error> {
    let root = root.to_owned();
    let mut pending = vec![root.clone()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(Error::Read { path, source }),
        };
        for entry in entries {
            let entry = entry.map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })?;
            let kind = entry.file_type().map_err(|source| Error::Read {
                path: entry.path(),
                source,
            })?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                let path = entry.path();
                match read(&path).and_then(|text| definition(&text, &path)) {
                    Ok(definition) => files.push(Entry {
                        path: path
                            .strip_prefix(&root)
                            .expect("entry inside theme root")
                            .to_owned(),
                        metadata: definition.metadata.expect("validated metadata"),
                    }),
                    Err(error) => tracing::warn!(%error, "ignoring invalid custom theme"),
                }
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub(super) fn load(relative: &Path, mode: Mode) -> Result<Loaded, Error> {
    let path = resolve(relative)?;
    parse(&read(&path)?, path, mode)
}

fn parse(text: &str, path: PathBuf, mode: Mode) -> Result<Loaded, Error> {
    let definition = definition(text, &path)?;
    Ok(Loaded {
        palette: definition.palette(mode),
        mode,
        source_path: Some(path),
    })
}

fn definition(text: &str, path: &Path) -> Result<Definition, Error> {
    let definition: Definition = toml::from_str(text)
        .map_err(|error| Error::invalid(path.display().to_string(), error.to_string()))?;
    definition
        .metadata
        .as_ref()
        .ok_or_else(|| Error::invalid("metadata.name", "a default theme name is required"))?
        .validate()?;
    Ok(definition)
}

#[cfg(test)]
mod tests;
