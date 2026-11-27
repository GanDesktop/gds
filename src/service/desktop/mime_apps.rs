//! FreeDesktop MIME Applications Associations 1.0 (`mimeapps.list`) and Desktop Entry discovery.
//!
//! Resolves default and recommended applications strictly following the XDG search precedence,
//! Shared MIME-Info alias/subclass inheritance, `Hidden` shadowing, `OnlyShowIn`/`NotShowIn`, and
//! `TryExec` validation, and updates `~/.config/mimeapps.list` losslessly and atomically.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    env, fs, io,
    path::{Path, PathBuf},
};

/// One installed application parsed from a FreeDesktop `.desktop` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopApp {
    /// Spec desktop file ID (e.g. `org.gnome.TextEditor.desktop` or `kde-kate.desktop`).
    pub id: String,
    /// Localized display name.
    pub name: String,
    /// Optional localized generic description.
    pub generic_name: Option<String>,
    /// Declared MIME types from `MimeType=`.
    pub mime_types: Vec<String>,
    /// Whether `NoDisplay=true` is set.
    pub no_display: bool,
}

/// Resolved application associations for one MIME type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MimeAssociations {
    /// Canonicalized MIME type queried.
    pub mime_type: String,
    /// Current default application, if any installed application matches.
    pub default_app: Option<DesktopApp>,
    /// Ordered candidate applications matching this MIME type or its parent subclasses.
    pub recommended: Vec<DesktopApp>,
    /// All visible installed desktop applications (`!no_display`), sorted by display name.
    pub all: Vec<DesktopApp>,
}

/// Injectable XDG environment paths used for resolution and testing.
#[derive(Clone, Debug)]
pub struct Environment {
    pub config_home: PathBuf,
    pub config_dirs: Vec<PathBuf>,
    pub data_home: PathBuf,
    pub data_dirs: Vec<PathBuf>,
    pub desktops: Vec<String>,
    pub locales: Vec<String>,
    pub path_dirs: Vec<PathBuf>,
}

impl Environment {
    /// Capture the current process's XDG Base Directory, desktop, locale, and `PATH` settings.
    #[must_use]
    pub fn from_system() -> Option<Self> {
        let base = directories::BaseDirs::new()?;
        let config_home =
            env_abs_path("XDG_CONFIG_HOME").unwrap_or_else(|| base.config_dir().to_path_buf());
        let data_home =
            env_abs_path("XDG_DATA_HOME").unwrap_or_else(|| base.data_local_dir().to_path_buf());
        let config_dirs =
            env_path_list("XDG_CONFIG_DIRS").unwrap_or_else(|| vec![PathBuf::from("/etc/xdg")]);
        let data_dirs = env_path_list("XDG_DATA_DIRS").unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
        let desktops = env::var("XDG_CURRENT_DESKTOP")
            .ok()
            .map(|raw| {
                raw.split(':')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let locales = system_locales();
        let path_dirs = env::var_os("PATH")
            .map(|raw| env::split_paths(&raw).collect())
            .unwrap_or_default();
        Some(Self {
            config_home,
            config_dirs,
            data_home,
            data_dirs,
            desktops,
            locales,
            path_dirs,
        })
    }

    /// Return candidate `mimeapps.list` paths in exact XDG MIME Applications Associations 1.0 order.
    #[must_use]
    pub fn mimeapps_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let lower_desktops: Vec<String> = self
            .desktops
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();

        push_mimeapps_dir(&mut out, &self.config_home, &lower_desktops);
        for dir in &self.config_dirs {
            push_mimeapps_dir(&mut out, dir, &lower_desktops);
        }
        push_mimeapps_dir(
            &mut out,
            &self.data_home.join("applications"),
            &lower_desktops,
        );
        for dir in &self.data_dirs {
            push_mimeapps_dir(&mut out, &dir.join("applications"), &lower_desktops);
        }
        out
    }

    fn application_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::with_capacity(1 + self.data_dirs.len());
        dirs.push(self.data_home.join("applications"));
        for dir in &self.data_dirs {
            dirs.push(dir.join("applications"));
        }
        dirs
    }

    fn mime_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::with_capacity(1 + self.data_dirs.len());
        dirs.push(self.data_home.join("mime"));
        for dir in &self.data_dirs {
            dirs.push(dir.join("mime"));
        }
        dirs
    }

    fn user_mimeapps_path(&self) -> PathBuf {
        self.config_home.join("mimeapps.list")
    }

    fn user_desktop_mimeapps_paths(&self) -> Vec<PathBuf> {
        self.desktops
            .iter()
            .map(|desktop| {
                self.config_home
                    .join(format!("{}-mimeapps.list", desktop.to_ascii_lowercase()))
            })
            .collect()
    }
}

fn push_mimeapps_dir(out: &mut Vec<PathBuf>, dir: &Path, lower_desktops: &[String]) {
    for desktop in lower_desktops {
        out.push(dir.join(format!("{desktop}-mimeapps.list")));
    }
    out.push(dir.join("mimeapps.list"));
}

fn env_abs_path(name: &str) -> Option<PathBuf> {
    let raw = env::var_os(name)?;
    let path = PathBuf::from(raw);
    path.is_absolute().then_some(path)
}

fn env_path_list(name: &str) -> Option<Vec<PathBuf>> {
    let raw = env::var_os(name)?;
    let dirs: Vec<PathBuf> = env::split_paths(&raw)
        .filter(|path| path.is_absolute())
        .collect();
    (!dirs.is_empty()).then_some(dirs)
}

fn system_locales() -> Vec<String> {
    let mut locales = Vec::new();
    let raw = env::var("LC_ALL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| env::var("LC_MESSAGES").ok().filter(|s| !s.is_empty()))
        .or_else(|| env::var("LANG").ok().filter(|s| !s.is_empty()));
    let Some(raw) = raw else {
        return locales;
    };
    let base = raw.split('.').next().unwrap_or(&raw);
    let (lang_country, modifier) = match base.split_once('@') {
        Some((lc, m)) => (lc, Some(m)),
        None => (base, None),
    };
    let (lang, country) = match lang_country.split_once('_') {
        Some((l, c)) => (l, Some(c)),
        None => (lang_country, None),
    };
    if lang.is_empty() || lang.eq_ignore_ascii_case("C") || lang.eq_ignore_ascii_case("POSIX") {
        return locales;
    }
    if let (Some(c), Some(m)) = (country, modifier) {
        locales.push(format!("{lang}_{c}@{m}"));
    }
    if let Some(c) = country {
        locales.push(format!("{lang}_{c}"));
    }
    if let Some(m) = modifier {
        locales.push(format!("{lang}@{m}"));
    }
    locales.push(lang.to_owned());
    locales
}

#[derive(Default)]
struct MimeAppsFile {
    defaults: HashMap<String, Vec<String>>,
    added: HashMap<String, Vec<String>>,
    removed: HashMap<String, Vec<String>>,
}

#[derive(Default)]
struct MimeHierarchy {
    aliases: HashMap<String, String>,
    parents: HashMap<String, Vec<String>>,
}

impl MimeHierarchy {
    fn load(mime_dirs: &[PathBuf]) -> Self {
        let mut hierarchy = Self::default();
        for dir in mime_dirs {
            if let Ok(content) = fs::read_to_string(dir.join("aliases")) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let mut parts = line.split_whitespace();
                    if let (Some(alias), Some(canonical)) = (parts.next(), parts.next()) {
                        hierarchy
                            .aliases
                            .entry(alias.to_ascii_lowercase())
                            .or_insert_with(|| canonical.to_ascii_lowercase());
                    }
                }
            }
            if let Ok(content) = fs::read_to_string(dir.join("subclasses")) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let mut parts = line.split_whitespace();
                    if let (Some(child), Some(parent)) = (parts.next(), parts.next()) {
                        let list = hierarchy
                            .parents
                            .entry(child.to_ascii_lowercase())
                            .or_default();
                        let parent_lower = parent.to_ascii_lowercase();
                        if !list.contains(&parent_lower) {
                            list.push(parent_lower);
                        }
                    }
                }
            }
        }
        hierarchy
    }

    fn canonical(&self, raw_mime: &str) -> String {
        let lower = raw_mime.trim().to_ascii_lowercase();
        self.aliases.get(&lower).cloned().unwrap_or(lower)
    }

    fn chain(&self, raw_mime: &str) -> Vec<String> {
        let start = self.canonical(raw_mime);
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        if visited.insert(start.clone()) {
            queue.push_back(start);
        }
        while let Some(current) = queue.pop_front() {
            if let Some(parents) = self.parents.get(&current) {
                for parent in parents {
                    let canon = self.canonical(parent);
                    if visited.insert(canon.clone()) {
                        queue.push_back(canon);
                    }
                }
            }
            // Shared MIME-Info spec: every `text/*` type implicitly subclasses `text/plain`.
            if current.starts_with("text/")
                && current != "text/plain"
                && visited.insert("text/plain".to_owned())
            {
                queue.push_back("text/plain".to_owned());
            }
            result.push(current);
        }
        result
    }
}

/// Snapshot of desktop applications, MIME hierarchy, and parsed `mimeapps.list` layers.
pub struct Registry {
    apps: HashMap<String, DesktopApp>,
    mimeapps: Vec<MimeAppsFile>,
    hierarchy: MimeHierarchy,
}

impl Registry {
    /// Load desktop application associations from the given XDG [`Environment`].
    #[must_use]
    pub fn load(env: &Environment) -> Self {
        let apps = load_applications(env);
        let mimeapps = env
            .mimeapps_paths()
            .into_iter()
            .filter_map(|path| fs::read_to_string(&path).ok())
            .map(|text| parse_mimeapps_list(&text))
            .collect();
        let hierarchy = MimeHierarchy::load(&env.mime_dirs());
        Self {
            apps,
            mimeapps,
            hierarchy,
        }
    }

    /// Resolve the current default application, recommended candidates, and all visible apps
    /// for `mime_type`.
    #[must_use]
    pub fn resolve(&self, mime_type: &str) -> MimeAssociations {
        let chain = self.hierarchy.chain(mime_type);
        let canonical = chain
            .first()
            .cloned()
            .unwrap_or_else(|| mime_type.trim().to_ascii_lowercase());

        let recommended_ids = self.recommended_ids(&chain);
        let default_id = self
            .explicit_default_id(&chain)
            .or_else(|| recommended_ids.first().cloned());

        let default_app = default_id
            .as_deref()
            .and_then(|id| self.apps.get(id))
            .cloned();
        let recommended: Vec<DesktopApp> = recommended_ids
            .iter()
            .filter_map(|id| self.apps.get(id).cloned())
            .collect();

        let mut all: Vec<DesktopApp> = self
            .apps
            .values()
            .filter(|app| !app.no_display)
            .cloned()
            .collect();
        all.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });

        MimeAssociations {
            mime_type: canonical,
            default_app,
            recommended,
            all,
        }
    }

    fn explicit_default_id(&self, chain: &[String]) -> Option<String> {
        for mime in chain {
            for file in &self.mimeapps {
                if let Some(candidates) = file.defaults.get(mime) {
                    for id in candidates {
                        if self.apps.contains_key(id) {
                            return Some(id.clone());
                        }
                    }
                }
            }
        }
        None
    }

    fn recommended_ids(&self, chain: &[String]) -> Vec<String> {
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();

        for mime in chain {
            let mut blacklisted = HashSet::new();
            for file in &self.mimeapps {
                if let Some(defaults) = file.defaults.get(mime) {
                    for id in defaults {
                        if !blacklisted.contains(id)
                            && self.apps.contains_key(id)
                            && seen.insert(id.clone())
                        {
                            ordered.push(id.clone());
                        }
                    }
                }
                if let Some(added) = file.added.get(mime) {
                    for id in added {
                        if !blacklisted.contains(id)
                            && self.apps.contains_key(id)
                            && seen.insert(id.clone())
                        {
                            ordered.push(id.clone());
                        }
                    }
                }
                if let Some(removed) = file.removed.get(mime) {
                    for id in removed {
                        blacklisted.insert(id.clone());
                    }
                }
            }

            let mut desktop_matches: Vec<&DesktopApp> = self
                .apps
                .values()
                .filter(|app| {
                    !app.no_display
                        && !blacklisted.contains(&app.id)
                        && app
                            .mime_types
                            .iter()
                            .any(|declared| self.hierarchy.canonical(declared) == *mime)
                })
                .collect();
            desktop_matches.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.id.cmp(&b.id))
            });
            for app in desktop_matches {
                if seen.insert(app.id.clone()) {
                    ordered.push(app.id.clone());
                }
            }
        }

        ordered
    }
}

/// Resolve the default and candidate applications for `mime_type` using the system XDG environment.
#[must_use]
pub fn resolve_system(mime_type: &str) -> Option<MimeAssociations> {
    let env = Environment::from_system()?;
    Some(Registry::load(&env).resolve(mime_type))
}

/// Persist `desktop_id` as the user's default application for `mime_type` in
/// `$XDG_CONFIG_HOME/mimeapps.list` (and any existing desktop-prefixed user override file).
///
/// # Errors
/// Rejects invalid MIME types or desktop IDs and reports filesystem write failures.
pub fn set_default(mime_type: &str, desktop_id: &str) -> io::Result<()> {
    let env = Environment::from_system().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "cannot determine XDG configuration directory",
        )
    })?;
    set_default_in(&env, mime_type, desktop_id)
}

/// Persist `desktop_id` as the default application for `mime_type` inside `env.config_home`.
///
/// # Errors
/// Rejects invalid MIME types or desktop IDs and reports filesystem write failures.
pub fn set_default_in(env: &Environment, mime_type: &str, desktop_id: &str) -> io::Result<()> {
    let mime_trimmed = mime_type.trim().to_ascii_lowercase();
    let id_trimmed = desktop_id.trim();
    if mime_trimmed.is_empty()
        || !mime_trimmed.contains('/')
        || mime_trimmed.contains(['=', ';', '\n', '\r'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid MIME type",
        ));
    }
    if id_trimmed.is_empty()
        || !id_trimmed.ends_with(".desktop")
        || id_trimmed.contains(['/', '=', ';', '\n', '\r'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid desktop application ID",
        ));
    }

    let hierarchy = MimeHierarchy::load(&env.mime_dirs());
    let canonical_mime = hierarchy.canonical(&mime_trimmed);

    // Update the canonical user `~/.config/mimeapps.list`.
    update_mimeapps_file(&env.user_mimeapps_path(), &canonical_mime, id_trimmed)?;

    // If a higher-precedence desktop-specific user override file already exists and mentions this
    // MIME type, keep it synchronized so the newly chosen default takes effect immediately.
    for override_path in env.user_desktop_mimeapps_paths() {
        if let Ok(existing) = fs::read_to_string(&override_path) {
            let parsed = parse_mimeapps_list(&existing);
            if parsed.defaults.contains_key(&canonical_mime)
                || parsed
                    .removed
                    .get(&canonical_mime)
                    .is_some_and(|list| list.iter().any(|entry| entry == id_trimmed))
            {
                update_mimeapps_file(&override_path, &canonical_mime, id_trimmed)?;
            }
        }
    }

    Ok(())
}

fn update_mimeapps_file(path: &Path, mime: &str, desktop_id: &str) -> io::Result<()> {
    use std::io::Write as _;

    let existing = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let updated = edit_mimeapps_text(&existing, mime, desktop_id);
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "mimeapps.list has no parent")
    })?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(updated.as_bytes())?;
    temp.as_file().sync_data()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Losslessly update `[Default Applications]` and scrub conflicting `[Removed Associations]`
/// entries while preserving comments, formatting, and unrelated sections.
#[must_use]
pub fn edit_mimeapps_text(existing: &str, mime: &str, desktop_id: &str) -> String {
    let mut lines: Vec<String> = existing.lines().map(ToOwned::to_owned).collect();
    let mut section = String::new();
    let mut default_section_end: Option<usize> = None;
    let mut updated_default = false;
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 3);

    for line in lines.drain(..) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if section == "Default Applications" && !updated_default {
                out.push(format!("{mime}={desktop_id};"));
                updated_default = true;
            }
            section = trimmed[1..trimmed.len() - 1].trim().to_owned();
            out.push(line);
            if section == "Default Applications" {
                default_section_end = Some(out.len());
            }
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push(line);
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key_lower = key.trim().to_ascii_lowercase();
            if section == "Default Applications" && key_lower == mime {
                if !updated_default {
                    out.push(format!("{mime}={desktop_id};"));
                    updated_default = true;
                }
                continue;
            }
            if section == "Removed Associations" && key_lower == mime {
                let remaining: Vec<&str> = value
                    .split(';')
                    .map(str::trim)
                    .filter(|item| !item.is_empty() && *item != desktop_id)
                    .collect();
                if !remaining.is_empty() {
                    out.push(format!("{mime}={};", remaining.join(";")));
                }
                continue;
            }
        }

        out.push(line);
        if section == "Default Applications" {
            default_section_end = Some(out.len());
        }
    }

    if !updated_default {
        if let Some(insert_at) = default_section_end {
            out.insert(insert_at, format!("{mime}={desktop_id};"));
        } else {
            if !out.is_empty() && !out.last().is_some_and(String::is_empty) {
                out.push(String::new());
            }
            out.push("[Default Applications]".to_owned());
            out.push(format!("{mime}={desktop_id};"));
        }
    }

    let mut result = out.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn parse_mimeapps_list(text: &str) -> MimeAppsFile {
    let mut parsed = MimeAppsFile::default();
    let mut section = "";
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = header.trim();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let mime = key.trim().to_ascii_lowercase();
        if mime.is_empty() {
            continue;
        }
        let ids: Vec<String> = value
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if ids.is_empty() {
            continue;
        }
        let target = match section {
            "Default Applications" => &mut parsed.defaults,
            "Added Associations" => &mut parsed.added,
            "Removed Associations" => &mut parsed.removed,
            _ => continue,
        };
        target.entry(mime).or_default().extend(ids);
    }
    parsed
}

fn load_applications(env: &Environment) -> HashMap<String, DesktopApp> {
    let mut apps = HashMap::new();
    let mut seen_ids = HashSet::new();

    for root in env.application_dirs() {
        if !root.is_dir() {
            continue;
        }
        collect_desktop_dir(&root, &root, 0, env, &mut seen_ids, &mut apps);
    }
    apps
}

fn collect_desktop_dir(
    root: &Path,
    current: &Path,
    depth: usize,
    env: &Environment,
    seen_ids: &mut HashSet<String>,
    apps: &mut HashMap<String, DesktopApp>,
) {
    const MAX_DEPTH: usize = 4;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_dir() {
            collect_desktop_dir(root, &path, depth + 1, env, seen_ids, apps);
            continue;
        }
        let Ok(target_meta) = fs::metadata(&path) else {
            continue;
        };
        if !target_meta.is_file() || path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let Some(id) = desktop_id_from_relative(rel) else {
            continue;
        };
        // Higher-priority XDG directory shadows any lower-priority file with the same ID.
        if !seen_ids.insert(id.clone()) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(app) = parse_desktop_entry(&id, &content, env) {
            apps.insert(id, app);
        }
    }
}

fn desktop_id_from_relative(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in rel.components() {
        let text = component.as_os_str().to_str()?;
        parts.push(text);
    }
    (!parts.is_empty()).then(|| parts.join("-"))
}

fn parse_desktop_entry(id: &str, content: &str, env: &Environment) -> Option<DesktopApp> {
    let mut in_entry = false;
    let mut entry_type: Option<&str> = None;
    let mut exec_present = false;
    let mut try_exec: Option<&str> = None;
    let mut hidden = false;
    let mut no_display = false;
    let mut only_show_in: Vec<String> = Vec::new();
    let mut not_show_in: Vec<String> = Vec::new();
    let mut mime_types: Vec<String> = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut generic_names: HashMap<String, String> = HashMap::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if header == "Desktop Entry" {
                in_entry = true;
            } else if in_entry {
                break;
            }
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Type" => entry_type = Some(value),
            "Exec" => exec_present = !value.is_empty(),
            "TryExec" => try_exec = (!value.is_empty()).then_some(value),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
            "OnlyShowIn" => {
                only_show_in = value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_ascii_lowercase)
                    .collect();
            }
            "NotShowIn" => {
                not_show_in = value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_ascii_lowercase)
                    .collect();
            }
            "MimeType" => {
                mime_types = value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_ascii_lowercase)
                    .collect();
            }
            "Name" => {
                names.insert(String::new(), unescape_desktop_value(value));
            }
            "GenericName" => {
                generic_names.insert(String::new(), unescape_desktop_value(value));
            }
            _ => {
                if let Some(locale) = key.strip_prefix("Name[").and_then(|s| s.strip_suffix(']')) {
                    names.insert(locale.to_owned(), unescape_desktop_value(value));
                } else if let Some(locale) = key
                    .strip_prefix("GenericName[")
                    .and_then(|s| s.strip_suffix(']'))
                {
                    generic_names.insert(locale.to_owned(), unescape_desktop_value(value));
                }
            }
        }
    }

    if entry_type != Some("Application") || hidden || !exec_present {
        return None;
    }
    if let Some(program) = try_exec
        && !is_executable_available(program, &env.path_dirs)
    {
        return None;
    }
    if !visible_in_desktop(&only_show_in, &not_show_in, &env.desktops) {
        return None;
    }

    let name = pick_localized(&names, &env.locales)?;
    let generic_name = pick_localized(&generic_names, &env.locales);

    Some(DesktopApp {
        id: id.to_owned(),
        name,
        generic_name,
        mime_types,
        no_display,
    })
}

fn pick_localized(values: &HashMap<String, String>, locales: &[String]) -> Option<String> {
    for locale in locales {
        if let Some(value) = values.get(locale)
            && !value.is_empty()
        {
            return Some(value.clone());
        }
    }
    values.get("").filter(|v| !v.is_empty()).cloned()
}

fn visible_in_desktop(
    only_show_in: &[String],
    not_show_in: &[String],
    desktops: &[String],
) -> bool {
    let active: Vec<String> = desktops.iter().map(|d| d.to_ascii_lowercase()).collect();
    if !only_show_in.is_empty() && !active.iter().any(|d| only_show_in.contains(d)) {
        return false;
    }
    if !not_show_in.is_empty() && active.iter().any(|d| not_show_in.contains(d)) {
        return false;
    }
    true
}

fn is_executable_available(program: &str, path_dirs: &[PathBuf]) -> bool {
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        return is_executable_file(candidate);
    }
    path_dirs
        .iter()
        .any(|dir| is_executable_file(&dir.join(candidate)))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn unescape_desktop_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
        fs::write(path, content).expect("write file");
    }

    #[test]
    fn resolves_xdg_precedence_hidden_shadowing_removed_associations_and_subclasses() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_home = root.path().join("config");
        let config_sys = root.path().join("etc/xdg");
        let data_home = root.path().join("data_home");
        let data_sys = root.path().join("usr/share");

        // System apps
        write_file(
            &data_sys.join("applications/editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Plain Editor\nName[zh_CN]=纯文本编辑器\nExec=editor %U\nMimeType=text/plain;\n",
        );
        write_file(
            &data_sys.join("applications/code.desktop"),
            "[Desktop Entry]\nType=Application\nName=Code Studio\nExec=code %U\nMimeType=text/x-rust;text/plain;\n",
        );
        write_file(
            &data_sys.join("applications/legacy.desktop"),
            "[Desktop Entry]\nType=Application\nName=Legacy Editor\nExec=legacy %U\nMimeType=text/plain;\n",
        );
        // Higher-priority Hidden=true shadows legacy.desktop
        write_file(
            &data_home.join("applications/legacy.desktop"),
            "[Desktop Entry]\nType=Application\nName=Legacy Editor\nExec=legacy %U\nHidden=true\n",
        );
        // Subdirectory desktop ID: org/viewer.desktop -> org-viewer.desktop
        write_file(
            &data_sys.join("applications/org/viewer.desktop"),
            "[Desktop Entry]\nType=Application\nName=Subdir Viewer\nExec=viewer %U\nMimeType=text/plain;\n",
        );

        // Shared MIME-Info subclasses & aliases
        write_file(
            &data_sys.join("mime/subclasses"),
            "text/x-rust text/plain\n",
        );
        write_file(&data_sys.join("mime/aliases"), "text/rust text/x-rust\n");

        // System default sets editor.desktop for text/plain, user removes org-viewer.desktop
        write_file(
            &config_sys.join("mimeapps.list"),
            "[Default Applications]\ntext/plain=legacy.desktop;editor.desktop;\n",
        );
        write_file(
            &config_home.join("mimeapps.list"),
            "[Removed Associations]\ntext/plain=org-viewer.desktop;\n",
        );

        let env = Environment {
            config_home: config_home.clone(),
            config_dirs: vec![config_sys],
            data_home,
            data_dirs: vec![data_sys],
            desktops: vec!["Niri".to_owned()],
            locales: vec!["zh_CN".to_owned(), "zh".to_owned()],
            path_dirs: Vec::new(),
        };

        let reg = Registry::load(&env);
        let text_assoc = reg.resolve("text/plain");
        // legacy.desktop was shadowed by Hidden=true, so editor.desktop wins with zh_CN name!
        assert_eq!(
            text_assoc.default_app.as_ref().map(|a| a.id.as_str()),
            Some("editor.desktop")
        );
        assert_eq!(
            text_assoc.default_app.as_ref().map(|a| a.name.as_str()),
            Some("纯文本编辑器")
        );
        // org-viewer.desktop was excluded by [Removed Associations]
        assert!(
            !text_assoc
                .recommended
                .iter()
                .any(|a| a.id == "org-viewer.desktop")
        );

        // Alias + subclass inheritance: text/rust -> text/x-rust -> text/plain
        let rust_assoc = reg.resolve("text/rust");
        assert_eq!(rust_assoc.mime_type, "text/x-rust");
        assert_eq!(
            rust_assoc.default_app.as_ref().map(|a| a.id.as_str()),
            Some("editor.desktop")
        );
        assert!(
            rust_assoc
                .recommended
                .iter()
                .any(|a| a.id == "code.desktop")
        );

        // Persist code.desktop as new default for text/rust (canonicalizes to text/x-rust)
        set_default_in(&env, "text/rust", "code.desktop").expect("set default");
        let reloaded = Registry::load(&env);
        assert_eq!(
            reloaded
                .resolve("text/x-rust")
                .default_app
                .as_ref()
                .map(|a| a.id.as_str()),
            Some("code.desktop")
        );
    }

    #[test]
    fn edit_mimeapps_text_preserves_comments_and_scrubs_removed_associations() {
        let original = "\
# User MIME preferences
[Default Applications]
image/png=viewer.desktop;
text/plain=old.desktop;

[Added Associations]
text/plain=extra.desktop;

[Removed Associations]
text/plain=new.desktop;other.desktop;
";
        let updated = edit_mimeapps_text(original, "text/plain", "new.desktop");
        assert!(updated.contains("# User MIME preferences"));
        assert!(updated.contains("image/png=viewer.desktop;"));
        assert!(updated.contains("text/plain=new.desktop;"));
        assert!(updated.contains("[Added Associations]\ntext/plain=extra.desktop;"));
        assert!(updated.contains("[Removed Associations]\ntext/plain=other.desktop;"));
        assert!(!updated.contains("text/plain=old.desktop;"));
    }
}
