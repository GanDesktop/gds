//! Fontconfig resolution with fingerprint-validated disk and memory caches.

use std::{
    env, fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use super::{SYSTEM_FAMILY, SYSTEM_MONO_FAMILY};

const SCHEMA_VERSION: u32 = 1;
const CACHE_SUBPATH: &str = "gds/typography.json";
const ENV_KEYS: [&str; 6] = [
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "FONTCONFIG_FILE",
    "FONTCONFIG_PATH",
    "FONTCONFIG_SYSROOT",
];

pub(super) struct SystemFamilies {
    pub sans_serif: String,
    pub monospace: String,
}

pub(super) struct CustomMatch {
    pub family: String,
    pub confirmed_monospace: bool,
}

pub(super) struct QueriedFamilies {
    pub sans_serif: String,
    pub monospace: String,
    pub watched: Vec<PathBuf>,
}

struct QueriedInstalled {
    families: Vec<String>,
    watched: Vec<PathBuf>,
}

struct InstalledCache {
    env: Vec<(String, Option<String>)>,
    watched: Vec<PathStamp>,
    families: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct CacheEntry {
    version: u32,
    sans_serif: String,
    monospace: String,
    env: Vec<(String, Option<String>)>,
    watched: Vec<PathStamp>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct PathStamp {
    path: PathBuf,
    state: PathState,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PathState {
    Missing,
    Present {
        is_dir: bool,
        link_target: Option<PathBuf>,
        mtime_sec: i64,
        mtime_nsec: u32,
        size: u64,
        ino: u64,
    },
}

pub(super) fn system_families() -> &'static SystemFamilies {
    static FAMILIES: OnceLock<SystemFamilies> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let cache_path = BaseDirs::new().map(|dirs| dirs.cache_dir().join(CACHE_SUBPATH));
        load_or_query_with(cache_path.as_deref(), current_env, query_system_families)
    })
}

pub(super) fn installed_families() -> Vec<String> {
    static INSTALLED: Mutex<Option<InstalledCache>> = Mutex::new(None);
    cached_installed_families_with(&INSTALLED, current_env, query_installed_families)
}

fn cached_installed_families_with(
    slot: &Mutex<Option<InstalledCache>>,
    read_env: impl FnOnce() -> Vec<(String, Option<String>)>,
    query: impl FnOnce() -> QueriedInstalled,
) -> Vec<String> {
    let env_snapshot = read_env();
    {
        let guard = slot.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(cached) = guard.as_ref()
            && !cached.watched.is_empty()
            && cached.env == env_snapshot
            && stamps_match(&cached.watched)
        {
            return cached.families.clone();
        }
    }

    let queried = query();
    if !queried.watched.is_empty() {
        let watched = stamp_paths(&queried.watched);
        let mut guard = slot.lock().unwrap_or_else(|poison| poison.into_inner());
        *guard = Some(InstalledCache {
            env: env_snapshot,
            watched,
            families: queried.families.clone(),
        });
    }
    queried.families
}

pub(super) fn resolve_custom_monospace(requested: &str) -> Option<CustomMatch> {
    fc::resolve_custom_monospace(requested)
}

pub(super) fn load_or_query_with(
    cache_path: Option<&Path>,
    read_env: impl FnOnce() -> Vec<(String, Option<String>)>,
    query: impl FnOnce() -> QueriedFamilies,
) -> SystemFamilies {
    let env_snapshot = read_env();
    let mut existing_bytes = None;

    if let Some(path) = cache_path {
        let file = crate::persistence::File::new(path.to_path_buf());
        match file.read() {
            Ok(Some(bytes)) => {
                if let Some(cached) = valid_cached_families(&bytes, &env_snapshot) {
                    return cached;
                }
                existing_bytes = Some(bytes);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(?path, %error, "cannot read typography cache");
            }
        }
    }

    let queried = query();
    if let Some(path) = cache_path
        && !queried.watched.is_empty()
    {
        persist_cache(path, existing_bytes.as_deref(), &env_snapshot, &queried);
    }

    SystemFamilies {
        sans_serif: queried.sans_serif,
        monospace: queried.monospace,
    }
}

fn valid_cached_families(
    bytes: &[u8],
    env_snapshot: &[(String, Option<String>)],
) -> Option<SystemFamilies> {
    let entry: CacheEntry = serde_json::from_slice(bytes).ok()?;
    if entry.version != SCHEMA_VERSION
        || entry.sans_serif.is_empty()
        || entry.monospace.is_empty()
        || entry.watched.is_empty()
        || entry.env != env_snapshot
    {
        return None;
    }
    if stamps_match(&entry.watched) {
        Some(SystemFamilies {
            sans_serif: entry.sans_serif,
            monospace: entry.monospace,
        })
    } else {
        None
    }
}

fn persist_cache(
    path: &Path,
    expected: Option<&[u8]>,
    env_snapshot: &[(String, Option<String>)],
    queried: &QueriedFamilies,
) {
    let watched = stamp_paths(&queried.watched);
    let entry = CacheEntry {
        version: SCHEMA_VERSION,
        sans_serif: queried.sans_serif.clone(),
        monospace: queried.monospace.clone(),
        env: env_snapshot.to_vec(),
        watched,
    };
    let Ok(encoded) = serde_json::to_vec(&entry) else {
        return;
    };
    let file = crate::persistence::File::new(path.to_path_buf());
    if let Err(error) = file.write(expected, &encoded) {
        tracing::debug!(?path, %error, "cannot write typography cache");
    }
}

fn current_env() -> Vec<(String, Option<String>)> {
    ENV_KEYS
        .iter()
        .map(|key| ((*key).to_owned(), env::var(key).ok()))
        .collect()
}

fn stamps_match(watched: &[PathStamp]) -> bool {
    watched
        .iter()
        .all(|item| stamp_path(&item.path) == item.state)
}

fn stamp_paths(paths: &[PathBuf]) -> Vec<PathStamp> {
    paths
        .iter()
        .map(|watched_path| PathStamp {
            path: watched_path.clone(),
            state: stamp_path(watched_path),
        })
        .collect()
}

fn stamp_path(path: &Path) -> PathState {
    let Ok(link_meta) = fs::symlink_metadata(path) else {
        return PathState::Missing;
    };
    let link_target = link_meta
        .file_type()
        .is_symlink()
        .then(|| fs::read_link(path).ok())
        .flatten();
    let target_meta = fs::metadata(path).unwrap_or(link_meta);
    let mtime_nsec = u32::try_from(target_meta.mtime_nsec()).unwrap_or(0);
    PathState::Present {
        is_dir: target_meta.is_dir(),
        link_target,
        mtime_sec: target_meta.mtime(),
        mtime_nsec,
        size: target_meta.len(),
        ino: target_meta.ino(),
    }
}

fn query_system_families() -> QueriedFamilies {
    let (sans_serif, monospace, mut watched) = fc::resolve_defaults();
    append_standard_paths(&mut watched);
    watched.sort();
    watched.dedup();
    QueriedFamilies {
        sans_serif: sans_serif.unwrap_or_else(|| SYSTEM_FAMILY.to_owned()),
        monospace: monospace.unwrap_or_else(|| SYSTEM_MONO_FAMILY.to_owned()),
        watched,
    }
}

fn query_installed_families() -> QueriedInstalled {
    let (families, mut watched) = fc::installed_families();
    if !families.is_empty() || !watched.is_empty() {
        append_standard_paths(&mut watched);
        watched.sort();
        watched.dedup();
    }
    QueriedInstalled { families, watched }
}

fn append_standard_paths(paths: &mut Vec<PathBuf>) {
    const SYSTEM_PATHS: [&str; 10] = [
        "/etc/fonts",
        "/etc/fonts/fonts.conf",
        "/etc/fonts/local.conf",
        "/etc/fonts/conf.d",
        "/usr/local/etc/fonts",
        "/usr/local/etc/fonts/fonts.conf",
        "/usr/local/etc/fonts/local.conf",
        "/usr/local/etc/fonts/conf.d",
        "/usr/share/fonts",
        "/usr/local/share/fonts",
    ];
    for path in SYSTEM_PATHS {
        paths.push(PathBuf::from(path));
    }
    append_dir_entries(Path::new("/etc/fonts/conf.d"), paths);
    append_dir_entries(Path::new("/usr/local/etc/fonts/conf.d"), paths);

    if let Some(dirs) = BaseDirs::new() {
        let user_fc = dirs.config_dir().join("fontconfig");
        let user_conf_d = user_fc.join("conf.d");
        paths.push(user_fc.join("fonts.conf"));
        paths.push(user_conf_d.clone());
        paths.push(user_fc);
        paths.push(dirs.data_dir().join("fonts"));
        paths.push(dirs.home_dir().join(".fonts.conf"));
        paths.push(dirs.home_dir().join(".fonts"));
        append_dir_entries(&user_conf_d, paths);
    }
}

fn append_dir_entries(dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        paths.push(entry.path());
    }
}

mod fc {
    use std::{
        ffi::{CStr, CString, OsStr},
        os::unix::ffi::OsStrExt as _,
        path::PathBuf,
        ptr,
        sync::Mutex,
    };

    use fontconfig_sys::{
        FcBool, FcConfig, FcConfigGetConfigFiles, FcConfigGetCurrent, FcConfigGetFontDirs,
        FcConfigSubstitute, FcDefaultSubstitute, FcFontList, FcFontMatch, FcFontSet,
        FcFontSetDestroy, FcInit, FcMatchPattern, FcObjectSet, FcObjectSetAdd, FcObjectSetCreate,
        FcObjectSetDestroy, FcPattern, FcPatternAddString, FcPatternCreate, FcPatternDestroy,
        FcPatternGetInteger, FcPatternGetString, FcResultMatch, FcStrList, FcStrListDone,
        FcStrListNext,
        constants::{FC_CHARCELL, FC_DUAL, FC_FAMILY, FC_FAMILYLANG, FC_MONO, FC_SPACING},
    };

    use super::CustomMatch;

    static FC_LOCK: Mutex<()> = Mutex::new(());

    struct Pattern(*mut FcPattern);

    impl Pattern {
        fn new() -> Option<Self> {
            // SAFETY: `FcPatternCreate` takes no arguments and returns an owned pattern or null.
            let raw = unsafe { FcPatternCreate() };
            (!raw.is_null()).then_some(Self(raw))
        }

        fn from_raw(raw: *mut FcPattern) -> Option<Self> {
            (!raw.is_null()).then_some(Self(raw))
        }

        fn add_string(&mut self, object: &CStr, value: &str) -> bool {
            let Ok(c_value) = CString::new(value) else {
                return false;
            };
            // SAFETY: `self.0`, `object`, and `c_value` are valid non-null pointers for the call.
            unsafe { FcPatternAddString(self.0, object.as_ptr(), c_value.as_ptr().cast()) != 0 }
        }
    }

    impl Drop for Pattern {
        fn drop(&mut self) {
            // SAFETY: `self.0` was uniquely owned by this guard.
            unsafe { FcPatternDestroy(self.0) };
        }
    }

    struct ObjectSet(*mut FcObjectSet);

    impl ObjectSet {
        fn new() -> Option<Self> {
            // SAFETY: `FcObjectSetCreate` allocates an empty set or returns null.
            let raw = unsafe { FcObjectSetCreate() };
            (!raw.is_null()).then_some(Self(raw))
        }

        fn add(&mut self, object: &CStr) -> bool {
            // SAFETY: `self.0` and `object` are valid non-null pointers.
            unsafe { FcObjectSetAdd(self.0, object.as_ptr()) != 0 }
        }
    }

    impl Drop for ObjectSet {
        fn drop(&mut self) {
            // SAFETY: `self.0` was uniquely owned by this guard.
            unsafe { FcObjectSetDestroy(self.0) };
        }
    }

    struct FontSet(*mut FcFontSet);

    impl FontSet {
        fn from_raw(raw: *mut FcFontSet) -> Option<Self> {
            (!raw.is_null()).then_some(Self(raw))
        }

        fn patterns(&self) -> &[*mut FcPattern] {
            // SAFETY: `self.0` is non-null and initialized by Fontconfig.
            let set = unsafe { &*self.0 };
            if set.nfont <= 0 || set.fonts.is_null() {
                &[]
            } else {
                // SAFETY: Fontconfig guarantees `fonts` points to `nfont` valid pattern pointers.
                unsafe { std::slice::from_raw_parts(set.fonts, set.nfont as usize) }
            }
        }
    }

    impl Drop for FontSet {
        fn drop(&mut self) {
            // SAFETY: `self.0` was uniquely owned by this guard.
            unsafe { FcFontSetDestroy(self.0) };
        }
    }

    struct StrList(*mut FcStrList);

    impl StrList {
        fn from_raw(raw: *mut FcStrList) -> Option<Self> {
            (!raw.is_null()).then_some(Self(raw))
        }

        fn collect_paths(self, out: &mut Vec<PathBuf>) {
            loop {
                // SAFETY: `self.0` is a live iterator owned by this guard.
                let next = unsafe { FcStrListNext(self.0) };
                if next.is_null() {
                    break;
                }
                // SAFETY: Fontconfig returns a NUL-terminated path string valid until `FcStrListDone`.
                let bytes = unsafe { CStr::from_ptr(next.cast()) }.to_bytes();
                if !bytes.is_empty() {
                    out.push(PathBuf::from(OsStr::from_bytes(bytes)));
                }
            }
        }
    }

    impl Drop for StrList {
        fn drop(&mut self) {
            // SAFETY: `self.0` was returned by Fontconfig and not yet closed.
            unsafe { FcStrListDone(self.0) };
        }
    }

    pub(super) fn installed_families() -> (Vec<String>, Vec<PathBuf>) {
        let _guard = FC_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(config) = current_config() else {
            return (Vec::new(), Vec::new());
        };
        let Some(pattern) = Pattern::new() else {
            return (Vec::new(), Vec::new());
        };
        let Some(mut objects) = ObjectSet::new() else {
            return (Vec::new(), Vec::new());
        };
        if !objects.add(FC_FAMILY) || !objects.add(FC_FAMILYLANG) {
            return (Vec::new(), Vec::new());
        }
        // SAFETY: config, pattern, and object set remain alive for the call.
        let Some(fonts) = FontSet::from_raw(unsafe { FcFontList(config, pattern.0, objects.0) })
        else {
            return (Vec::new(), Vec::new());
        };
        let mut names: Vec<_> = fonts
            .patterns()
            .iter()
            .filter_map(|face| select_english_or_first_family(*face))
            .collect();
        names.sort();
        names.dedup();
        let watched = collect_watched_paths(config);
        (names, watched)
    }

    pub(super) fn resolve_defaults() -> (Option<String>, Option<String>, Vec<PathBuf>) {
        let _guard = FC_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(config) = current_config() else {
            return (None, None, Vec::new());
        };
        let sans_serif = match_alias(config, "sans-serif");
        let monospace = match_alias(config, "monospace");
        let watched = collect_watched_paths(config);
        (sans_serif, monospace, watched)
    }

    pub(super) fn resolve_custom_monospace(requested: &str) -> Option<CustomMatch> {
        let _guard = FC_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let config = current_config()?;
        let mut pat = Pattern::new()?;
        if !pat.add_string(FC_FAMILY, requested) {
            return None;
        }
        let mut objects = ObjectSet::new()?;
        if !objects.add(FC_FAMILY) || !objects.add(FC_FAMILYLANG) || !objects.add(FC_SPACING) {
            return None;
        }
        // SAFETY: `config`, `pat.0`, and `objects.0` are valid non-null Fontconfig handles.
        let fonts = FontSet::from_raw(unsafe { FcFontList(config, pat.0, objects.0) })?;
        let faces = fonts.patterns();
        let family = faces
            .iter()
            .find_map(|face| select_english_or_first_family(*face))?;
        let confirmed_monospace = faces.iter().any(|face| {
            let mut spacing = 0;
            // SAFETY: `face` is non-null inside `FontSet`.
            let status =
                unsafe { FcPatternGetInteger(*face, FC_SPACING.as_ptr(), 0, &mut spacing) };
            status == FcResultMatch && matches!(spacing, FC_MONO | FC_DUAL | FC_CHARCELL)
        });
        Some(CustomMatch {
            family,
            confirmed_monospace,
        })
    }

    fn current_config() -> Option<*mut FcConfig> {
        // SAFETY: `FcConfigGetCurrent` initializes Fontconfig lazily if needed and returns a
        // process-owned handle (never destroyed by the caller).
        let config = unsafe { FcConfigGetCurrent() };
        if !config.is_null() {
            return Some(config);
        }
        // SAFETY: fallback initialization when no default configuration is loaded yet.
        let ok: FcBool = unsafe { FcInit() };
        if ok == 0 {
            return None;
        }
        // SAFETY: `FcInit` succeeded above.
        let config = unsafe { FcConfigGetCurrent() };
        (!config.is_null()).then_some(config)
    }

    fn match_alias(config: *mut FcConfig, alias: &str) -> Option<String> {
        let mut pat = Pattern::new()?;
        if !pat.add_string(FC_FAMILY, alias) {
            return None;
        }
        // Pass the explicit `config` instead of NULL so Fontconfig reuses the current
        // configuration instead of re-initializing a duplicate config tree.
        // SAFETY: `config` and `pat.0` are non-null handles.
        if unsafe { FcConfigSubstitute(config, pat.0, FcMatchPattern) } == 0 {
            return None;
        }
        // SAFETY: `pat.0` is non-null.
        unsafe { FcDefaultSubstitute(pat.0) };
        let mut result = FcResultMatch;
        // SAFETY: `config` and `pat.0` are non-null; `result` receives the match outcome.
        let matched = Pattern::from_raw(unsafe { FcFontMatch(config, pat.0, &mut result) })?;
        if result != FcResultMatch {
            return None;
        }
        select_english_or_first_family(matched.0)
    }

    fn collect_watched_paths(config: *mut FcConfig) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        // SAFETY: `config` is a valid non-null handle.
        if let Some(files) = StrList::from_raw(unsafe { FcConfigGetConfigFiles(config) }) {
            files.collect_paths(&mut paths);
        }
        // SAFETY: `config` is a valid non-null handle.
        if let Some(dirs) = StrList::from_raw(unsafe { FcConfigGetFontDirs(config) }) {
            dirs.collect_paths(&mut paths);
        }
        paths
    }

    fn select_english_or_first_family(pattern: *mut FcPattern) -> Option<String> {
        if pattern.is_null() {
            return None;
        }
        let mut first: Option<String> = None;
        let mut index = 0;
        while let Some(family) = pattern_string(pattern, FC_FAMILY, index) {
            if first.is_none() {
                first = Some(family.clone());
            }
            if pattern_string(pattern, FC_FAMILYLANG, index)
                .as_deref()
                .is_some_and(is_english_lang)
            {
                return Some(family);
            }
            index += 1;
        }
        first
    }

    fn pattern_string(pattern: *mut FcPattern, object: &CStr, index: i32) -> Option<String> {
        let mut value = ptr::null_mut();
        // SAFETY: `pattern` and `object` are valid non-null pointers.
        let status = unsafe { FcPatternGetString(pattern, object.as_ptr(), index, &mut value) };
        if status != FcResultMatch || value.is_null() {
            return None;
        }
        // SAFETY: Fontconfig returned a valid NUL-terminated UTF-8/ASCII string on `FcResultMatch`.
        let text = unsafe { CStr::from_ptr(value.cast()) }
            .to_str()
            .ok()?
            .trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    fn is_english_lang(lang: &str) -> bool {
        lang.eq_ignore_ascii_case("en")
            || lang
                .get(..3)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("en-"))
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs, os::unix::fs::symlink, sync::Mutex};

    use super::{QueriedInstalled, cached_installed_families_with, installed_families};

    fn sample_env(fc_path: &str) -> Vec<(String, Option<String>)> {
        vec![("FONTCONFIG_PATH".to_owned(), Some(fc_path.to_owned()))]
    }

    #[test]
    fn installed_families_memory_cache_hits_and_invalidates_on_stamp_or_env_change() {
        let temp = tempfile::tempdir().expect("temp dir");
        let watched_conf = temp.path().join("fonts.conf");
        let target_a = temp.path().join("10-a.conf");
        let target_b = temp.path().join("10-b.conf");
        let watched_link = temp.path().join("10-active.conf");
        let user_font_dir = temp.path().join("user-fonts");

        fs::write(&watched_conf, b"<fontconfig/>").expect("write watched conf");
        fs::write(&target_a, b"a").expect("write target a");
        fs::write(&target_b, b"b").expect("write target b");
        symlink(&target_a, &watched_link).expect("create watched symlink");

        let watched = vec![
            watched_conf.clone(),
            watched_link.clone(),
            user_font_dir.clone(),
        ];
        let cache = Mutex::new(None);
        let queries = Cell::new(0_usize);

        let cold = cached_installed_families_with(
            &cache,
            || sample_env("/etc/fonts"),
            || {
                queries.set(queries.get() + 1);
                QueriedInstalled {
                    families: vec!["Inter".to_owned(), "Sarasa UI SC".to_owned()],
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 1);
        assert_eq!(cold, vec!["Inter".to_owned(), "Sarasa UI SC".to_owned()]);

        let hit = cached_installed_families_with(
            &cache,
            || sample_env("/etc/fonts"),
            || {
                queries.set(queries.get() + 1);
                QueriedInstalled {
                    families: vec!["Unexpected".to_owned()],
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 1);
        assert_eq!(hit, cold);

        let env_miss = cached_installed_families_with(
            &cache,
            || sample_env("/custom/fonts"),
            || {
                queries.set(queries.get() + 1);
                QueriedInstalled {
                    families: vec!["Noto Sans".to_owned()],
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 2);
        assert_eq!(env_miss, vec!["Noto Sans".to_owned()]);

        fs::write(&watched_conf, b"<fontconfig><!-- changed --></fontconfig>")
            .expect("modify watched conf");
        let conf_miss = cached_installed_families_with(
            &cache,
            || sample_env("/custom/fonts"),
            || {
                queries.set(queries.get() + 1);
                QueriedInstalled {
                    families: vec!["IBM Plex Sans".to_owned(), "Noto Sans".to_owned()],
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 3);
        assert_eq!(
            conf_miss,
            vec!["IBM Plex Sans".to_owned(), "Noto Sans".to_owned()]
        );

        fs::create_dir(&user_font_dir).expect("create watched font dir");
        let dir_miss = cached_installed_families_with(
            &cache,
            || sample_env("/custom/fonts"),
            || {
                queries.set(queries.get() + 1);
                QueriedInstalled {
                    families: vec!["JetBrains Mono".to_owned()],
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 4);
        assert_eq!(dir_miss, vec!["JetBrains Mono".to_owned()]);

        fs::remove_file(&watched_link).expect("remove old symlink");
        symlink(&target_b, &watched_link).expect("retarget symlink");
        let link_miss = cached_installed_families_with(
            &cache,
            || sample_env("/custom/fonts"),
            || {
                queries.set(queries.get() + 1);
                QueriedInstalled {
                    families: vec!["Fira Code".to_owned()],
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 5);
        assert_eq!(link_miss, vec!["Fira Code".to_owned()]);
    }

    #[test]
    fn live_installed_families_cache_returns_identical_sorted_families() {
        let first = installed_families();
        let second = installed_families();
        assert_eq!(first, second);
        let mut sorted = first.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(first, sorted);
    }
}
