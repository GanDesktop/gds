//! Shared MIME database backend preferring binary `mime.cache` with plain-text fallback.
//!
//! Loads XDG MIME directories in specification precedence order
//! (`$XDG_DATA_HOME/mime`, then `$XDG_DATA_DIRS/*/mime`), using `mmap`ed
//! `mime.cache` v1.2 when available and falling back to plain-text index files
//! (`globs2`, `aliases`, `subclasses`, `XMLnamespaces`, `icons`, `generic-icons`,
//! `magic`) otherwise.

mod cache;
mod text;

#[cfg(test)]
use std::path::Path;
use std::{
    borrow::Cow,
    collections::{HashSet, VecDeque},
    env,
    ffi::OsStr,
    path::PathBuf,
};

use directories::BaseDirs;

use cache::MimeCache;
use text::TextIndex;

/// Sentinel pattern emitted by `update-mime-database` for `<glob-deleteall/>`.
pub(super) const NOGLOBS_SENTINEL: &str = "__NOGLOBS__";

const DEFAULT_XDG_DATA_DIRS: [&str; 2] = ["/usr/local/share", "/usr/share"];

/// Resolve XDG data base directories per the XDG Base Directory Specification,
/// ignoring relative paths and falling back to specification defaults.
pub(super) fn resolve_xdg_base_dirs(
    xdg_data_home: Option<&OsStr>,
    xdg_data_dirs: Option<&OsStr>,
    default_data_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut base_dirs = Vec::new();
    let home_data = xdg_data_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or(default_data_home);
    if let Some(home_data) = home_data {
        base_dirs.push(home_data);
    }

    let system_dirs: Vec<PathBuf> = xdg_data_dirs
        .filter(|value| !value.is_empty())
        .map(|value| {
            env::split_paths(value)
                .filter(|path| path.is_absolute())
                .collect::<Vec<_>>()
        })
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| {
            DEFAULT_XDG_DATA_DIRS
                .into_iter()
                .map(PathBuf::from)
                .collect()
        });
    base_dirs.extend(system_dirs);

    base_dirs
}

#[derive(Debug, Clone)]
pub(super) struct GlobMatch {
    pub(super) mime: String,
    pub(super) weight: u32,
    pub(super) pattern_len: usize,
}

enum DirectoryIndex {
    Cache(MimeCache),
    Text(TextIndex),
}

struct LoadedDirectory {
    index: DirectoryIndex,
}

pub(super) struct MimeDatabase {
    directories: Vec<LoadedDirectory>,
    mime_dirs: Vec<PathBuf>,
}

impl MimeDatabase {
    /// Load all available XDG shared MIME databases in precedence order.
    pub(super) fn from_xdg() -> Self {
        let xdg_data_home = env::var_os("XDG_DATA_HOME");
        let xdg_data_dirs = env::var_os("XDG_DATA_DIRS");
        let default_data_home = BaseDirs::new().map(|dirs| dirs.data_dir().to_path_buf());
        let base_dirs = resolve_xdg_base_dirs(
            xdg_data_home.as_deref(),
            xdg_data_dirs.as_deref(),
            default_data_home,
        );
        Self::from_base_directories(&base_dirs)
    }

    /// Load a single base directory containing a `mime/` subdirectory (primarily for tests).
    #[cfg(test)]
    pub(super) fn from_directory(base_dir: &Path) -> Self {
        Self::from_base_directories(&[base_dir.to_path_buf()])
    }

    pub(super) fn from_base_directories(base_dirs: &[PathBuf]) -> Self {
        let mut directories = Vec::new();
        let mut mime_dirs = Vec::new();
        let mut seen = HashSet::new();

        for base_dir in base_dirs {
            let mime_dir = base_dir.join("mime");
            if !seen.insert(mime_dir.clone()) || !mime_dir.is_dir() {
                continue;
            }
            let index = match MimeCache::load(&mime_dir) {
                Some(cache) => DirectoryIndex::Cache(cache),
                None => DirectoryIndex::Text(TextIndex::load(base_dir)),
            };
            mime_dirs.push(mime_dir);
            directories.push(LoadedDirectory { index });
        }

        Self {
            directories,
            mime_dirs,
        }
    }

    /// XDG `mime/` directories in precedence order, shared with per-type XML lookups.
    pub(super) fn mime_dirs(&self) -> &[PathBuf] {
        &self.mime_dirs
    }

    /// Normalize a MIME alias to its canonical MIME string.
    pub(super) fn unalias<'a>(&'a self, mime: &'a str) -> Cow<'a, str> {
        let mut current = mime;
        for _ in 0..8 {
            let mut next = None;
            for dir in &self.directories {
                let candidate = match &dir.index {
                    DirectoryIndex::Cache(cache) => cache.unalias(current),
                    DirectoryIndex::Text(text) => text.unalias(current),
                };
                if let Some(found) = candidate {
                    next = Some(found);
                    break;
                }
            }
            match next {
                Some(resolved) if resolved != current => current = resolved,
                _ => break,
            }
        }
        Cow::Borrowed(current)
    }

    /// Match `file_name` against installed filename rules, keeping highest weight
    /// then longest pattern, normalizing aliases, honoring `<glob-deleteall/>`
    /// (`__NOGLOBS__`) suppression across XDG layers, deduplicating, and dropping
    /// `application/octet-stream`. The returned vector is deterministically sorted.
    pub(super) fn filename_candidates(&self, file_name: &str) -> Vec<String> {
        let mut raw = Vec::new();
        let mut suppressed: HashSet<String> = HashSet::new();
        for dir in &self.directories {
            let start_len = raw.len();
            match &dir.index {
                DirectoryIndex::Cache(cache) => cache.match_filename(file_name, &mut raw),
                DirectoryIndex::Text(text) => text.match_filename(file_name, &mut raw),
            }
            if !suppressed.is_empty() && raw.len() > start_len {
                let mut idx = start_len;
                while idx < raw.len() {
                    if suppressed.contains(self.unalias(&raw[idx].mime).as_ref()) {
                        raw.swap_remove(idx);
                    } else {
                        idx += 1;
                    }
                }
            }
            let noglobs = match &dir.index {
                DirectoryIndex::Cache(cache) => cache.noglobs(),
                DirectoryIndex::Text(text) => text.noglobs(),
            };
            for mime in noglobs {
                suppressed.insert(self.unalias(mime).into_owned());
            }
        }
        let Some(max_weight) = raw.iter().map(|m| m.weight).max() else {
            return Vec::new();
        };
        let Some(max_len) = raw
            .iter()
            .filter(|m| m.weight == max_weight)
            .map(|m| m.pattern_len)
            .max()
        else {
            return Vec::new();
        };

        let mut canonical: Vec<String> = raw
            .into_iter()
            .filter(|m| m.weight == max_weight && m.pattern_len == max_len)
            .map(|m| self.unalias(&m.mime).into_owned())
            .filter(|mime| mime != "application/octet-stream")
            .collect();
        canonical.sort();
        canonical.dedup();
        canonical
    }

    /// Direct parent MIME types declared in `subclasses` for canonical `mime`.
    pub(super) fn direct_parents(&self, mime: &str) -> Vec<String> {
        let canonical = self.unalias(mime);
        let mut parents = Vec::new();
        for dir in &self.directories {
            let list = match &dir.index {
                DirectoryIndex::Cache(cache) => cache.parents(canonical.as_ref()),
                DirectoryIndex::Text(text) => text.parents(canonical.as_ref()),
            };
            for parent in list {
                let normalized = self.unalias(parent).into_owned();
                if !parents.contains(&normalized) {
                    parents.push(normalized);
                }
            }
        }
        parents
    }

    /// Whether `child` is equal to or a subclass of `ancestor` according to XDG MIME rules.
    pub(super) fn is_subclass(&self, child: &str, ancestor: &str) -> bool {
        let child = self.unalias(child);
        let ancestor = self.unalias(ancestor);
        if child == ancestor || ancestor == "*/*" {
            return true;
        }
        // Per shared-mime-info spec:
        // - All streamable types (everything except `inode/*`) are subclasses of `application/octet-stream`.
        // - All `text/*` types are subclasses of `text/plain`.
        if ancestor == "application/octet-stream" && !child.starts_with("inode/") {
            return true;
        }
        if matches_supertype(child.as_ref(), ancestor.as_ref()) {
            return true;
        }
        if ancestor == "text/plain" && child.starts_with("text/") {
            return true;
        }

        let child_owned = child.into_owned();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(child_owned.clone());
        queue.push_back(child_owned);

        while let Some(current) = queue.pop_front() {
            for parent in self.direct_parents(&current) {
                if parent == ancestor.as_ref()
                    || matches_supertype(&parent, ancestor.as_ref())
                    || (ancestor == "text/plain" && parent.starts_with("text/"))
                    || (ancestor == "application/octet-stream" && !parent.starts_with("inode/"))
                {
                    return true;
                }
                if visited.insert(parent.clone()) {
                    queue.push_back(parent);
                }
            }
        }
        false
    }

    /// Explicit `icons` entry declared directly for `mime` (never synthesized).
    pub(super) fn explicit_icon(&self, mime: &str) -> Option<String> {
        let canonical = self.unalias(mime);
        for dir in &self.directories {
            let found = match &dir.index {
                DirectoryIndex::Cache(cache) => cache.icon(canonical.as_ref()),
                DirectoryIndex::Text(text) => text.icon(canonical.as_ref()),
            };
            if let Some(icon) = found {
                return Some(icon.to_owned());
            }
        }
        None
    }

    /// Match an XML root element's `(namespace_uri, local_name)` against installed
    /// `XMLnamespaces` / `mime.cache` `NamespaceList` entries in XDG precedence order.
    pub(super) fn match_xml_namespace(
        &self,
        namespace_uri: &str,
        local_name: &str,
    ) -> Option<String> {
        if namespace_uri.is_empty() && local_name.is_empty() {
            return None;
        }
        if let Some(mime) = self.lookup_xml_namespace_exact(namespace_uri, local_name) {
            return Some(mime);
        }
        if !namespace_uri.is_empty() && !local_name.is_empty() {
            if let Some(mime) = self.lookup_xml_namespace_exact(namespace_uri, "") {
                return Some(mime);
            }
            if let Some(mime) = self.lookup_xml_namespace_exact("", local_name) {
                return Some(mime);
            }
        }
        None
    }

    fn lookup_xml_namespace_exact(&self, namespace_uri: &str, local_name: &str) -> Option<String> {
        for dir in &self.directories {
            let found = match &dir.index {
                DirectoryIndex::Cache(cache) => cache.xml_namespace(namespace_uri, local_name),
                DirectoryIndex::Text(text) => text.xml_namespace(namespace_uri, local_name),
            };
            if let Some(mime) = found {
                return Some(self.unalias(mime).into_owned());
            }
        }
        None
    }

    /// Explicit `generic-icons` entry declared directly for `mime` (never synthesized).
    pub(super) fn explicit_generic_icon(&self, mime: &str) -> Option<String> {
        let canonical = self.unalias(mime);
        for dir in &self.directories {
            let found = match &dir.index {
                DirectoryIndex::Cache(cache) => cache.generic_icon(canonical.as_ref()),
                DirectoryIndex::Text(text) => text.generic_icon(canonical.as_ref()),
            };
            if let Some(icon) = found {
                return Some(icon.to_owned());
            }
        }
        None
    }

    /// Explicit `generic-icons` entry inherited from the nearest ancestor in `subclasses`.
    pub(super) fn inherited_generic_icon(&self, mime: &str) -> Option<String> {
        let canonical = self.unalias(mime).into_owned();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(canonical.clone());
        for parent in self.direct_parents(&canonical) {
            if visited.insert(parent.clone()) {
                queue.push_back(parent);
            }
        }
        while let Some(current) = queue.pop_front() {
            if let Some(icon) = self.explicit_generic_icon(&current) {
                return Some(icon);
            }
            for parent in self.direct_parents(&current) {
                if visited.insert(parent.clone()) {
                    queue.push_back(parent);
                }
            }
        }
        None
    }

    /// If one candidate in `candidates` is a superclass of every other candidate
    /// (e.g. `application/json` covers `application/schema+json`), return it.
    pub(super) fn common_superclass(&self, candidates: &[String]) -> Option<String> {
        candidates
            .iter()
            .find(|base| {
                candidates
                    .iter()
                    .all(|other| other == *base || self.is_subclass(other, base))
            })
            .cloned()
    }

    /// Find a non-generic shared parent across all `candidates` (e.g. `audio/ogg`
    /// and `video/ogg` share `application/ogg`), excluding `application/octet-stream`
    /// and `text/plain`.
    pub(super) fn shared_parent(&self, candidates: &[String]) -> Option<String> {
        let first = candidates.first()?;
        let rest = &candidates[1..];
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(first.clone());
        for parent in self.direct_parents(first) {
            if visited.insert(parent.clone()) {
                queue.push_back(parent);
            }
        }
        while let Some(parent) = queue.pop_front() {
            if parent != "application/octet-stream"
                && parent != "text/plain"
                && rest.iter().all(|other| self.is_subclass(other, &parent))
            {
                return Some(parent);
            }
            for grand in self.direct_parents(&parent) {
                if visited.insert(grand.clone()) {
                    queue.push_back(grand);
                }
            }
        }
        None
    }

    /// Sniff `sample` against installed `magic` rules, returning the highest-priority
    /// canonical MIME match and its priority.
    pub(super) fn sniff_data(&self, sample: &[u8]) -> Option<(String, u32)> {
        if sample.is_empty() {
            return None;
        }
        let mut best: Option<(String, u32)> = None;
        for dir in &self.directories {
            let matched = match &dir.index {
                DirectoryIndex::Cache(cache) => cache
                    .match_data(sample)
                    .map(|(mime, prio)| (mime.to_owned(), prio)),
                DirectoryIndex::Text(text) => text.match_data(sample),
            };
            if let Some((mime, prio)) = matched {
                let replace = best.as_ref().is_none_or(|(_, best_prio)| prio > *best_prio);
                if replace {
                    let canonical = self.unalias(&mime).into_owned();
                    best = Some((canonical, prio));
                }
            }
        }
        best
    }

    #[cfg(test)]
    pub(super) fn uses_binary_cache(&self) -> bool {
        self.directories
            .iter()
            .any(|dir| matches!(dir.index, DirectoryIndex::Cache(_)))
    }
}

fn matches_supertype(mime: &str, ancestor: &str) -> bool {
    ancestor
        .strip_suffix("/*")
        .is_some_and(|prefix| mime.split_once('/').is_some_and(|(top, _)| top == prefix))
}
