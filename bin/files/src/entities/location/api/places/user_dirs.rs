//! Pure FreeDesktop `xdg-user-dirs` (`user-dirs.dirs`) parser, resolver, and Quick Access reconciler.
//!
//! Follows the XDG Base Directory Specification and FreeDesktop `xdg-user-dirs`
//! conventions (<https://www.freedesktop.org/wiki/Software/xdg-user-dirs/>):
//! - Resolves `$XDG_CONFIG_HOME/user-dirs.dirs`, ignoring relative
//!   `XDG_CONFIG_HOME` values and falling back to `$HOME/.config/user-dirs.dirs`.
//! - Parses double-quoted `XDG_*_DIR` lines with `$HOME` expansion and POSIX
//!   double-quote escapes (`\"`, `\\`, `\$`, `` \` ``).
//! - Treats paths pointing to `$HOME` itself as disabled (`None`).
//! - Applies standard fallback directory names only when `user-dirs.dirs` does
//!   not exist on disk.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::entities::location::Location;

const USER_DIRS_FILENAME: &str = "user-dirs.dirs";

/// Standard FreeDesktop `xdg-user-dirs` slots tracked for Quick Access sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum XdgUserDirSlot {
    Desktop,
    Downloads,
    Documents,
    Pictures,
    Music,
    Videos,
    Public,
}

impl XdgUserDirSlot {
    pub(crate) const ALL: [Self; 7] = [
        Self::Desktop,
        Self::Downloads,
        Self::Documents,
        Self::Pictures,
        Self::Music,
        Self::Videos,
        Self::Public,
    ];

    /// Default slots seeded once into Quick Access on first launch.
    pub(crate) const DEFAULT_QUICK_ACCESS: [Self; 4] =
        [Self::Desktop, Self::Downloads, Self::Pictures, Self::Videos];

    pub(crate) const fn fallback_name(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Downloads => "Downloads",
            Self::Documents => "Documents",
            Self::Pictures => "Pictures",
            Self::Music => "Music",
            Self::Videos => "Videos",
            Self::Public => "Public",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "XDG_DESKTOP_DIR" => Some(Self::Desktop),
            "XDG_DOWNLOAD_DIR" => Some(Self::Downloads),
            "XDG_DOCUMENTS_DIR" => Some(Self::Documents),
            "XDG_PICTURES_DIR" => Some(Self::Pictures),
            "XDG_MUSIC_DIR" => Some(Self::Music),
            "XDG_VIDEOS_DIR" => Some(Self::Videos),
            "XDG_PUBLICSHARE_DIR" => Some(Self::Public),
            _ => None,
        }
    }
}

/// Resolved path for each standard XDG user directory slot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct XdgUserDirs {
    slots: BTreeMap<XdgUserDirSlot, Option<PathBuf>>,
}

impl PartialEq for XdgUserDirs {
    fn eq(&self, other: &Self) -> bool {
        XdgUserDirSlot::ALL
            .into_iter()
            .all(|slot| self.get(slot) == other.get(slot))
    }
}

impl Eq for XdgUserDirs {}

impl XdgUserDirs {
    /// Resolve XDG user directories from `home` and optional `user-dirs.dirs`
    /// file contents.
    ///
    /// When `config_content` is `Some`, only valid entries declared in the file
    /// are populated (entries pointing to `$HOME` resolve to `None`). When
    /// `config_content` is `None` (`user-dirs.dirs` does not exist), standard
    /// fallback directory names are checked on disk under `home`.
    pub(crate) fn resolve(home: &Path, config_content: Option<&str>) -> Self {
        let normalized_home = normalize_path(home);
        let mut slots = BTreeMap::new();
        match config_content {
            Some(content) => {
                let parsed = parse_user_dirs(&normalized_home, content);
                for slot in XdgUserDirSlot::ALL {
                    let resolved = parsed.get(&slot).and_then(Option::clone);
                    slots.insert(slot, resolved);
                }
            }
            None => {
                for slot in XdgUserDirSlot::ALL {
                    let candidate = normalize_path(&normalized_home.join(slot.fallback_name()));
                    let resolved =
                        (candidate != normalized_home && candidate.is_dir()).then_some(candidate);
                    slots.insert(slot, resolved);
                }
            }
        }
        Self { slots }
    }

    #[cfg(test)]
    pub(crate) fn from_slots(
        entries: impl IntoIterator<Item = (XdgUserDirSlot, Option<PathBuf>)>,
    ) -> Self {
        let mut slots: BTreeMap<XdgUserDirSlot, Option<PathBuf>> = XdgUserDirSlot::ALL
            .into_iter()
            .map(|slot| (slot, None))
            .collect();
        for (slot, path) in entries {
            slots.insert(slot, path.map(|dir| normalize_path(&dir)));
        }
        Self { slots }
    }

    pub(crate) fn get(&self, slot: XdgUserDirSlot) -> Option<&Path> {
        self.slots.get(&slot).and_then(Option::as_deref)
    }

    pub(crate) fn contains_path(&self, path: &Path) -> bool {
        XdgUserDirSlot::ALL
            .into_iter()
            .any(|slot| self.get(slot) == Some(path))
    }

    /// Initial Quick Access locations seeded on first launch: existing
    /// directories for the default Quick Access XDG slots, deduplicated in
    /// canonical slot order.
    pub(crate) fn initial_quick_access_locations(&self) -> Vec<Location> {
        let mut out = Vec::new();
        for slot in XdgUserDirSlot::DEFAULT_QUICK_ACCESS {
            let Some(path) = self.get(slot) else {
                continue;
            };
            if !path.is_dir() {
                continue;
            }
            let location = Location::from(path);
            if !out.contains(&location) {
                out.push(location);
            }
        }
        out
    }
}

/// Concrete home directory and `user-dirs.dirs` configuration file location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserDirsSource {
    pub(crate) home: PathBuf,
    pub(crate) config_path: PathBuf,
}

impl UserDirsSource {
    /// Resolve the active user's home directory and `user-dirs.dirs` path from
    /// process environment and platform user-directory lookup.
    pub(crate) fn current() -> Option<Self> {
        let home = current_home_dir()?;
        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
        let config_path = resolve_config_path(Some(home.as_path()), xdg_config_home.as_deref())?;
        Some(Self { home, config_path })
    }

    /// Read and resolve `user-dirs.dirs`, applying standard fallback names only
    /// when the file does not exist (`ErrorKind::NotFound`).
    ///
    /// # Errors
    /// Reports I/O errors other than `NotFound` so callers avoid replacing a
    /// known snapshot on transient read failures.
    pub(crate) fn load(&self) -> Result<XdgUserDirs, std::io::Error> {
        match std::fs::read_to_string(&self.config_path) {
            Ok(content) => Ok(XdgUserDirs::resolve(&self.home, Some(&content))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(XdgUserDirs::resolve(&self.home, None))
            }
            Err(error) => Err(error),
        }
    }
}

/// Resolve `$XDG_CONFIG_HOME/user-dirs.dirs`, ignoring empty or relative
/// `XDG_CONFIG_HOME` values per the XDG Base Directory Specification and
/// falling back to `$HOME/.config/user-dirs.dirs`.
pub(crate) fn resolve_config_path(
    home: Option<&Path>,
    xdg_config_home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(config_home) = xdg_config_home
        .filter(|value| !value.is_empty())
        .map(Path::new)
        .filter(|path| path.is_absolute())
    {
        return Some(config_home.join(USER_DIRS_FILENAME));
    }
    home.map(|dir| dir.join(".config").join(USER_DIRS_FILENAME))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct QuickAccessEntry {
    pub(super) location: Location,
    pub(super) position: i64,
}

/// Reconcile existing Quick Access entries against a transition from `previous`
/// to `current` XDG user directories.
///
/// Returns `Some(updated)` only when at least one Quick Access entry changed;
/// returns `None` when the existing Quick Access list should stay untouched.
pub(super) fn reconcile_quick_access(
    entries: &[QuickAccessEntry],
    previous: Option<&XdgUserDirs>,
    current: &XdgUserDirs,
    home: Option<&Path>,
) -> Option<Vec<QuickAccessEntry>> {
    let mut relocations: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    let mut removals: BTreeSet<PathBuf> = BTreeSet::new();

    if let Some(previous) = previous {
        if previous == current {
            return None;
        }
        let mut anchored: BTreeSet<&Path> = BTreeSet::new();
        for slot in XdgUserDirSlot::ALL {
            if let (Some(old), Some(new)) = (previous.get(slot), current.get(slot))
                && old == new
            {
                anchored.insert(old);
            }
        }
        for slot in XdgUserDirSlot::ALL {
            let Some(old) = previous.get(slot) else {
                continue;
            };
            if anchored.contains(old) {
                continue;
            }
            match current.get(slot) {
                Some(new) if new != old => {
                    relocations
                        .entry(old.to_path_buf())
                        .or_insert_with(|| new.to_path_buf());
                }
                Some(_) => {}
                None => {
                    if !current.contains_path(old) && !old.is_dir() {
                        removals.insert(old.to_path_buf());
                    }
                }
            }
        }
        for old in relocations.keys() {
            removals.remove(old);
        }
    } else if let Some(home) = home {
        // Legacy database upgrade (`quick_access_seeded == "1"` without a saved
        // `xdg_user_dirs_snapshot`): migrate missing fallback directories that
        // now resolve elsewhere in `user-dirs.dirs`.
        let normalized_home = normalize_path(home);
        for slot in XdgUserDirSlot::ALL {
            let Some(new) = current.get(slot) else {
                continue;
            };
            let fallback = normalize_path(&normalized_home.join(slot.fallback_name()));
            if fallback != normalized_home
                && fallback.as_path() != new
                && !current.contains_path(&fallback)
                && !fallback.is_dir()
            {
                relocations
                    .entry(fallback)
                    .or_insert_with(|| new.to_path_buf());
            }
        }
    }

    if relocations.is_empty() && removals.is_empty() {
        return None;
    }

    let preserved_paths: BTreeSet<&Path> = entries
        .iter()
        .filter_map(|entry| match &entry.location {
            Location::Directory(raw) => {
                let path = Path::new(raw);
                (!relocations.contains_key(path) && !removals.contains(path)).then_some(path)
            }
            _ => None,
        })
        .collect();

    let mut updated = Vec::with_capacity(entries.len());
    let mut claimed: BTreeSet<&Path> = BTreeSet::new();
    for entry in entries {
        match &entry.location {
            Location::Directory(raw) => {
                let path = Path::new(raw);
                if removals.contains(path) {
                    continue;
                }
                if let Some(target) = relocations.get(path) {
                    if !preserved_paths.contains(target.as_path())
                        && claimed.insert(target.as_path())
                    {
                        updated.push(QuickAccessEntry {
                            location: Location::from(target.as_path()),
                            position: entry.position,
                        });
                    }
                } else {
                    updated.push(entry.clone());
                }
            }
            _ => updated.push(entry.clone()),
        }
    }

    (updated != entries).then_some(updated)
}

enum ParsedDir {
    Disabled,
    Path(PathBuf),
}

fn parse_user_dirs(home: &Path, content: &str) -> BTreeMap<XdgUserDirSlot, Option<PathBuf>> {
    let mut parsed = BTreeMap::new();
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let Some(slot) = XdgUserDirSlot::from_key(key.trim()) else {
            continue;
        };
        let Some(entry) = parse_dir_value(home, raw_value.trim()) else {
            continue;
        };
        match entry {
            ParsedDir::Disabled => {
                parsed.insert(slot, None);
            }
            ParsedDir::Path(path) => {
                parsed.insert(slot, Some(path));
            }
        }
    }
    parsed
}

fn parse_dir_value(home: &Path, raw: &str) -> Option<ParsedDir> {
    let rest = raw.strip_prefix('"')?;
    let (base_home, body) = if let Some(after_home) = rest.strip_prefix("$HOME") {
        if after_home.starts_with('"') || after_home.starts_with('/') {
            (true, after_home)
        } else {
            return None;
        }
    } else if rest.starts_with('/') {
        (false, rest)
    } else {
        return None;
    };

    let unescaped = unescape_double_quoted(body)?;
    let resolved = if base_home {
        let suffix = unescaped.trim_start_matches('/');
        if suffix.is_empty() {
            return Some(ParsedDir::Disabled);
        }
        normalize_path(&home.join(suffix))
    } else {
        normalize_path(Path::new(&unescaped))
    };

    if !resolved.is_absolute() {
        return None;
    }
    if resolved == home {
        return Some(ParsedDir::Disabled);
    }
    Some(ParsedDir::Path(resolved))
}

fn unescape_double_quoted(rest: &str) -> Option<String> {
    let mut chars = rest.chars();
    let mut out = String::new();
    let mut closed = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                closed = true;
                break;
            }
            '\\' => {
                let escaped = chars.next()?;
                match escaped {
                    '"' | '\\' | '$' | '`' => out.push(escaped),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
            other => out.push(other),
        }
    }
    if !closed {
        return None;
    }
    let trailing = chars.as_str().trim();
    if !trailing.is_empty() && !trailing.starts_with('#') {
        return None;
    }
    Some(out)
}

fn normalize_path(path: &Path) -> PathBuf {
    path.components().collect()
}

fn current_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| directories::BaseDirs::new().map(|base| base.home_dir().to_path_buf()))
}
