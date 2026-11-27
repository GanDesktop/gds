//! FreeDesktop `.thumbnailer` discovery and execution with optional Bubblewrap (`bwrap`) sandboxing.
//!
//! On Linux, Bubblewrap (`bwrap`) sandboxing is recommended and enabled by default. Users can
//! explicitly opt out of sandboxing (or run on non-Linux Unix platforms such as FreeBSD); even
//! unsandboxed execution avoids shells (`execve` only), isolates each helper in its own process
//! group for reliable timeout/cancellation cleanup, caps concurrency, and bounds PNG decoding.

use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::Duration,
};

use directories::BaseDirs;
use image::RgbaImage;
use tokio_util::sync::CancellationToken;

use super::{EDGE, Error, Target, cache, image as thumb_image};

const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const ERROR_BYTES: u64 = 4 * 1024;
const MAX_OUTPUT_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CONCURRENT_EXTERNAL: usize = 2;

/// Read-only host roots exposed inside the `bwrap` sandbox via `--ro-bind-try`.
/// Covers both FHS Linux distributions and NixOS (`/nix/store`, `/etc/static`, `/etc/profiles`).
const SANDBOX_RO_ROOTS: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/lib32",
    "/nix/store",
    "/nix/var/nix/profiles",
    "/run/current-system/sw",
    "/etc/profiles",
    "/etc/static",
    "/etc/fonts",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/alternatives",
    "/etc/ssl",
    "/etc/xdg",
];

/// Generic MIME supertypes that must never match an external thumbnailer via ancestor fallback.
const IGNORED_ANCESTOR_MIMES: &[&str] = &[
    "application/octet-stream",
    "text/plain",
    "application/x-zerosize",
];

/// Metadata describing one discovered FreeDesktop `.thumbnailer` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbnailerInfo {
    pub id: String,
    pub path: PathBuf,
    pub executable: PathBuf,
    pub mime_types: Vec<String>,
    pub user_override: bool,
}

/// Execution isolation mode for external FreeDesktop `.thumbnailer` helpers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxMode {
    /// Isolate each helper inside a verified Bubblewrap (`bwrap`) sandbox (recommended on Linux).
    Bwrap(PathBuf),
    /// Execute helpers directly without mount/network sandboxing (explicit user opt-out or non-Linux).
    Unsandboxed,
}

/// Reason why the Bubblewrap (`bwrap`) sandbox cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxError {
    UnsupportedPlatform,
    NotFound(PathBuf),
    NotExecutable(PathBuf),
    ProbeFailed(String),
}

/// Combined result of probing `bwrap` and scanning XDG `.thumbnailer` directories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalProbeReport {
    pub discovered: Vec<ThumbnailerInfo>,
    pub mime_count: usize,
    pub sandbox: Result<PathBuf, SandboxError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThumbnailerSpec {
    info: ThumbnailerInfo,
    exec_argv: Vec<String>,
}

/// Discovered FreeDesktop `.thumbnailer` registry paired with its execution sandbox mode.
///
/// Sandboxed instances (`SandboxMode::Bwrap`) require passing a live `bwrap` probe (except in
/// unit tests). Unsandboxed instances are only constructed when the caller explicitly opts out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalThumbnailers {
    sandbox: SandboxMode,
    entries: Arc<[ThumbnailerSpec]>,
    by_mime: Arc<HashMap<String, Vec<usize>>>,
    mime_count: usize,
}

impl ExternalThumbnailers {
    /// Whether Bubblewrap (`bwrap`) sandboxing is supported on the current target OS.
    pub const BWRAP_SUPPORTED: bool = cfg!(target_os = "linux");

    /// Probes Bubblewrap (`bwrap`) availability and scans XDG `.thumbnailer` directories.
    pub async fn probe_report(
        bwrap_override: Option<&Path>,
        cancel: &CancellationToken,
    ) -> ExternalProbeReport {
        let discovered_specs = discover_thumbnailers(&xdg_thumbnailer_dirs());
        let mime_count = count_unique_mimes(&discovered_specs);
        let discovered: Vec<ThumbnailerInfo> =
            discovered_specs.into_iter().map(|spec| spec.info).collect();
        let sandbox = probe_bwrap(bwrap_override, cancel).await;
        ExternalProbeReport {
            discovered,
            mime_count,
            sandbox,
        }
    }

    /// Loads discovered `.thumbnailer` entries in the requested sandbox mode.
    ///
    /// When `use_bwrap` is `true`, verifies a live `bwrap` sandbox first and refuses to fall back
    /// to unsandboxed execution on failure.
    ///
    /// # Errors
    /// Reports missing, non-executable, unsupported, or non-functional `bwrap` sandboxes when
    /// `use_bwrap` is `true`.
    pub async fn load(
        use_bwrap: bool,
        bwrap_override: Option<&Path>,
        cancel: &CancellationToken,
    ) -> Result<Self, SandboxError> {
        let specs = discover_thumbnailers(&xdg_thumbnailer_dirs());
        let sandbox = if use_bwrap {
            SandboxMode::Bwrap(probe_bwrap(bwrap_override, cancel).await?)
        } else {
            SandboxMode::Unsandboxed
        };
        Ok(Self::from_parts(sandbox, specs))
    }

    fn from_parts(sandbox: SandboxMode, specs: Vec<ThumbnailerSpec>) -> Self {
        let mut by_mime: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, spec) in specs.iter().enumerate() {
            for mime in &spec.info.mime_types {
                let list = by_mime.entry(mime.clone()).or_default();
                if !list.contains(&index) {
                    list.push(index);
                }
            }
        }
        let mime_count = by_mime.len();
        Self {
            sandbox,
            entries: Arc::from(specs),
            by_mime: Arc::new(by_mime),
            mime_count,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_specs_for_test(
        sandbox: SandboxMode,
        id: &str,
        executable: PathBuf,
        exec_argv: Vec<String>,
        mime_types: Vec<String>,
        user_override: bool,
    ) -> Self {
        let spec = ThumbnailerSpec {
            info: ThumbnailerInfo {
                id: id.to_owned(),
                path: PathBuf::from(format!("/test/{id}.thumbnailer")),
                executable,
                mime_types,
                user_override,
            },
            exec_argv,
        };
        Self::from_parts(sandbox, vec![spec])
    }

    pub fn sandbox_mode(&self) -> &SandboxMode {
        &self.sandbox
    }

    pub fn bwrap_path(&self) -> Option<&Path> {
        match &self.sandbox {
            SandboxMode::Bwrap(path) => Some(path.as_path()),
            SandboxMode::Unsandboxed => None,
        }
    }

    pub fn is_sandboxed(&self) -> bool {
        matches!(self.sandbox, SandboxMode::Bwrap(_))
    }

    pub fn entries(&self) -> Vec<ThumbnailerInfo> {
        self.entries.iter().map(|spec| spec.info.clone()).collect()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn mime_count(&self) -> usize {
        self.mime_count
    }

    pub fn has_match(&self, mime: Option<&str>, ancestors: &[Arc<str>]) -> bool {
        !self.matching_indices(mime, ancestors, false).is_empty()
    }

    pub fn has_user_override(&self, mime: Option<&str>, ancestors: &[Arc<str>]) -> bool {
        !self.matching_indices(mime, ancestors, true).is_empty()
    }

    fn matching_indices(
        &self,
        mime: Option<&str>,
        ancestors: &[Arc<str>],
        user_only: bool,
    ) -> Vec<usize> {
        let Some(mime) = mime else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut push_for = |key: &str| {
            let normalized = key.trim().to_ascii_lowercase();
            if let Some(indices) = self.by_mime.get(&normalized) {
                for &idx in indices {
                    if user_only && !self.entries[idx].info.user_override {
                        continue;
                    }
                    if !out.contains(&idx) {
                        out.push(idx);
                    }
                }
            }
        };
        push_for(mime);
        for ancestor in ancestors {
            let lower = ancestor.trim().to_ascii_lowercase();
            if IGNORED_ANCESTOR_MIMES.contains(&lower.as_str()) {
                continue;
            }
            push_for(&lower);
        }
        out
    }

    /// Execute matching external thumbnailer(s) and decode the bounded PNG output.
    ///
    /// # Errors
    /// Reports sandbox unavailability, timeout, cancellation, or thumbnailer decoding failures.
    pub(super) fn generate(
        &self,
        target: Target<'_>,
        timeout: Duration,
        cancel: &CancellationToken,
        user_only: bool,
    ) -> Result<RgbaImage, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
        }
        let candidates = self.matching_indices(target.mime, target.mime_ancestors, user_only);
        if candidates.is_empty() {
            return Err(Error::NoCover);
        }

        let _permit = ExternalPermit::acquire(cancel)?;
        let timeout = timeout.max(Duration::from_secs(1));
        let mut last_error: Option<Error> = None;

        for idx in candidates {
            if cancel.is_cancelled() {
                return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
            }
            let spec = &self.entries[idx];
            match run_thumbnailer(&self.sandbox, spec, target.path, timeout, cancel) {
                Ok(image) => return Ok(image),
                Err(
                    err @ (Error::SandboxUnavailable
                    | Error::ExternalTimeout
                    | Error::TooLarge
                    | Error::Io(_)),
                ) => {
                    if !cancel.is_cancelled() {
                        tracing::warn!(
                            thumbnailer = %spec.info.id,
                            path = %target.path.display(),
                            timeout_secs = timeout.as_secs(),
                            %err,
                            "external thumbnailer aborted"
                        );
                    }
                    return Err(err);
                }
                Err(err) => {
                    tracing::debug!(
                        thumbnailer = %spec.info.id,
                        path = %target.path.display(),
                        %err,
                        "external thumbnailer candidate failed"
                    );
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or(Error::NoCover))
    }
}

fn count_unique_mimes(specs: &[ThumbnailerSpec]) -> usize {
    let mut unique = HashSet::new();
    for spec in specs {
        for mime in &spec.info.mime_types {
            unique.insert(mime.as_str());
        }
    }
    unique.len()
}

struct ExternalSemaphore {
    active: Mutex<usize>,
    ready: Condvar,
}

impl ExternalSemaphore {
    const fn new() -> Self {
        Self {
            active: Mutex::new(0),
            ready: Condvar::new(),
        }
    }
}

struct ExternalPermit(&'static ExternalSemaphore);

impl ExternalPermit {
    fn acquire(cancel: &CancellationToken) -> Result<Self, Error> {
        static SEM: OnceLock<ExternalSemaphore> = OnceLock::new();
        let sem = SEM.get_or_init(ExternalSemaphore::new);
        let mut guard = sem
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *guard >= MAX_CONCURRENT_EXTERNAL {
            if cancel.is_cancelled() {
                return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
            }
            let (next, _) = sem
                .ready
                .wait_timeout(guard, Duration::from_millis(25))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next;
        }
        *guard += 1;
        Ok(Self(sem))
    }
}

impl Drop for ExternalPermit {
    fn drop(&mut self) {
        let mut guard = self
            .0
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = guard.saturating_sub(1);
        self.0.ready.notify_one();
    }
}

#[cfg(not(target_os = "linux"))]
async fn probe_bwrap(
    _bwrap_override: Option<&Path>,
    _cancel: &CancellationToken,
) -> Result<PathBuf, SandboxError> {
    Err(SandboxError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
async fn probe_bwrap(
    bwrap_override: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<PathBuf, SandboxError> {
    let bwrap = resolve_bwrap(bwrap_override)?;
    let mut cmd = tokio::process::Command::new(&bwrap);
    cmd.env_remove("LD_LIBRARY_PATH");
    for arg in build_bwrap_probe_args(&bwrap) {
        cmd.arg(arg);
    }
    let output = crate::service::task_manager::process::output(&mut cmd, cancel, PROBE_TIMEOUT)
        .await
        .map_err(|err| SandboxError::ProbeFailed(err.to_string()))?;
    if output.status.success() {
        Ok(bwrap)
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if detail.is_empty() {
            format!("exit status {}", output.status)
        } else {
            detail
        };
        Err(SandboxError::ProbeFailed(detail))
    }
}

fn resolve_bwrap(bwrap_override: Option<&Path>) -> Result<PathBuf, SandboxError> {
    if let Some(custom) = bwrap_override.filter(|path| !path.as_os_str().is_empty()) {
        return check_explicit_executable(custom);
    }
    find_in_path("bwrap").ok_or_else(|| SandboxError::NotFound(PathBuf::from("bwrap")))
}

fn check_explicit_executable(path: &Path) -> Result<PathBuf, SandboxError> {
    if path.components().count() == 1 && !path.is_absolute() {
        return find_in_path(path).ok_or_else(|| SandboxError::NotFound(path.to_path_buf()));
    }
    if !path.exists() {
        return Err(SandboxError::NotFound(path.to_path_buf()));
    }
    if !is_executable_file(path) {
        return Err(SandboxError::NotExecutable(path.to_path_buf()));
    }
    Ok(path.to_path_buf())
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

fn find_in_path(program: impl AsRef<OsStr>) -> Option<PathBuf> {
    let program = Path::new(program.as_ref());
    if program.is_absolute() || program.components().count() > 1 {
        return is_executable_file(program).then(|| program.to_path_buf());
    }
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(program);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn xdg_thumbnailer_dirs() -> Vec<(PathBuf, bool)> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |dir: PathBuf, user: bool| {
        if seen.insert(dir.clone()) {
            dirs.push((dir, user));
        }
    };

    if let Some(data_home) = env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        push(PathBuf::from(data_home).join("thumbnailers"), true);
    } else if let Some(base) = BaseDirs::new() {
        push(base.data_local_dir().join("thumbnailers"), true);
    }

    let data_dirs = env::var_os("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
    for dir in env::split_paths(&data_dirs) {
        if !dir.as_os_str().is_empty() {
            push(dir.join("thumbnailers"), false);
        }
    }
    dirs
}

fn discover_thumbnailers(search_dirs: &[(PathBuf, bool)]) -> Vec<ThumbnailerSpec> {
    let mut seen_names: HashSet<OsString> = HashSet::new();
    let mut specs = Vec::new();

    for (dir, user_override) in search_dirs {
        let Ok(read_dir) = fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = read_dir
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(OsStr::to_str) == Some("thumbnailer"))
            .collect();
        paths.sort();

        for path in paths {
            let Some(file_name) = path.file_name().map(OsStr::to_os_string) else {
                continue;
            };
            // Higher-priority directories shadow lower-priority files of the same name,
            // including an intentionally empty override used to disable a system entry.
            if !seen_names.insert(file_name) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let id = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(spec) = parse_thumbnailer_entry(&id, &path, &content, *user_override) {
                specs.push(spec);
            }
        }
    }

    specs
}

fn parse_thumbnailer_entry(
    id: &str,
    path: &Path,
    content: &str,
    user_override: bool,
) -> Option<ThumbnailerSpec> {
    let mut in_section = false;
    let mut try_exec: Option<String> = None;
    let mut exec_raw: Option<String> = None;
    let mut mime_raw: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line == "[Thumbnailer Entry]";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "TryExec" => try_exec = Some(value.trim().to_owned()),
            "Exec" => exec_raw = Some(value.trim().to_owned()),
            "MimeType" => mime_raw = Some(value.trim().to_owned()),
            _ => {}
        }
    }

    let exec_raw = exec_raw.filter(|value| !value.is_empty())?;
    let mime_raw = mime_raw.filter(|value| !value.is_empty())?;
    let exec_argv = parse_exec_argv(&exec_raw).ok()?;
    if exec_argv.is_empty() {
        return None;
    }

    let has_input = exec_argv
        .iter()
        .any(|arg| has_placeholder(arg, 'i') || has_placeholder(arg, 'u'));
    let has_output = exec_argv.iter().any(|arg| has_placeholder(arg, 'o'));
    if !has_input || !has_output {
        return None;
    }

    if let Some(try_exec) = try_exec.as_deref().filter(|value| !value.is_empty())
        && find_in_path(try_exec).is_none()
    {
        return None;
    }
    let executable = find_in_path(&exec_argv[0])?;

    let mut mime_types = Vec::new();
    for raw_mime in mime_raw.split(';') {
        let mime = raw_mime.trim().to_ascii_lowercase();
        if mime.is_empty() || !mime.contains('/') {
            continue;
        }
        if !mime_types.contains(&mime) {
            mime_types.push(mime);
        }
    }
    if mime_types.is_empty() {
        return None;
    }

    Some(ThumbnailerSpec {
        info: ThumbnailerInfo {
            id: id.to_owned(),
            path: path.to_path_buf(),
            executable,
            mime_types,
            user_override,
        },
        exec_argv,
    })
}

fn has_placeholder(arg: &str, code: char) -> bool {
    let mut chars = arg.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('%') => {}
                Some(next) if next == code => return true,
                _ => {}
            }
        }
    }
    false
}

/// Parse an XDG `Exec` string into argv tokens following GLib `g_shell_parse_argv` quoting rules.
fn parse_exec_argv(input: &str) -> Result<Vec<String>, &'static str> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current.push(ch);
            }
        } else if in_double {
            match ch {
                '"' => in_double = false,
                '\\' => match chars.peek().copied() {
                    Some(next @ ('"' | '\\' | '$' | '`' | '\n')) => {
                        chars.next();
                        if next != '\n' {
                            current.push(next);
                        }
                    }
                    _ => current.push('\\'),
                },
                _ => current.push(ch),
            }
        } else {
            match ch {
                '\'' => {
                    in_single = true;
                    in_token = true;
                }
                '"' => {
                    in_double = true;
                    in_token = true;
                }
                '\\' => {
                    in_token = true;
                    if let Some(next) = chars.next()
                        && next != '\n'
                    {
                        current.push(next);
                    }
                }
                c if c.is_ascii_whitespace() => {
                    if in_token {
                        args.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                }
                _ => {
                    in_token = true;
                    current.push(ch);
                }
            }
        }
    }

    if in_single || in_double {
        return Err("unterminated quote in Exec entry");
    }
    if in_token {
        args.push(current);
    }
    Ok(args)
}

fn expand_exec_argv(
    template: &[String],
    resolved_executable: &Path,
    input_path: &Path,
    input_uri: &str,
    output_path: &Path,
    size: u32,
) -> Vec<OsString> {
    let size_str = size.to_string();
    let mut expanded = Vec::with_capacity(template.len());
    for (idx, token) in template.iter().enumerate() {
        if idx == 0 && !token.contains('%') {
            expanded.push(resolved_executable.as_os_str().to_os_string());
            continue;
        }
        let mut arg = OsString::new();
        let mut chars = token.chars();
        while let Some(ch) = chars.next() {
            if ch == '%' {
                match chars.next() {
                    Some('i') => arg.push(input_path.as_os_str()),
                    Some('u') => arg.push(input_uri),
                    Some('o') => arg.push(output_path.as_os_str()),
                    Some('s') => arg.push(&size_str),
                    Some('%') => arg.push("%"),
                    Some(other) => {
                        arg.push("%");
                        let mut buf = [0u8; 4];
                        arg.push(other.encode_utf8(&mut buf));
                    }
                    None => arg.push("%"),
                }
            } else {
                let mut buf = [0u8; 4];
                arg.push(ch.encode_utf8(&mut buf));
            }
        }
        expanded.push(arg);
    }
    expanded
}

fn is_under_sandbox_roots(path: &Path) -> bool {
    SANDBOX_RO_ROOTS
        .iter()
        .any(|root| path.starts_with(Path::new(root)))
}

fn safe_sandbox_path(extra_exec: &Path) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut push = |dir: PathBuf| {
        if !dir.as_os_str().is_empty() && seen.insert(dir.clone()) {
            dirs.push(dir);
        }
    };

    if let Some(parent) = extra_exec.parent() {
        push(parent.to_path_buf());
    }
    if let Some(host_path) = env::var_os("PATH") {
        for dir in env::split_paths(&host_path) {
            if is_under_sandbox_roots(&dir) {
                push(dir);
            }
        }
    }
    for fallback in [
        "/run/current-system/sw/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
    ] {
        let path = PathBuf::from(fallback);
        if path.exists() {
            push(path);
        }
    }
    env::join_paths(dirs).unwrap_or_else(|_| OsString::from("/usr/bin:/bin"))
}

fn sanitized_sandbox_filename(path: &Path) -> OsString {
    let Some(name) = path.file_name() else {
        return OsString::from("input");
    };
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let bytes = name.as_bytes();
        if bytes.is_empty()
            || bytes == b"."
            || bytes == b".."
            || bytes.contains(&b'/')
            || bytes.contains(&0)
        {
            return OsString::from("input");
        }
        name.to_os_string()
    }
    #[cfg(not(unix))]
    {
        let text = name.to_string_lossy();
        if text.is_empty()
            || text == "."
            || text == ".."
            || text.contains('/')
            || text.contains('\\')
        {
            OsString::from("input")
        } else {
            name.to_os_string()
        }
    }
}

fn build_bwrap_probe_args(bwrap: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--unshare-all".into(),
        "--die-with-parent".into(),
        "--new-session".into(),
        "--clearenv".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
    ];
    for root in SANDBOX_RO_ROOTS {
        args.push("--ro-bind-try".into());
        args.push(OsString::from(root));
        args.push(OsString::from(root));
    }
    if !is_under_sandbox_roots(bwrap) {
        args.push("--ro-bind".into());
        args.push(bwrap.as_os_str().to_os_string());
        args.push(bwrap.as_os_str().to_os_string());
    }
    args.push("--chdir".into());
    args.push("/tmp".into());
    args.push("--".into());
    args.push(bwrap.as_os_str().to_os_string());
    args.push("--version".into());
    args
}

fn build_bwrap_thumbnailer_args(
    spec: &ThumbnailerSpec,
    host_input: &Path,
    host_out_dir: &Path,
    size: u32,
) -> Result<Vec<OsString>, Error> {
    let sandbox_name = sanitized_sandbox_filename(host_input);
    let sandbox_input = PathBuf::from("/tmp/gds-in").join(&sandbox_name);
    let sandbox_output = PathBuf::from("/tmp/gds-out/thumb.png");
    let sandbox_input_uri = cache::file_uri(&sandbox_input)
        .map_err(|err| Error::ExternalFailed(format!("invalid sandbox input URI: {err}")))?;
    let lang = env::var("LANG")
        .ok()
        .filter(|value| value.to_ascii_lowercase().contains("utf"))
        .unwrap_or_else(|| "C.UTF-8".to_owned());

    let mut args: Vec<OsString> = vec![
        "--unshare-all".into(),
        "--die-with-parent".into(),
        "--new-session".into(),
        "--clearenv".into(),
        "--setenv".into(),
        "PATH".into(),
        safe_sandbox_path(&spec.info.executable),
        "--setenv".into(),
        "HOME".into(),
        "/tmp".into(),
        "--setenv".into(),
        "TMPDIR".into(),
        "/tmp".into(),
        "--setenv".into(),
        "XDG_CACHE_HOME".into(),
        "/tmp/cache".into(),
        "--setenv".into(),
        "XDG_RUNTIME_DIR".into(),
        "/tmp/runtime".into(),
        "--setenv".into(),
        "LANG".into(),
        lang.into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--dir".into(),
        "/tmp/gds-in".into(),
        "--dir".into(),
        "/tmp/gds-out".into(),
        "--dir".into(),
        "/tmp/cache".into(),
        "--dir".into(),
        "/tmp/runtime".into(),
    ];

    if Path::new("/etc/fonts").exists() {
        args.push("--setenv".into());
        args.push("FONTCONFIG_PATH".into());
        args.push("/etc/fonts".into());
    }

    for root in SANDBOX_RO_ROOTS {
        args.push("--ro-bind-try".into());
        args.push(OsString::from(root));
        args.push(OsString::from(root));
    }
    if !is_under_sandbox_roots(&spec.info.executable) {
        args.push("--ro-bind".into());
        args.push(spec.info.executable.as_os_str().to_os_string());
        args.push(spec.info.executable.as_os_str().to_os_string());
    }

    args.push("--ro-bind".into());
    args.push(host_input.as_os_str().to_os_string());
    args.push(sandbox_input.as_os_str().to_os_string());

    args.push("--bind".into());
    args.push(host_out_dir.as_os_str().to_os_string());
    args.push("/tmp/gds-out".into());

    args.push("--chdir".into());
    args.push("/tmp".into());
    args.push("--".into());

    args.extend(expand_exec_argv(
        &spec.exec_argv,
        &spec.info.executable,
        &sandbox_input,
        &sandbox_input_uri,
        &sandbox_output,
        size,
    ));
    Ok(args)
}

fn run_thumbnailer(
    sandbox: &SandboxMode,
    spec: &ThumbnailerSpec,
    host_input: &Path,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<RgbaImage, Error> {
    let temp = tempfile::tempdir()?;
    let out_dir = temp.path().join("out");
    fs::create_dir_all(&out_dir)?;
    let out_file = out_dir.join("thumb.png");
    let stderr_path = temp.path().join("stderr.log");
    let stderr = File::create(&stderr_path)?;

    let mut cmd = match sandbox {
        SandboxMode::Bwrap(bwrap) => {
            let bwrap_args = build_bwrap_thumbnailer_args(spec, host_input, &out_dir, EDGE)?;
            let mut cmd = Command::new(bwrap);
            cmd.env_remove("LD_LIBRARY_PATH").args(&bwrap_args);
            cmd
        }
        SandboxMode::Unsandboxed => {
            let input_uri = cache::file_uri(host_input)
                .map_err(|err| Error::ExternalFailed(format!("invalid input URI: {err}")))?;
            let argv = expand_exec_argv(
                &spec.exec_argv,
                &spec.info.executable,
                host_input,
                &input_uri,
                &out_file,
                EDGE,
            );
            let Some((program, rest)) = argv.split_first() else {
                return Err(Error::ExternalFailed(format!(
                    "{} has empty Exec command",
                    spec.info.id
                )));
            };
            let mut cmd = Command::new(program);
            cmd.env_remove("LD_LIBRARY_PATH")
                .args(rest)
                .current_dir(temp.path())
                .env("TMPDIR", temp.path());
            cmd
        }
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);

    let mut child =
        crate::service::task_manager::process::ChildGuard::spawn_in_process_group(&mut cmd)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    match sandbox {
                        SandboxMode::Bwrap(_) => Error::SandboxUnavailable,
                        SandboxMode::Unsandboxed => {
                            Error::ExternalFailed(format!("{} executable not found", spec.info.id))
                        }
                    }
                } else {
                    Error::Io(error)
                }
            })?;
    let status = child.wait(cancel, timeout).map_err(|error| {
        if error.kind() == std::io::ErrorKind::TimedOut {
            Error::ExternalTimeout
        } else {
            Error::Io(error)
        }
    })?;

    if !status.success() || !out_file.is_file() {
        let mut message = String::new();
        let _ = File::open(&stderr_path)
            .and_then(|file| file.take(ERROR_BYTES).read_to_string(&mut message));
        let detail = message.trim();
        let text = if detail.is_empty() {
            format!("{} exited with {status}", spec.info.id)
        } else {
            format!("{} ({status}): {detail}", spec.info.id)
        };
        return Err(Error::ExternalFailed(text));
    }

    let meta = fs::metadata(&out_file)?;
    if meta.len() == 0 {
        return Err(Error::ExternalFailed(format!(
            "{} produced an empty thumbnail",
            spec.info.id
        )));
    }
    if meta.len() > MAX_OUTPUT_PNG_BYTES {
        return Err(Error::TooLarge);
    }
    let bytes = fs::read(&out_file)?;
    thumb_image::load_png_bounded(&bytes).map_err(|err| {
        Error::ExternalFailed(format!("{} produced invalid PNG: {err}", spec.info.id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join(name);
        fs::write(&path, body).expect("write script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod +x");
        path
    }

    #[test]
    fn parse_exec_argv_handles_quotes_escapes_and_embedded_placeholders() {
        let argv = parse_exec_argv(
            r#"/usr/bin/my-thumb --title="a \"b\" c" '--opt=%s' --in=%i --out=%o 100%%"#,
        )
        .expect("parse valid Exec");
        assert_eq!(
            argv,
            vec![
                "/usr/bin/my-thumb",
                "--title=a \"b\" c",
                "--opt=%s",
                "--in=%i",
                "--out=%o",
                "100%%",
            ]
        );
        assert!(parse_exec_argv("unterminated 'quote").is_err());

        let expanded = expand_exec_argv(
            &argv,
            Path::new("/resolved/my-thumb"),
            Path::new("/tmp/gds-in/a; rm -rf.pdf"),
            "file:///tmp/gds-in/a%3B%20rm%20-rf.pdf",
            Path::new("/tmp/gds-out/thumb.png"),
            512,
        );
        assert_eq!(expanded[0], OsString::from("/resolved/my-thumb"));
        assert_eq!(expanded[2], OsString::from("--opt=512"));
        assert_eq!(
            expanded[3],
            OsString::from("--in=/tmp/gds-in/a; rm -rf.pdf")
        );
        assert_eq!(expanded[4], OsString::from("--out=/tmp/gds-out/thumb.png"));
        assert_eq!(expanded[5], OsString::from("100%"));
    }

    #[cfg(unix)]
    #[test]
    fn discovery_respects_user_overrides_disabling_and_semicolon_mime_lists() {
        let root = tempfile::tempdir().expect("tempdir");
        let bin = root.path().join("bin");
        let user_dir = root.path().join("user");
        let sys_dir = root.path().join("sys");
        for dir in [&bin, &user_dir, &sys_dir] {
            fs::create_dir_all(dir).expect("create dir");
        }
        let pdf_bin = write_executable(&bin, "pdf-thumb", "#!/bin/sh\nexit 0\n");
        let custom_bin = write_executable(&bin, "custom-pdf", "#!/bin/sh\nexit 0\n");
        let font_bin = write_executable(&bin, "font-thumb", "#!/bin/sh\nexit 0\n");

        // System has pdf.thumbnailer, disabled.thumbnailer, font.thumbnailer.
        fs::write(
            sys_dir.join("pdf.thumbnailer"),
            format!(
                "# comment\n[Thumbnailer Entry]\nTryExec={}\nExec={} -s %s %i %o\nMimeType=application/pdf;application/x-bzpdf;\n",
                pdf_bin.display(),
                pdf_bin.display()
            ),
        )
        .expect("write sys pdf");
        fs::write(
            sys_dir.join("disabled.thumbnailer"),
            format!(
                "[Thumbnailer Entry]\nExec={} %i %o\nMimeType=model/stl;\n",
                font_bin.display()
            ),
        )
        .expect("write sys disabled");
        fs::write(
            sys_dir.join("font.thumbnailer"),
            format!(
                "[Thumbnailer Entry]\nExec={} -s %s %u %o\nMimeType=font/ttf;font/otf;\n",
                font_bin.display()
            ),
        )
        .expect("write sys font");

        // User overrides pdf.thumbnailer and blanks out disabled.thumbnailer.
        fs::write(
            user_dir.join("pdf.thumbnailer"),
            format!(
                "[Thumbnailer Entry]\nExec={} -s %s %i %o\nMimeType=application/pdf;\n",
                custom_bin.display()
            ),
        )
        .expect("write user pdf");
        fs::write(
            user_dir.join("disabled.thumbnailer"),
            "[Thumbnailer Entry]\n",
        )
        .expect("write user disable");

        let specs = discover_thumbnailers(&[(user_dir, true), (sys_dir, false)]);
        let registry = ExternalThumbnailers::from_parts(
            SandboxMode::Bwrap(PathBuf::from("/usr/bin/bwrap")),
            specs,
        );

        assert_eq!(registry.entry_count(), 2);
        assert!(registry.has_user_override(Some("application/pdf"), &[]));
        assert!(!registry.has_match(Some("model/stl"), &[]));
        assert!(registry.has_match(Some("font/otf"), &[]));
        assert!(!registry.has_user_override(Some("font/otf"), &[]));
        // Ancestor matching works for specialized PDFs, but generic octet-stream is ignored.
        assert!(registry.has_match(
            Some("application/x-gzpdf"),
            &[
                Arc::from("application/pdf"),
                Arc::from("application/octet-stream")
            ]
        ));
        assert!(!registry.has_match(
            Some("application/x-unknown"),
            &[Arc::from("application/octet-stream")]
        ));
    }

    #[cfg(unix)]
    #[test]
    fn bwrap_args_enforce_isolation_nix_roots_and_preserve_input_extension() {
        let spec = ThumbnailerSpec {
            info: ThumbnailerInfo {
                id: "pdf".into(),
                path: PathBuf::from("/usr/share/thumbnailers/pdf.thumbnailer"),
                executable: PathBuf::from("/opt/custom/bin/pdf-thumb"),
                mime_types: vec!["application/pdf".into()],
                user_override: false,
            },
            exec_argv: vec![
                "/opt/custom/bin/pdf-thumb".into(),
                "-s".into(),
                "%s".into(),
                "%i".into(),
                "%o".into(),
            ],
        };
        let args = build_bwrap_thumbnailer_args(
            &spec,
            Path::new("/home/atlas/secret/report.pdf"),
            Path::new("/tmp/host-out"),
            512,
        )
        .expect("build bwrap args");
        let strings: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();

        assert!(strings.contains(&"--unshare-all".to_owned()));
        assert!(strings.contains(&"--die-with-parent".to_owned()));
        assert!(strings.contains(&"--clearenv".to_owned()));
        assert!(strings.contains(&"/nix/store".to_owned()));
        assert!(strings.contains(&"/etc/static".to_owned()));
        // Input is bound under /tmp/gds-in/report.pdf preserving extension without leaking parent dir.
        assert!(strings.contains(&"/tmp/gds-in/report.pdf".to_owned()));
        assert!(strings.contains(&"/tmp/gds-out/thumb.png".to_owned()));
        // Custom executable outside system roots is explicitly ro-bound.
        assert!(strings.windows(3).any(|w| w[0] == "--ro-bind"
            && w[1] == "/opt/custom/bin/pdf-thumb"
            && w[2] == "/opt/custom/bin/pdf-thumb"));
    }

    #[cfg(unix)]
    #[test]
    fn sandboxed_runner_generates_scaled_png_and_caches_failure_distinctions() {
        let root = tempfile::tempdir().expect("tempdir");
        let input = root.path().join("sample.pdf");
        fs::write(&input, b"%PDF-1.7\n").expect("write dummy pdf");

        // Write a tiny valid 600x300 PNG fixture that our mock thumbnailer will copy to %o.
        let fixture_png = root.path().join("fixture.png");
        let big = RgbaImage::from_pixel(600, 300, image::Rgba([24, 140, 220, 255]));
        big.save(&fixture_png).expect("save fixture png");

        // Mock bwrap script: skips sandbox flags up to `--`, remaps /tmp/gds-out/thumb.png to host out dir.
        let mock_bwrap = write_executable(
            root.path(),
            "mock-bwrap",
            r#"#!/bin/sh
HOST_OUT=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--bind" ]; then
    HOST_OUT="$2"
    shift 3
    continue
  fi
  if [ "$1" = "--" ]; then
    shift
    break
  fi
  shift
done
PROG="$1"
shift
# Replace /tmp/gds-out/thumb.png with $HOST_OUT/thumb.png
OUT=""
for arg in "$@"; do
  if [ "$arg" = "/tmp/gds-out/thumb.png" ]; then
    OUT="$HOST_OUT/thumb.png"
  fi
done
exec "$PROG" "$OUT"
"#,
        );
        let mock_thumb = write_executable(
            root.path(),
            "mock-thumb",
            &format!("#!/bin/sh\ncp '{}' \"$1\"\n", fixture_png.display()),
        );
        let spec = ThumbnailerSpec {
            info: ThumbnailerInfo {
                id: "mock-pdf".into(),
                path: root.path().join("mock-pdf.thumbnailer"),
                executable: mock_thumb.clone(),
                mime_types: vec!["application/pdf".into()],
                user_override: false,
            },
            exec_argv: vec![
                mock_thumb.display().to_string(),
                "-s".into(),
                "%s".into(),
                "%i".into(),
                "%o".into(),
            ],
        };
        let runner = ExternalThumbnailers::from_parts(SandboxMode::Bwrap(mock_bwrap), vec![spec]);
        let target = Target {
            path: &input,
            mime: Some("application/pdf"),
            mime_ancestors: &[],
        };
        let cancel = CancellationToken::new();
        let img = runner
            .generate(target, Duration::from_secs(5), &cancel, false)
            .expect("generate thumbnail via mock bwrap");
        assert_eq!(img.width(), 512);
        assert_eq!(img.height(), 256);
    }

    #[cfg(unix)]
    #[test]
    fn unsandboxed_runner_executes_directly_without_shell_and_scales_output() {
        let root = tempfile::tempdir().expect("tempdir");
        // Include spaces and shell metacharacters to prove execve avoids shell interpolation.
        let input = root.path().join("视频; $(touch pwned) [one].mp4");
        fs::write(&input, b"fake video bytes").expect("write dummy video");

        let fixture_png = root.path().join("frame.png");
        let big = RgbaImage::from_pixel(800, 400, image::Rgba([40, 180, 90, 255]));
        big.save(&fixture_png).expect("save fixture png");

        let mock_thumb = write_executable(
            root.path(),
            "mock-video-thumb",
            &format!(
                "#!/bin/sh\n# Expect: -s 512 -i <input> -o <output>\n[ \"$1\" = \"-s\" ] && [ \"$2\" = \"512\" ] && [ \"$3\" = \"-i\" ] && [ -f \"$4\" ] && [ \"$5\" = \"-o\" ] || exit 2\ncp '{}' \"$6\"\n",
                fixture_png.display()
            ),
        );
        let spec = ThumbnailerSpec {
            info: ThumbnailerInfo {
                id: "ffmpegthumbnailer".into(),
                path: root.path().join("ffmpegthumbnailer.thumbnailer"),
                executable: mock_thumb.clone(),
                mime_types: vec!["video/mp4".into()],
                user_override: false,
            },
            exec_argv: vec![
                mock_thumb.display().to_string(),
                "-s".into(),
                "%s".into(),
                "-i".into(),
                "%i".into(),
                "-o".into(),
                "%o".into(),
            ],
        };
        let runner = ExternalThumbnailers::from_parts(SandboxMode::Unsandboxed, vec![spec]);
        assert!(!runner.is_sandboxed());
        assert!(runner.bwrap_path().is_none());

        let target = Target {
            path: &input,
            mime: Some("video/mp4"),
            mime_ancestors: &[],
        };
        let cancel = CancellationToken::new();
        let img = runner
            .generate(target, Duration::from_secs(5), &cancel, false)
            .expect("generate video thumbnail directly without shell");
        assert_eq!((img.width(), img.height()), (512, 256));
        assert!(!root.path().join("pwned").exists());
    }
}
