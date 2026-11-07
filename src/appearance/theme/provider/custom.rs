//! User palettes share the built-in TOML schema and live under GDS's theme directory.
use super::{Error, Loaded, Mode, builtin::Definition, read};
use directories::BaseDirs;
use std::path::{Component, Path, PathBuf};

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
pub fn catalog() -> Result<Vec<PathBuf>, Error> {
    catalog_at(&directory()?)
}

fn catalog_at(root: &Path) -> Result<Vec<PathBuf>, Error> {
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
                files.push(
                    entry
                        .path()
                        .strip_prefix(&root)
                        .expect("entry inside theme root")
                        .to_owned(),
                );
            }
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn load(relative: &Path, mode: Mode) -> Result<Loaded, Error> {
    let path = resolve(relative)?;
    parse(&read(&path)?, path, mode)
}

fn parse(text: &str, path: PathBuf, mode: Mode) -> Result<Loaded, Error> {
    let definition: Definition = toml::from_str(text)
        .map_err(|error| Error::invalid(path.display().to_string(), error.to_string()))?;
    Ok(Loaded {
        palette: definition.palette(mode),
        mode,
        source_path: Some(path),
    })
}

#[cfg(test)]
mod tests;
