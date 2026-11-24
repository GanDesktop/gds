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
    sync::Mutex,
    time::SystemTime,
};

use chrono::{DateTime, SecondsFormat, Utc};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, percent_encode};
use roxmltree::{Document, Node};

const BOOKMARK_NS: &str = "http://www.freedesktop.org/standards/desktop-bookmarks";
const MIME_NS: &str = "http://www.freedesktop.org/standards/shared-mime-info";
const FREEDESKTOP_OWNER: &str = "http://freedesktop.org";
const MAX_XBEL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_BOOKMARKS: usize = 1000;

const XBEL_HEADER: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<xbel version=\"1.0\"\n",
    "      xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n",
    "      xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\"\n",
    ">\n",
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
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let target_uri = encode_local_file_uri(file_path);
    let mime = mime_type
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("application/octet-stream");

    let existing_xml = read_xbel_string(xbel_path)?;
    let mut bookmarks: Vec<(Option<SystemTime>, String)> = Vec::new();
    let mut updated_existing = None;

    if let Some(ref xml) = existing_xml
        && let Ok(doc) = Document::parse(xml)
        && doc.root_element().tag_name().name() == "xbel"
    {
        for node in doc
            .root_element()
            .children()
            .filter(|child| child.is_element() && child.tag_name().name() == "bookmark")
        {
            let raw = &xml[node.range()];
            let is_match = node.attribute("href").is_some_and(|href| {
                href == target_uri || parse_local_file_uri(href).as_deref() == Some(file_path)
            });
            if is_match && updated_existing.is_none() {
                updated_existing = Some(update_existing_bookmark(
                    xml, node, mime, app_name, exec, timestamp,
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

    let entry_xml = updated_existing
        .unwrap_or_else(|| format_new_bookmark(&target_uri, mime, app_name, exec, timestamp));
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

    let mut out = String::from(XBEL_HEADER);
    for (_, raw) in bookmarks {
        out.push_str("  ");
        out.push_str(raw.trim());
        out.push('\n');
    }
    out.push_str("</xbel>\n");

    atomic_write_xbel(xbel_path, out.as_bytes())
}

/// Remove any local-file bookmark matching `file_path` from `xbel_path`, preserving all other
/// entries verbatim.
///
/// # Errors
/// Reports XML or filesystem failures.
pub fn remove_at(xbel_path: &Path, file_path: &Path) -> io::Result<()> {
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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

    let mut out = String::from(XBEL_HEADER);
    for raw in kept {
        out.push_str("  ");
        out.push_str(&raw);
        out.push('\n');
    }
    out.push_str("</xbel>\n");
    atomic_write_xbel(xbel_path, out.as_bytes())
}

/// Atomically replace `xbel_path` with an empty valid XBEL 1.0 document (`0o600`).
///
/// # Errors
/// Reports filesystem errors.
pub fn clear_at(xbel_path: &Path) -> io::Result<()> {
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = String::from(XBEL_HEADER);
    out.push_str("</xbel>\n");
    atomic_write_xbel(xbel_path, out.as_bytes())
}

fn atomic_write_xbel(xbel_path: &Path, content: &[u8]) -> io::Result<()> {
    use std::io::Write as _;

    let parent = xbel_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "recently-used.xbel path has no parent directory",
        )
    })?;
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

fn format_new_bookmark(
    uri: &str,
    mime: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> String {
    format!(
        concat!(
            "<bookmark href=\"{uri}\" added=\"{ts}\" modified=\"{ts}\" visited=\"{ts}\">\n",
            "    <info>\n",
            "      <metadata owner=\"http://freedesktop.org\">\n",
            "        <mime:mime-type type=\"{mime}\"/>\n",
            "        <bookmark:applications>\n",
            "          <bookmark:application name=\"{app}\" exec=\"{exec}\" modified=\"{ts}\" count=\"1\"/>\n",
            "        </bookmark:applications>\n",
            "      </metadata>\n",
            "    </info>\n",
            "  </bookmark>",
        ),
        uri = escape_xml_attr(uri),
        ts = escape_xml_attr(timestamp),
        mime = escape_xml_attr(mime),
        app = escape_xml_attr(app_name),
        exec = escape_xml_attr(exec),
    )
}

fn update_existing_bookmark(
    xml: &str,
    bookmark: Node<'_, '_>,
    mime: &str,
    app_name: &str,
    exec: &str,
    timestamp: &str,
) -> String {
    let range = bookmark.range();
    let raw = &xml[range.clone()];
    let Some(open_end_rel) = find_tag_end(raw) else {
        let href = bookmark.attribute("href").unwrap_or_default();
        return format_new_bookmark(href, mime, app_name, exec, timestamp);
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

    let mut inner = raw[open_end_rel + 1..].to_owned();
    let base_offset = range.start + open_end_rel + 1;

    // Update or insert our `<bookmark:application>` entry inside `<bookmark:applications>`,
    // leaving all other applications, groups, icons, and metadata untouched.
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
            let replacement = format!(
                "<bookmark:application name=\"{}\" exec=\"{}\" modified=\"{}\" count=\"{count}\"/>",
                escape_xml_attr(app_name),
                escape_xml_attr(exec),
                escape_xml_attr(timestamp),
            );
            let rel_start = app_node.range().start.saturating_sub(base_offset);
            let rel_end = app_node.range().end.saturating_sub(base_offset);
            if rel_start <= rel_end && rel_end <= inner.len() {
                inner.replace_range(rel_start..rel_end, &replacement);
            }
        } else {
            let apps_rel_end = apps_node.range().end.saturating_sub(base_offset);
            if apps_rel_end <= inner.len()
                && let Some(close_tag_pos) = inner[..apps_rel_end].rfind("</")
            {
                let insertion = format!(
                    "  <bookmark:application name=\"{}\" exec=\"{}\" modified=\"{}\" count=\"1\"/>\n        ",
                    escape_xml_attr(app_name),
                    escape_xml_attr(exec),
                    escape_xml_attr(timestamp),
                );
                inner.insert_str(close_tag_pos, &insertion);
            }
        }
    }

    format!("{opening}{inner}")
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

fn find_freedesktop_metadata<'a, 'd>(bookmark: Node<'a, 'd>) -> Option<Node<'a, 'd>> {
    let info = bookmark
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "info")?;
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

/// Parse a local `file://` URI (empty or `localhost` host) preserving raw Unix bytes.
#[must_use]
pub fn parse_local_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let (authority, encoded_path) = if let Some(after_slash) = rest.strip_prefix('/') {
        ("", format!("/{after_slash}"))
    } else {
        let slash = rest.find('/')?;
        (&rest[..slash], rest[slash..].to_owned())
    };
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let path_without_query = encoded_path
        .split(['?', '#'])
        .next()
        .unwrap_or(&encoded_path);
    let decoded: Vec<u8> = percent_decode_str(path_without_query).collect();
    if decoded.is_empty() || decoded.contains(&0) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        Some(PathBuf::from(OsString::from_vec(decoded)))
    }
    #[cfg(not(unix))]
    {
        let text = String::from_utf8(decoded).ok()?;
        Some(PathBuf::from(text))
    }
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
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_unix_path_round_trips_through_file_uri() {
        use std::os::unix::ffi::OsStringExt as _;

        let raw = PathBuf::from(OsString::from_vec(b"/tmp/raw-\xff-name.bin".to_vec()));
        let uri = encode_local_file_uri(&raw);
        assert_eq!(uri, "file:///tmp/raw-%FF-name.bin");
        assert_eq!(parse_local_file_uri(&uri), Some(raw));
    }
}
