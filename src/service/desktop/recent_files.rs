//! FreeDesktop Desktop Bookmark Specification 1.0 (`$XDG_DATA_HOME/recently-used.xbel`).
//!
//! Reads and losslessly updates the shared desktop recent-files XBEL document, preserving
//! third-party application metadata, groups, private flags, and non-`file://` entries across
//! concurrent desktop writers.

use std::{
    collections::HashSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use chrono::{DateTime, SecondsFormat, Utc};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, percent_encode};
use roxmltree::{Document, Node};

const BOOKMARK_NS: &str = "http://www.freedesktop.org/standards/desktop-bookmarks";
const MIME_NS: &str = "http://www.freedesktop.org/standards/shared-mime-info";
const FREEDESKTOP_OWNER: &str = "http://freedesktop.org";
const MAX_XBEL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_BOOKMARKS: usize = 1000;
const LOCK_RETRY_BUDGET: Duration = Duration::from_secs(1);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(2);

const DEFAULT_XBEL_OPEN_TAG: &str = concat!(
    "<xbel version=\"1.0\"\n",
    "      xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n",
    "      xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\"\n",
    ">",
);

const FILE_URI_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'\\')
    .add(b'^')
    .add(b'[')
    .add(b']');

static WRITE_LOCK: Mutex<()> = Mutex::new(());
static LOCAL_HOSTNAME: OnceLock<Option<String>> = OnceLock::new();

/// One visible local-file entry parsed from `recently-used.xbel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub uri: String,
    pub mime_type: Option<String>,
    pub added: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub visited: Option<SystemTime>,
}

impl RecentEntry {
    /// Most recent timestamp among `visited`, `modified`, and `added`.
    #[must_use]
    pub fn latest_timestamp(&self) -> Option<SystemTime> {
        [self.visited, self.modified, self.added]
            .into_iter()
            .flatten()
            .max()
    }
}

/// Resolve `$XDG_DATA_HOME/recently-used.xbel` (falling back to `~/.local/share/recently-used.xbel`).
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("XDG_DATA_HOME") {
        let dir = PathBuf::from(raw);
        if dir.is_absolute() {
            return Some(dir.join("recently-used.xbel"));
        }
    }
    directories::BaseDirs::new().map(|dirs| dirs.data_local_dir().join("recently-used.xbel"))
}

/// List visible local recent files from the user's default `recently-used.xbel`, newest first.
///
/// # Errors
/// Reports unreadable or malformed XBEL files (missing files return an empty list).
pub fn list_default() -> io::Result<Vec<RecentEntry>> {
    let Some(path) = default_path() else {
        return Ok(Vec::new());
    };
    read_from(&path)
}

/// Record a local file access into the user's default `recently-used.xbel`.
///
/// # Errors
/// Rejects non-absolute paths and reports XML or filesystem errors.
pub fn record_default(
    file_path: &Path,
    mime_type: Option<&str>,
    app_name: &str,
    exec: &str,
) -> io::Result<()> {
    let Some(xbel_path) = default_path() else {
        return Ok(());
    };
    record_at(&xbel_path, file_path, mime_type, app_name, exec)
}

/// Remove a local file entry from the user's default `recently-used.xbel`.
///
/// # Errors
/// Reports XML or filesystem errors.
pub fn remove_default(file_path: &Path) -> io::Result<()> {
    let Some(xbel_path) = default_path() else {
        return Ok(());
    };
    remove_at(&xbel_path, file_path)
}

/// Clear all entries in the user's default `recently-used.xbel`.
///
/// # Errors
/// Reports filesystem errors writing the cleared XBEL document.
pub fn clear_default() -> io::Result<()> {
    let Some(xbel_path) = default_path() else {
        return Ok(());
    };
    clear_at(&xbel_path)
}

/// Parse visible local-file entries from `xbel_path`, sorted newest-first and deduplicated by path.
///
/// # Errors
/// Reports I/O failures (other than `NotFound`), oversized files, or malformed XML.
pub fn read_from(xbel_path: &Path) -> io::Result<Vec<RecentEntry>> {
    let Some(xml) = read_xbel_string(xbel_path)? else {
        return Ok(Vec::new());
    };
    parse_entries(&xml)
}

fn read_xbel_string(xbel_path: &Path) -> io::Result<Option<String>> {
    let meta = match fs::metadata(xbel_path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if meta.len() > MAX_XBEL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "recently-used.xbel exceeds size limit",
        ));
    }
    let content = fs::read_to_string(xbel_path)?;
    Ok(Some(content))
}

fn parse_entries(xml: &str) -> io::Result<Vec<RecentEntry>> {
    let doc =
        Document::parse(xml).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let root = doc.root_element();
    if root.tag_name().name() != "xbel" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected <xbel> root element",
        ));
    }

    let mut entries = Vec::new();
    for node in root
        .children()
        .filter(|child| child.is_element() && child.tag_name().name() == "bookmark")
    {
        let Some(href) = node.attribute("href") else {
            continue;
        };
        if is_private_bookmark(node) {
            continue;
        }
        let Some(path) = parse_local_file_uri(href) else {
            continue;
        };
        let added = node.attribute("added").and_then(parse_timestamp);
        let modified = node.attribute("modified").and_then(parse_timestamp);
        let visited = node.attribute("visited").and_then(parse_timestamp);
        let mime_type = bookmark_mime_type(node).map(str::to_owned);
        entries.push(RecentEntry {
            path,
            uri: href.to_owned(),
            mime_type,
            added,
            modified,
            visited,
        });
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.latest_timestamp()));
    let mut seen = HashSet::with_capacity(entries.len());
    entries.retain(|entry| seen.insert(entry.path.clone()));
    Ok(entries)
}

/// Record or update `file_path` in `xbel_path`, preserving all untouched bookmarks and foreign
/// metadata losslessly.
///
/// # Errors
/// Rejects non-absolute paths and reports XML or filesystem failures.
pub fn record_at(
    xbel_path: &Path,
    file_path: &Path,
    mime_type: Option<&str>,
    app_name: &str,
    exec: &str,
) -> io::Result<()> {
    if !file_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "recent file path must be absolute",
        ));
    }
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
    record_at_timestamp(xbel_path, file_path, mime_type, app_name, exec, &now)
}

fn record_at_timestamp(
    xbel_path: &Path,
    file_path: &Path,
    mime_type: Option<&str>,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> io::Result<()> {
    let parent = xbel_parent(xbel_path)?;
    fs::create_dir_all(parent)?;
    let _guard = acquire_write_lock(xbel_path)?;

    let target_uri = encode_local_file_uri(file_path);
    let mime = mime_type
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("application/octet-stream");

    let existing_xml = read_xbel_string(xbel_path)?;
    let mut envelope = DocumentEnvelope::default();
    let mut bookmarks: Vec<(Option<SystemTime>, String)> = Vec::new();
    let mut updated_existing = None;

    if let Some(ref xml) = existing_xml
        && let Ok(doc) = Document::parse(xml)
        && doc.root_element().tag_name().name() == "xbel"
    {
        let root = doc.root_element();
        envelope = DocumentEnvelope::from_root(xml, root);
        for node in root
            .children()
            .filter(|child| child.is_element() && child.tag_name().name() == "bookmark")
        {
            let raw = &xml[node.range()];
            let is_match = node.attribute("href").is_some_and(|href| {
                href == target_uri || parse_local_file_uri(href).as_deref() == Some(file_path)
            });
            if is_match && updated_existing.is_none() {
                updated_existing = Some(update_existing_bookmark(
                    xml, node, &envelope, mime, app_name, exec, timestamp,
                ));
            } else if !is_match {
                let ts = [
                    node.attribute("visited"),
                    node.attribute("modified"),
                    node.attribute("added"),
                ]
                .into_iter()
                .flatten()
                .filter_map(parse_timestamp)
                .max();
                bookmarks.push((ts, raw.to_owned()));
            }
        }
    }

    let entry_xml = updated_existing.unwrap_or_else(|| {
        format_new_bookmark(&envelope, &target_uri, mime, app_name, exec, timestamp)
    });
    let entry_ts = parse_timestamp(timestamp);
    bookmarks.push((entry_ts, entry_xml));

    if bookmarks.len() > MAX_BOOKMARKS {
        // Keep the newest MAX_BOOKMARKS entries while preserving relative order among survivors.
        let mut order: Vec<usize> = (0..bookmarks.len()).collect();
        order.sort_by(|&a, &b| bookmarks[b].0.cmp(&bookmarks[a].0));
        let keep: HashSet<usize> = order.into_iter().take(MAX_BOOKMARKS).collect();
        bookmarks = bookmarks
            .into_iter()
            .enumerate()
            .filter_map(|(idx, item)| keep.contains(&idx).then_some(item))
            .collect();
    }

    let kept_raw: Vec<String> = bookmarks.into_iter().map(|(_, raw)| raw).collect();
    let out = render_xbel(&envelope, &kept_raw);
    atomic_write_xbel(xbel_path, out.as_bytes())
}

/// Remove any local-file bookmark matching `file_path` from `xbel_path`, preserving all other
/// entries, foreign XML namespaces, and non-bookmark nodes verbatim.
///
/// # Errors
/// Reports XML or filesystem failures.
pub fn remove_at(xbel_path: &Path, file_path: &Path) -> io::Result<()> {
    let parent = xbel_parent(xbel_path)?;
    if !parent.exists() {
        return Ok(());
    }
    let _guard = acquire_write_lock(xbel_path)?;

    let Some(xml) = read_xbel_string(xbel_path)? else {
        return Ok(());
    };
    let doc =
        Document::parse(&xml).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let root = doc.root_element();
    if root.tag_name().name() != "xbel" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected <xbel> root element",
        ));
    }

    let envelope = DocumentEnvelope::from_root(&xml, root);
    let mut kept = Vec::new();
    let mut removed = false;
    for node in root
        .children()
        .filter(|child| child.is_element() && child.tag_name().name() == "bookmark")
    {
        let matches_target = node
            .attribute("href")
            .and_then(parse_local_file_uri)
            .as_deref()
            == Some(file_path);
        if matches_target {
            removed = true;
        } else {
            kept.push(xml[node.range()].trim().to_owned());
        }
    }
    if !removed {
        return Ok(());
    }

    let out = render_xbel(&envelope, &kept);
    atomic_write_xbel(xbel_path, out.as_bytes())
}

/// Clear visible local-file entries in `xbel_path`, preserving `<bookmark:private/>`, non-`file://`
/// bookmarks, foreign namespaces, and non-bookmark nodes (`0o600`).
///
/// # Errors
/// Reports filesystem errors.
pub fn clear_at(xbel_path: &Path) -> io::Result<()> {
    let parent = xbel_parent(xbel_path)?;
    if !parent.exists() {
        return Ok(());
    }
    let _guard = acquire_write_lock(xbel_path)?;

    let Some(xml) = read_xbel_string(xbel_path)? else {
        return Ok(());
    };
    let Ok(doc) = Document::parse(&xml) else {
        let out = render_xbel(&DocumentEnvelope::default(), &[]);
        return atomic_write_xbel(xbel_path, out.as_bytes());
    };
    let root = doc.root_element();
    if root.tag_name().name() != "xbel" {
        let out = render_xbel(&DocumentEnvelope::default(), &[]);
        return atomic_write_xbel(xbel_path, out.as_bytes());
    }

    let envelope = DocumentEnvelope::from_root(&xml, root);
    let mut kept = Vec::new();
    let mut removed = false;
    for node in root
        .children()
        .filter(|child| child.is_element() && child.tag_name().name() == "bookmark")
    {
        let is_visible_local = !is_private_bookmark(node)
            && node
                .attribute("href")
                .and_then(parse_local_file_uri)
                .is_some();
        if is_visible_local {
            removed = true;
        } else {
            kept.push(xml[node.range()].trim().to_owned());
        }
    }
    if !removed {
        return Ok(());
    }

    let out = render_xbel(&envelope, &kept);
    atomic_write_xbel(xbel_path, out.as_bytes())
}

struct WriteLockGuard {
    _process: MutexGuard<'static, ()>,
    _file: fs::File,
}

fn xbel_parent(xbel_path: &Path) -> io::Result<&Path> {
    xbel_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "recently-used.xbel path has no parent directory",
        )
    })
}

fn lock_file_path(xbel_path: &Path) -> PathBuf {
    let mut lock_path = xbel_path.as_os_str().to_owned();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

fn open_lock_file(path: &Path) -> io::Result<fs::File> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use rustix::{
            fs::{CWD, Mode, OFlags, openat},
            io::Errno,
        };
        loop {
            match openat(
                CWD,
                path,
                OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => return Ok(fs::File::from(fd)),
                Err(Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        options.open(path)
    }
}

fn acquire_write_lock(xbel_path: &Path) -> io::Result<WriteLockGuard> {
    acquire_write_lock_with_budget(xbel_path, LOCK_RETRY_BUDGET)
}

fn acquire_write_lock_with_budget(
    xbel_path: &Path,
    budget: Duration,
) -> io::Result<WriteLockGuard> {
    let process = WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let lock_path = lock_file_path(xbel_path);
    let file = open_lock_file(&lock_path)?;
    let deadline = Instant::now() + budget;
    loop {
        match file.try_lock() {
            Ok(()) => {
                return Ok(WriteLockGuard {
                    _process: process,
                    _file: file,
                });
            }
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(LOCK_RETRY_INTERVAL);
            }
            Err(fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "timed out waiting for recently-used.xbel write lock",
                ));
            }
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
    }
}

struct DocumentEnvelope {
    opening_tag: String,
    bookmark_prefix: String,
    mime_prefix: String,
    preserved_children: Vec<String>,
}

impl Default for DocumentEnvelope {
    fn default() -> Self {
        Self {
            opening_tag: DEFAULT_XBEL_OPEN_TAG.to_owned(),
            bookmark_prefix: "bookmark".to_owned(),
            mime_prefix: "mime".to_owned(),
            preserved_children: Vec::new(),
        }
    }
}

impl DocumentEnvelope {
    fn from_root(xml: &str, root: Node<'_, '_>) -> Self {
        let root_raw = &xml[root.range()];
        let Some(open_end) = find_tag_end(root_raw) else {
            return Self::default();
        };
        let raw_open = &root_raw[..=open_end];
        let head = raw_open
            .strip_suffix('>')
            .map(str::trim_end)
            .map(|before| before.strip_suffix('/').map_or(before, str::trim_end))
            .unwrap_or("<xbel version=\"1.0\"");

        let mut opening_tag = head.to_owned();
        let bookmark_prefix = ensure_namespace_prefix(
            &mut opening_tag,
            root,
            BOOKMARK_NS,
            "bookmark",
            "gdsbookmark",
        );
        let mime_prefix =
            ensure_namespace_prefix(&mut opening_tag, root, MIME_NS, "mime", "gdsmime");
        opening_tag.push('>');

        let preserved_children = root
            .children()
            .filter(|child| child.is_element() && child.tag_name().name() != "bookmark")
            .map(|child| xml[child.range()].trim().to_owned())
            .collect();

        Self {
            opening_tag,
            bookmark_prefix,
            mime_prefix,
            preserved_children,
        }
    }
}

fn ensure_namespace_prefix(
    opening_tag: &mut String,
    root: Node<'_, '_>,
    target_uri: &str,
    preferred: &str,
    fallback: &str,
) -> String {
    if let Some(existing) = root
        .namespaces()
        .find(|ns| ns.uri() == target_uri && ns.name().is_some_and(|name| !name.is_empty()))
        .and_then(|ns| ns.name())
    {
        return existing.to_owned();
    }
    let prefix = if root.namespaces().any(|ns| ns.name() == Some(preferred)) {
        fallback
    } else {
        preferred
    };
    opening_tag.push_str("\n      xmlns:");
    opening_tag.push_str(prefix);
    opening_tag.push_str("=\"");
    opening_tag.push_str(target_uri);
    opening_tag.push('"');
    prefix.to_owned()
}

fn render_xbel(envelope: &DocumentEnvelope, bookmarks: &[String]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&envelope.opening_tag);
    out.push('\n');
    for child in &envelope.preserved_children {
        out.push_str("  ");
        out.push_str(child);
        out.push('\n');
    }
    for raw in bookmarks {
        out.push_str("  ");
        out.push_str(raw.trim());
        out.push('\n');
    }
    out.push_str("</xbel>\n");
    out
}

fn atomic_write_xbel(xbel_path: &Path, content: &[u8]) -> io::Result<()> {
    use std::io::Write as _;

    let parent = xbel_parent(xbel_path)?;
    fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temp.write_all(content)?;
    temp.as_file().sync_data()?;
    temp.persist(xbel_path).map_err(|error| error.error)?;
    Ok(())
}

fn format_application_tag(
    bookmark_prefix: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
    count: u64,
) -> String {
    format!(
        "<{bm}:application name=\"{app}\" exec=\"{exec}\" modified=\"{ts}\" count=\"{count}\"/>",
        bm = bookmark_prefix,
        app = escape_xml_attr(app_name),
        exec = escape_xml_attr(exec),
        ts = escape_xml_attr(timestamp),
    )
}

fn format_applications_block(
    bookmark_prefix: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> String {
    let app = format_application_tag(bookmark_prefix, app_name, exec, timestamp, 1);
    format!(
        "<{bm}:applications>\n          {app}\n        </{bm}:applications>",
        bm = bookmark_prefix
    )
}

fn format_metadata_block(
    envelope: &DocumentEnvelope,
    mime: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> String {
    let apps = format_applications_block(&envelope.bookmark_prefix, app_name, exec, timestamp);
    format!(
        concat!(
            "<metadata owner=\"{owner}\">\n",
            "        <{mime_pfx}:mime-type type=\"{mime}\"/>\n",
            "        {apps}\n",
            "      </metadata>",
        ),
        owner = FREEDESKTOP_OWNER,
        mime_pfx = envelope.mime_prefix,
        mime = escape_xml_attr(mime),
        apps = apps,
    )
}

fn format_info_block(
    envelope: &DocumentEnvelope,
    mime: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> String {
    let metadata = format_metadata_block(envelope, mime, app_name, exec, timestamp);
    format!("<info>\n      {metadata}\n    </info>")
}

fn format_new_bookmark(
    envelope: &DocumentEnvelope,
    uri: &str,
    mime: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> String {
    let info = format_info_block(envelope, mime, app_name, exec, timestamp);
    format!(
        "<bookmark href=\"{uri}\" added=\"{ts}\" modified=\"{ts}\" visited=\"{ts}\">\n    {info}\n  </bookmark>",
        uri = escape_xml_attr(uri),
        ts = escape_xml_attr(timestamp),
    )
}

fn update_existing_bookmark(
    xml: &str,
    bookmark: Node<'_, '_>,
    envelope: &DocumentEnvelope,
    mime: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> String {
    let range = bookmark.range();
    let raw = &xml[range.clone()];
    let Some(open_end_rel) = find_tag_end(raw) else {
        let href = bookmark.attribute("href").unwrap_or_default();
        return format_new_bookmark(envelope, href, mime, app_name, exec, timestamp);
    };

    // Rebuild the opening <bookmark ...> tag keeping all existing attributes while updating
    // `modified` and `visited`.
    let mut opening = String::from("<bookmark");
    let mut saw_added = false;
    for attr in bookmark.attributes() {
        let name = attr.name();
        if name == "modified" || name == "visited" {
            continue;
        }
        if name == "added" {
            saw_added = true;
        }
        opening.push(' ');
        if let Some(ns) = attr.namespace()
            && let Some(prefix) = bookmark.lookup_prefix(ns)
        {
            opening.push_str(prefix);
            opening.push(':');
        }
        opening.push_str(name);
        opening.push_str("=\"");
        opening.push_str(&escape_xml_attr(attr.value()));
        opening.push('"');
    }
    if !saw_added {
        opening.push_str(" added=\"");
        opening.push_str(&escape_xml_attr(timestamp));
        opening.push('"');
    }
    opening.push_str(" modified=\"");
    opening.push_str(&escape_xml_attr(timestamp));
    opening.push_str("\" visited=\"");
    opening.push_str(&escape_xml_attr(timestamp));
    opening.push_str("\">");

    if is_self_closing_tag(&raw[..=open_end_rel]) {
        let info = format_info_block(envelope, mime, app_name, exec, timestamp);
        return format!("{opening}\n    {info}\n  </bookmark>");
    }

    let mut inner = raw[open_end_rel + 1..].to_owned();
    let base_offset = range.start + open_end_rel + 1;
    let bm = &envelope.bookmark_prefix;

    // Update or insert our `<bookmark:application>` entry while preserving all other
    // applications, groups, icons, and foreign metadata verbatim.
    if let Some(apps_node) = find_bookmark_applications(bookmark) {
        let existing_app = apps_node.children().find(|child| {
            child.is_element()
                && child.tag_name().name() == "application"
                && child.tag_name().namespace() == Some(BOOKMARK_NS)
                && child.attribute("name") == Some(app_name)
        });
        if let Some(app_node) = existing_app {
            let count = app_node
                .attribute("count")
                .and_then(|c| c.parse::<u64>().ok())
                .unwrap_or(0)
                .saturating_add(1);
            let replacement = format_application_tag(bm, app_name, exec, timestamp, count);
            let rel_start = app_node.range().start.saturating_sub(base_offset);
            let rel_end = app_node.range().end.saturating_sub(base_offset);
            if rel_start <= rel_end && rel_end <= inner.len() {
                inner.replace_range(rel_start..rel_end, &replacement);
            }
        } else {
            let app_tag = format_application_tag(bm, app_name, exec, timestamp, 1);
            insert_child_xml(
                &mut inner,
                base_offset,
                apps_node,
                &format!("  {app_tag}\n        "),
            );
        }
    } else if let Some(meta_node) = find_freedesktop_metadata(bookmark) {
        let mut snippet = String::new();
        if bookmark_mime_type(bookmark).is_none() {
            snippet.push_str(&format!(
                "  <{}:mime-type type=\"{}\"/>\n      ",
                envelope.mime_prefix,
                escape_xml_attr(mime)
            ));
        }
        snippet.push_str(&format!(
            "  {}\n      ",
            format_applications_block(bm, app_name, exec, timestamp)
        ));
        insert_child_xml(&mut inner, base_offset, meta_node, &snippet);
    } else if let Some(info_node) = find_info(bookmark) {
        let metadata = format_metadata_block(envelope, mime, app_name, exec, timestamp);
        insert_child_xml(
            &mut inner,
            base_offset,
            info_node,
            &format!("  {metadata}\n    "),
        );
    } else if let Some(close_pos) = inner.rfind("</") {
        let info = format_info_block(envelope, mime, app_name, exec, timestamp);
        inner.insert_str(close_pos, &format!("  {info}\n  "));
    }

    format!("{opening}{inner}")
}

fn is_self_closing_tag(opening_tag: &str) -> bool {
    opening_tag
        .strip_suffix('>')
        .is_some_and(|before| before.trim_end().ends_with('/'))
}

fn insert_child_xml(inner: &mut String, base_offset: usize, parent: Node<'_, '_>, snippet: &str) {
    let rel_start = parent.range().start.saturating_sub(base_offset);
    let rel_end = parent.range().end.saturating_sub(base_offset);
    if rel_start > rel_end || rel_end > inner.len() {
        return;
    }
    let slice = &inner[rel_start..rel_end];
    let Some(open_end) = find_tag_end(slice) else {
        return;
    };
    let open_slice = &slice[..=open_end];
    if is_self_closing_tag(open_slice) {
        let head = open_slice
            .strip_suffix('>')
            .map(str::trim_end)
            .and_then(|before| before.strip_suffix('/'))
            .map(str::trim_end);
        let Some(head) = head else {
            return;
        };
        let Some(tag_name) = head
            .strip_prefix('<')
            .and_then(|rest| rest.split_whitespace().next())
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        let expanded = format!("{head}>\n      {snippet}</{tag_name}>");
        inner.replace_range(rel_start..rel_end, &expanded);
    } else if let Some(close_rel) = slice.rfind("</") {
        inner.insert_str(rel_start + close_rel, snippet);
    }
}

fn find_tag_end(raw: &str) -> Option<usize> {
    let mut in_quote: Option<u8> = None;
    for (idx, byte) in raw.bytes().enumerate() {
        match (in_quote, byte) {
            (None, b'"' | b'\'') => in_quote = Some(byte),
            (Some(q), b) if q == b => in_quote = None,
            (None, b'>') => return Some(idx),
            _ => {}
        }
    }
    None
}

fn find_info<'a, 'd>(bookmark: Node<'a, 'd>) -> Option<Node<'a, 'd>> {
    bookmark
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "info")
}

fn find_freedesktop_metadata<'a, 'd>(bookmark: Node<'a, 'd>) -> Option<Node<'a, 'd>> {
    let info = find_info(bookmark)?;
    info.children().find(|child| {
        child.is_element()
            && child.tag_name().name() == "metadata"
            && child.attribute("owner") == Some(FREEDESKTOP_OWNER)
    })
}

fn find_bookmark_applications<'a, 'd>(bookmark: Node<'a, 'd>) -> Option<Node<'a, 'd>> {
    let metadata = find_freedesktop_metadata(bookmark)?;
    metadata.children().find(|child| {
        child.is_element()
            && child.tag_name().name() == "applications"
            && child.tag_name().namespace() == Some(BOOKMARK_NS)
    })
}

fn is_private_bookmark(bookmark: Node<'_, '_>) -> bool {
    find_freedesktop_metadata(bookmark).is_some_and(|metadata| {
        metadata.children().any(|child| {
            child.is_element()
                && child.tag_name().name() == "private"
                && child.tag_name().namespace() == Some(BOOKMARK_NS)
        })
    })
}

fn bookmark_mime_type<'a>(bookmark: Node<'a, '_>) -> Option<&'a str> {
    let metadata = find_freedesktop_metadata(bookmark)?;
    metadata
        .children()
        .find(|child| {
            child.is_element()
                && child.tag_name().name() == "mime-type"
                && child.tag_name().namespace() == Some(MIME_NS)
        })
        .and_then(|node| node.attribute("type"))
}

fn parse_timestamp(raw: &str) -> Option<SystemTime> {
    DateTime::parse_from_rfc3339(raw).ok().map(SystemTime::from)
}

fn escape_xml_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// Encode an absolute local filesystem path as a `file://` URI preserving raw Unix bytes.
#[must_use]
pub fn encode_local_file_uri(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let encoded = percent_encode(path.as_os_str().as_bytes(), FILE_URI_ENCODE_SET);
        format!("file://{encoded}")
    }
    #[cfg(not(unix))]
    {
        url::Url::from_file_path(path)
            .map(|url| url.to_string())
            .unwrap_or_else(|()| format!("file://{}", path.to_string_lossy()))
    }
}

/// Cached local system hostname used for RFC 8089 `file://<hostname>/...` URI matching.
#[must_use]
pub fn local_hostname() -> Option<&'static str> {
    LOCAL_HOSTNAME.get_or_init(detect_local_hostname).as_deref()
}

fn detect_local_hostname() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(raw) = fs::read_to_string("/proc/sys/kernel/hostname") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    std::env::var("HOSTNAME")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// Return whether `host` refers to the local machine per RFC 8089 §2
/// (empty authority, `localhost`, or the current host's name, ASCII case-insensitively).
#[must_use]
pub fn is_local_file_host(host: &str) -> bool {
    is_local_file_host_with(host, local_hostname())
}

/// Return whether `host` refers to the local machine given an explicit `local_hostname`.
#[must_use]
pub fn is_local_file_host_with(host: &str, local_hostname: Option<&str>) -> bool {
    host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || local_hostname
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .is_some_and(|name| host.eq_ignore_ascii_case(name))
}

/// Parse a strict RFC 8089 local `file:` URL (rejecting foreign hosts, userinfo, ports,
/// queries, fragments, relative `file:path` forms, and embedded NUL bytes) while preserving
/// raw Unix path bytes.
#[must_use]
pub fn parse_strict_local_file_url(uri: &str) -> Option<PathBuf> {
    parse_strict_local_file_url_with_host(uri, local_hostname())
}

/// Parse a strict RFC 8089 local `file:` URL against an explicit `local_hostname`, preserving
/// raw Unix path bytes.
#[must_use]
pub fn parse_strict_local_file_url_with_host(
    uri: &str,
    local_hostname: Option<&str>,
) -> Option<PathBuf> {
    let parsed = url::Url::parse(uri).ok()?;
    let absolute = uri
        .split_once(':')
        .is_some_and(|(_, rest)| rest.starts_with('/'));
    if uri.as_bytes().contains(&0)
        || !absolute
        || parsed.scheme() != "file"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed
            .host_str()
            .is_some_and(|host| !is_local_file_host_with(host, local_hostname))
    {
        return None;
    }
    // `url::Url::to_file_path()` rejects non-`localhost` hostnames on Unix; decode the validated
    // URL path directly on Unix (preserving raw bytes) and normalize the host on other platforms.
    #[cfg(unix)]
    {
        decode_local_file_path(parsed.path())
    }
    #[cfg(not(unix))]
    {
        let normalized = if parsed.host_str().is_some() {
            url::Url::parse(&format!("file://{}", parsed.path())).ok()?
        } else {
            parsed
        };
        let path = normalized.to_file_path().ok()?;
        if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
            return None;
        }
        Some(path)
    }
}

fn decode_local_file_path(encoded_path: &str) -> Option<PathBuf> {
    let decoded: Vec<u8> = percent_decode_str(encoded_path).collect();
    if decoded.is_empty() || decoded.contains(&0) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        let path = PathBuf::from(OsString::from_vec(decoded));
        path.is_absolute().then_some(path)
    }
    #[cfg(not(unix))]
    {
        let text = String::from_utf8(decoded).ok()?;
        let path = PathBuf::from(text);
        path.is_absolute().then_some(path)
    }
}

/// Parse a local `file://` URI (empty host, `localhost`, or the system's local hostname)
/// preserving raw Unix bytes.
#[must_use]
pub fn parse_local_file_uri(uri: &str) -> Option<PathBuf> {
    parse_local_file_uri_with_host(uri, local_hostname())
}

/// Parse a local `file://` URI against an explicit `local_hostname`, preserving raw Unix bytes.
#[must_use]
pub fn parse_local_file_uri_with_host(uri: &str, local_hostname: Option<&str>) -> Option<PathBuf> {
    let (scheme, rest) = uri.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let (authority, encoded_path) = if rest.starts_with('/') {
        ("", rest)
    } else {
        let slash = rest.find('/')?;
        (&rest[..slash], &rest[slash..])
    };
    if !is_local_file_host_with(authority, local_hostname) {
        return None;
    }
    let path_without_query = encoded_path
        .split(['?', '#'])
        .next()
        .unwrap_or(encoded_path);
    decode_local_file_path(path_without_query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sorts_filters_private_and_preserves_raw_unix_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xbel = dir.path().join("recently-used.xbel");
        let sample = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0"
      xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks"
      xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  <bookmark href="file:///tmp/older.txt" added="2026-11-20T10:00:00Z" modified="2026-11-20T10:00:00Z" visited="2026-11-20T10:00:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="text/plain"/>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="file:///tmp/secret.txt" added="2026-11-24T12:00:00Z" modified="2026-11-24T12:00:00Z" visited="2026-11-24T12:00:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="text/plain"/>
        <bookmark:private/>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="https://example.org/ignored" added="2026-11-24T13:00:00Z" modified="2026-11-24T13:00:00Z" visited="2026-11-24T13:00:00Z"/>
  <bookmark href="file:///tmp/report%20%231.pdf" added="2026-11-24T10:00:00Z" modified="2026-11-24T11:00:00Z" visited="2026-11-24T11:30:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="application/pdf"/>
      </metadata>
    </info>
  </bookmark>
</xbel>
"#;
        fs::write(&xbel, sample).expect("write sample xbel");

        let entries = read_from(&xbel).expect("read entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, PathBuf::from("/tmp/report #1.pdf"));
        assert_eq!(entries[0].mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(entries[1].path, PathBuf::from("/tmp/older.txt"));
    }

    #[test]
    fn record_updates_entry_losslessly_preserving_foreign_apps_groups_and_private_items() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xbel = dir.path().join("recently-used.xbel");
        let sample = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0"
      xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks"
      xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  <bookmark href="file:///tmp/doc.md" added="2026-11-20T09:00:00Z" modified="2026-11-20T09:00:00Z" visited="2026-11-20T09:00:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="text/markdown"/>
        <bookmark:groups>
          <bookmark:group>Editors</bookmark:group>
        </bookmark:groups>
        <bookmark:applications>
          <bookmark:application name="Firefox" exec="&apos;firefox&apos; %u" modified="2026-11-20T09:00:00Z" count="3"/>
        </bookmark:applications>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="file:///tmp/private.key" added="2026-11-21T09:00:00Z" modified="2026-11-21T09:00:00Z" visited="2026-11-21T09:00:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <bookmark:private/>
      </metadata>
    </info>
  </bookmark>
</xbel>
"#;
        fs::write(&xbel, sample).expect("write initial xbel");

        record_at_timestamp(
            &xbel,
            Path::new("/tmp/doc.md"),
            Some("text/markdown"),
            "GDS Files",
            "'gds-files' %u",
            "2026-11-24T12:00:00Z",
        )
        .expect("record existing doc");
        record_at_timestamp(
            &xbel,
            Path::new("/tmp/doc.md"),
            Some("text/markdown"),
            "GDS Files",
            "'gds-files' %u",
            "2026-11-24T12:05:00Z",
        )
        .expect("increment count");
        record_at_timestamp(
            &xbel,
            Path::new("/tmp/brand new.png"),
            Some("image/png"),
            "GDS Files",
            "'gds-files' %u",
            "2026-11-24T12:10:00Z",
        )
        .expect("record new file");

        let raw = fs::read_to_string(&xbel).expect("read updated xbel");
        assert!(raw.contains("<bookmark:group>Editors</bookmark:group>"));
        assert!(raw.contains("name=\"Firefox\""));
        assert!(raw.contains("count=\"2\""));
        assert!(raw.contains("file:///tmp/private.key"));
        assert!(raw.contains("added=\"2026-11-20T09:00:00Z\""));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&xbel).expect("xbel meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let entries = read_from(&xbel).expect("read back");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, PathBuf::from("/tmp/brand new.png"));
        assert_eq!(entries[1].path, PathBuf::from("/tmp/doc.md"));

        remove_at(&xbel, Path::new("/tmp/doc.md")).expect("remove doc.md");
        let after_remove = read_from(&xbel).expect("read after remove");
        assert_eq!(after_remove.len(), 1);
        assert_eq!(after_remove[0].path, PathBuf::from("/tmp/brand new.png"));

        clear_at(&xbel).expect("clear xbel");
        assert!(read_from(&xbel).expect("empty after clear").is_empty());
        let after_clear = fs::read_to_string(&xbel).expect("read after clear");
        assert!(
            after_clear.contains("file:///tmp/private.key"),
            "clear_at must preserve private bookmarks"
        );
    }

    #[test]
    fn updates_self_closing_and_incomplete_bookmarks_preserving_foreign_namespaces_and_nodes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xbel = dir.path().join("recently-used.xbel");
        let sample = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0"
      xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks"
      xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info"
      xmlns:custom="https://example.org/custom">
  <title>Desktop Recent Files</title>
  <bookmark href="file:///tmp/self-closing.txt" added="2026-11-20T09:00:00Z" modified="2026-11-20T09:00:00Z" visited="2026-11-20T09:00:00Z"/>
  <bookmark href="file:///tmp/no-apps.txt" added="2026-11-20T10:00:00Z" modified="2026-11-20T10:00:00Z" visited="2026-11-20T10:00:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="text/plain"/>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="file:///tmp/empty-apps.txt" added="2026-11-20T11:00:00Z" modified="2026-11-20T11:00:00Z" visited="2026-11-20T11:00:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <bookmark:groups><bookmark:group>Work</bookmark:group></bookmark:groups>
        <bookmark:applications/>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="https://example.org/remote" added="2026-11-20T12:00:00Z" modified="2026-11-20T12:00:00Z" visited="2026-11-20T12:00:00Z">
    <info>
      <metadata owner="https://example.org/custom">
        <custom:tag>keep</custom:tag>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="file:///tmp/secret.key" added="2026-11-20T13:00:00Z" modified="2026-11-20T13:00:00Z" visited="2026-11-20T13:00:00Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <bookmark:private/>
      </metadata>
    </info>
  </bookmark>
</xbel>
"#;
        fs::write(&xbel, sample).expect("write fixture");

        for (file, mime, ts) in [
            (
                "/tmp/self-closing.txt",
                "text/plain",
                "2026-11-24T12:00:00Z",
            ),
            ("/tmp/no-apps.txt", "text/plain", "2026-11-24T12:01:00Z"),
            ("/tmp/empty-apps.txt", "text/plain", "2026-11-24T12:02:00Z"),
        ] {
            record_at_timestamp(
                &xbel,
                Path::new(file),
                Some(mime),
                "GDS Files",
                "'gds-files' %u",
                ts,
            )
            .expect("record bookmark");
        }

        let raw = fs::read_to_string(&xbel).expect("read updated xbel");
        let doc = Document::parse(&raw).expect("updated XML must remain well-formed");
        assert!(raw.contains("xmlns:custom=\"https://example.org/custom\""));
        assert!(raw.contains("<title>Desktop Recent Files</title>"));
        assert!(raw.contains("<custom:tag>keep</custom:tag>"));
        assert!(raw.contains("<bookmark:group>Work</bookmark:group>"));

        let entries = read_from(&xbel).expect("parse visible entries");
        assert_eq!(entries.len(), 3);
        let app_nodes = doc
            .descendants()
            .filter(|node| {
                node.is_element()
                    && node.tag_name().name() == "application"
                    && node.tag_name().namespace() == Some(BOOKMARK_NS)
                    && node.attribute("name") == Some("GDS Files")
            })
            .count();
        assert_eq!(app_nodes, 3);

        clear_at(&xbel).expect("clear visible local entries");
        assert!(
            read_from(&xbel)
                .expect("no visible local entries")
                .is_empty()
        );
        let cleared = fs::read_to_string(&xbel).expect("read cleared xbel");
        Document::parse(&cleared).expect("cleared XML must remain well-formed");
        assert!(cleared.contains("https://example.org/remote"));
        assert!(cleared.contains("file:///tmp/secret.key"));
        assert!(cleared.contains("<title>Desktop Recent Files</title>"));
        assert!(cleared.contains("xmlns:custom=\"https://example.org/custom\""));
    }

    #[test]
    fn write_lock_blocks_when_external_holder_locks_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xbel = dir.path().join("recently-used.xbel");
        let lock_file = open_lock_file(&lock_file_path(&xbel)).expect("open lock file");
        lock_file.try_lock().expect("acquire external lock");

        let blocked = acquire_write_lock_with_budget(&xbel, Duration::ZERO);
        assert!(
            matches!(blocked, Err(ref err) if err.kind() == io::ErrorKind::WouldBlock),
            "write lock must fail with WouldBlock while another holder owns the flock"
        );

        drop(lock_file);
        record_at_timestamp(
            &xbel,
            Path::new("/tmp/locked.txt"),
            Some("text/plain"),
            "GDS Files",
            "'gds-files' %u",
            "2026-11-24T12:00:00Z",
        )
        .expect("writer completes once lock releases");

        let entries = read_from(&xbel).expect("read written entry");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("/tmp/locked.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_unix_path_round_trips_through_file_uri() {
        use std::os::unix::ffi::OsStringExt as _;

        let raw = PathBuf::from(OsString::from_vec(b"/tmp/raw-\xff-name.bin".to_vec()));
        let uri = encode_local_file_uri(&raw);
        assert_eq!(uri, "file:///tmp/raw-%FF-name.bin");
        assert_eq!(parse_local_file_uri(&uri), Some(raw.clone()));
        assert_eq!(
            parse_local_file_uri_with_host(
                "file://workstation/tmp/raw-%FF-name.bin",
                Some("Workstation"),
            ),
            Some(raw),
        );
    }

    #[test]
    fn parse_local_file_uri_matches_rfc8089_local_hosts_only() {
        assert_eq!(
            parse_local_file_uri_with_host("file:///tmp/doc.txt", Some("workstation")),
            Some(PathBuf::from("/tmp/doc.txt"))
        );
        assert_eq!(
            parse_local_file_uri_with_host("file://localhost/tmp/doc.txt", Some("workstation")),
            Some(PathBuf::from("/tmp/doc.txt"))
        );
        assert_eq!(
            parse_local_file_uri_with_host("file://LOCALHOST/tmp/doc.txt", None),
            Some(PathBuf::from("/tmp/doc.txt"))
        );
        assert_eq!(
            parse_local_file_uri_with_host("file://workstation/tmp/doc.txt", Some("workstation")),
            Some(PathBuf::from("/tmp/doc.txt"))
        );
        assert_eq!(
            parse_local_file_uri_with_host("file://WORKSTATION/tmp/doc.txt", Some("workstation")),
            Some(PathBuf::from("/tmp/doc.txt"))
        );
        assert_eq!(
            parse_local_file_uri_with_host("file://other-host/tmp/doc.txt", Some("workstation")),
            None
        );
        assert_eq!(
            parse_local_file_uri_with_host(
                "file://user@workstation/tmp/doc.txt",
                Some("workstation")
            ),
            None
        );
        assert_eq!(
            parse_local_file_uri_with_host(
                "file://workstation:8080/tmp/doc.txt",
                Some("workstation")
            ),
            None
        );
        assert_eq!(
            parse_local_file_uri_with_host(
                "file://workstation/tmp/bad%00name",
                Some("workstation")
            ),
            None
        );
        assert_eq!(
            parse_local_file_uri_with_host("smb://workstation/share", Some("workstation")),
            None
        );
        assert!(is_local_file_host_with("", Some("workstation")));
        assert!(is_local_file_host_with("LocalHost", None));
        assert!(is_local_file_host_with("WorkStation", Some("workstation")));
        assert!(!is_local_file_host_with("other-host", Some("workstation")));

        assert_eq!(
            parse_strict_local_file_url_with_host(
                "file://WORKSTATION/tmp/doc.txt",
                Some("workstation")
            ),
            Some(PathBuf::from("/tmp/doc.txt"))
        );
        assert_eq!(
            parse_strict_local_file_url_with_host(
                "file://other-host/tmp/doc.txt",
                Some("workstation")
            ),
            None
        );
        assert_eq!(
            parse_strict_local_file_url_with_host(
                "file://workstation/tmp/doc.txt?q=1",
                Some("workstation")
            ),
            None
        );
        assert_eq!(
            parse_strict_local_file_url_with_host(
                "file://workstation/tmp/doc.txt#frag",
                Some("workstation")
            ),
            None
        );
        assert_eq!(
            parse_strict_local_file_url_with_host("file:relative/doc.txt", Some("workstation")),
            None
        );
    }
}
