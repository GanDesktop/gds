//! File and directory selection modes for `gds-files`.
//!
//! The picker turns the browser into a modal file chooser: the user picks one
//! file, and the chosen path is reported as one JSON line on stdout. Filter
//! groups narrow the file list without blocking directory navigation, and the
//! matcher works on the raw basename so non-UTF-8 names still match.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use gds::service::desktop::protocol::chooser::{self, Mode, Pattern};
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Serialize;
use thiserror::Error;

/// Exit status reported after the window closes: the user confirmed a file.
const EXIT_CONFIRMED: u8 = 0;
/// Exit status for cancellation, window close and startup failures.
const EXIT_CANCELLED: u8 = 1;
/// Exit status when the selection could not be written to stdout.
const EXIT_FAILED: u8 = 2;

/// Outcome of one picker session, shared with the process exit path.
static EXIT: AtomicU8 = AtomicU8::new(EXIT_CANCELLED);

/// Picker behaviour handed to the browser.
#[derive(Clone, Debug)]
pub struct Options {
    /// Selectable filter groups; never empty.
    pub filters: Vec<Filter>,
    /// Initial read-only flag returned to the caller; it does not change file permissions.
    pub read_only: bool,
    /// Select directories instead of regular files.
    pub directory: bool,
    /// Allow multiple selections of the requested type.
    pub multiple: bool,
    pub portal: Option<chooser::Request>,
}

impl Options {
    /// Filter groups always contain at least the default "all files" group.
    fn normalize_filters(filters: Vec<Filter>) -> Vec<Filter> {
        if filters.is_empty() {
            vec![Filter::all_files()]
        } else {
            filters
        }
    }

    /// Builds options, adding the default "all files" group when `filters` is
    /// empty so the invariant on [`Options::filters`] holds.
    pub fn new(filters: Vec<Filter>, read_only: bool) -> Self {
        Self {
            filters: Self::normalize_filters(filters),
            read_only,
            directory: false,
            multiple: false,
            portal: None,
        }
    }

    /// # Errors
    /// Reports invalid filename filters or save names.
    pub fn from_request(request: chooser::Request) -> Result<Self, Error> {
        let mut filters = request
            .filters
            .iter()
            .map(Filter::from_portal)
            .collect::<Result<Vec<_>, _>>()?;
        if filters.is_empty() {
            filters.push(Filter::all_files());
        }
        let mut directory = false;
        let mut multiple = false;
        match &request.mode {
            Mode::Open {
                directory: dir,
                multiple: multi,
            } => {
                directory = *dir;
                multiple = *multi;
            }
            Mode::Save { .. } => {}
            Mode::SaveFiles { names } => {
                if names.is_empty() || names.iter().any(|name| !Self::valid_name(name)) {
                    return Err(Error::InvalidSaveName);
                }
                directory = true;
            }
        }
        Ok(Self {
            filters,
            read_only: false,
            directory,
            multiple,
            portal: Some(request),
        })
    }

    pub fn valid_name(name: &std::ffi::OsStr) -> bool {
        crate::features::file_operations::names::validate(name).is_ok()
    }

    pub fn is_save(&self) -> bool {
        self.portal
            .as_ref()
            .is_some_and(|request| !matches!(request.mode, Mode::Open { .. }))
    }

    pub fn matches_type(&self, filter_index: usize, path: &Path, mime: Option<&str>) -> bool {
        self.filters
            .get(filter_index)
            .is_some_and(|filter| filter.is_match_type(path, mime))
    }

    /// Whether `path` is visible under the filter group at `filter_index`.
    ///
    /// Only the basename is matched; directories stay navigable because the
    /// browser never filters on this predicate for folders, and an unknown
    /// index hides every file.
    pub fn matches(&self, filter_index: usize, path: &Path) -> bool {
        self.filters
            .get(filter_index)
            .is_some_and(|filter| filter.is_match(path))
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::new(Vec::new(), false)
    }
}

/// One switchable filter group.
#[derive(Clone, Debug)]
pub struct Filter {
    /// `None` means the browser supplies its localized "all files" label;
    /// a bare glob without a label keeps the original glob text as label.
    pub label: Option<String>,
    narrows: bool,
    matcher: GlobSet,
    mime_types: Vec<mime_guess::mime::Mime>,
}

impl Filter {
    /// The group that shows every entry, used when no `--filter` was given.
    pub fn all_files() -> Self {
        Self::compile(None, &["*"], "*").expect("the wildcard glob is always valid")
    }

    /// Whether this filter restricts visible files rather than matching all files.
    pub fn narrows(&self) -> bool {
        self.narrows
    }

    /// Parses `[LABEL:]GLOB[,GLOB...]`.
    ///
    /// # Errors
    /// Reports empty or invalid glob patterns.
    pub fn from_spec(spec: &str) -> Result<Self, Error> {
        let (label, patterns) = match spec.split_once(':') {
            Some((label, patterns)) => (
                Some(label.trim()).filter(|label| !label.is_empty()),
                patterns,
            ),
            None => (Some(spec.trim()).filter(|label| !label.is_empty()), spec),
        };
        let patterns: Vec<&str> = patterns.split(',').map(str::trim).collect();
        Self::compile(label.map(str::to_owned), &patterns, spec)
    }

    /// # Errors
    /// Reports empty or invalid glob patterns.
    fn compile(label: Option<String>, patterns: &[&str], spec: &str) -> Result<Self, Error> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            if pattern.is_empty() {
                return Err(Error::EmptyFilter {
                    filter: spec.to_owned(),
                });
            }
            let glob = Glob::new(pattern).map_err(|source| Error::InvalidGlob {
                filter: spec.to_owned(),
                pattern: (*pattern).to_owned(),
                source,
            })?;
            builder.add(glob);
        }
        let matcher = builder.build().map_err(|source| Error::InvalidGlob {
            filter: spec.to_owned(),
            pattern: patterns.join(","),
            source,
        })?;
        let narrows = !patterns.contains(&"*");
        Ok(Self {
            label,
            narrows,
            matcher,
            mime_types: Vec::new(),
        })
    }

    /// # Errors
    /// Reports invalid glob or MIME filter patterns.
    fn from_portal(filter: &chooser::Filter) -> Result<Self, Error> {
        let mut builder = GlobSetBuilder::new();
        let mut mime_types = Vec::new();
        let mut matches_all = false;
        for pattern in &filter.patterns {
            match pattern {
                Pattern::Glob(pattern) => {
                    if pattern.trim() == "*" {
                        matches_all = true;
                    }
                    let literal = pattern
                        .replace('\\', "\\\\")
                        .replace('{', "\\{")
                        .replace('}', "\\}");
                    let glob = GlobBuilder::new(&literal)
                        .literal_separator(false)
                        .backslash_escape(true)
                        .build()
                        .map_err(|source| Error::InvalidGlob {
                            filter: filter.label.clone(),
                            pattern: pattern.clone(),
                            source,
                        })?;
                    builder.add(glob);
                }
                Pattern::Mime(mime) => {
                    if mime.trim() == "*/*" {
                        matches_all = true;
                    }
                    mime_types.push(mime.parse().map_err(|_| Error::InvalidMime(mime.clone()))?)
                }
            }
        }
        let matcher = builder.build().map_err(|source| Error::InvalidGlob {
            filter: filter.label.clone(),
            pattern: String::new(),
            source,
        })?;
        Ok(Self {
            label: Some(filter.label.clone()),
            narrows: !matches_all && !filter.patterns.is_empty(),
            matcher,
            mime_types,
        })
    }

    /// Matches the raw basename, so invalid UTF-8 names are still comparable.
    fn is_match(&self, path: &Path) -> bool {
        self.is_match_type(path, None)
    }

    fn is_match_type(&self, path: &Path, detected: Option<&str>) -> bool {
        let name = path.file_name().map_or(path, Path::new);
        if self.matcher.is_match(name) {
            return true;
        }
        if self.mime_types.is_empty() {
            return false;
        }
        static MIME: std::sync::LazyLock<xdg_mime::SharedMimeInfo> =
            std::sync::LazyLock::new(xdg_mime::SharedMimeInfo::new);
        if let Some(mime) = detected.and_then(|mime| mime.parse().ok()) {
            return self
                .mime_types
                .iter()
                .any(|expected| MIME.mime_type_subclass(&mime, expected));
        }
        let mut guess = MIME.guess_mime_type();
        let guessed = guess.file_name(name.to_string_lossy().as_ref()).guess();
        self.mime_types
            .iter()
            .any(|expected| MIME.mime_type_subclass(guessed.mime_type(), expected))
    }
}

/// Writes the confirmed selection to stdout as one JSON line and flushes it.
///
/// Nothing else in the picker writes to stdout. The file itself is neither
/// opened nor modified.
///
/// # Errors
/// Reports a path that cannot be made absolute, a serialization failure, or a
/// failed stdout write.
pub fn emit_selections(
    paths: &[PathBuf],
    read_only: bool,
    filter_index: usize,
) -> Result<(), Error> {
    let line = Selection::encode_line(paths, read_only, filter_index)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&line).map_err(Error::Output)?;
    stdout.write_all(b"\n").map_err(Error::Output)?;
    stdout.flush().map_err(Error::Output)
}

/// # Errors
/// Reports serialization or stdout failures.
pub fn emit_portal_result(result: &chooser::Result) -> Result<(), Error> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, result).map_err(Error::Serialize)?;
    stdout.write_all(b"\n").map_err(Error::Output)?;
    stdout.flush().map_err(Error::Output)
}

/// The selection line. A non-UTF-8 path is reported as `path: null` plus
/// `path_os`, whose Serde representation keeps the raw bytes recoverable
/// instead of substituting a lossy string.
#[derive(Serialize)]
struct Selection<'a> {
    path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path_os: Option<OsString>,
    read_only: bool,
    filter_index: usize,
}

impl Selection<'_> {
    /// # Errors
    /// Reports paths that cannot be made absolute or JSON serialization failures.
    fn encode_line(
        paths: &[PathBuf],
        read_only: bool,
        filter_index: usize,
    ) -> Result<Vec<u8>, Error> {
        let absolute: Vec<PathBuf> = paths
            .iter()
            .map(|path| {
                std::path::absolute(path).map_err(|source| Error::AbsolutePath {
                    path: path.clone(),
                    source,
                })
            })
            .collect::<Result<_, _>>()?;
        let selections: Vec<Selection<'_>> = absolute
            .iter()
            .map(|path| Selection {
                path: path.to_str(),
                path_os: path
                    .to_str()
                    .is_none()
                    .then(|| path.clone().into_os_string()),
                read_only,
                filter_index,
            })
            .collect();
        if let [selection] = selections.as_slice() {
            serde_json::to_vec(selection).map_err(Error::Serialize)
        } else {
            #[derive(Serialize)]
            struct Multiple<'a> {
                selections: &'a [Selection<'a>],
            }
            serde_json::to_vec(&Multiple {
                selections: &selections,
            })
            .map_err(Error::Serialize)
        }
    }
}

/// Records that the user picked a file and the result reached stdout.
pub fn mark_confirmed() {
    EXIT.store(EXIT_CONFIRMED, Ordering::Relaxed);
}

/// Records that the user cancelled or closed the window. This is the default.
pub fn mark_cancelled() {
    EXIT.store(EXIT_CANCELLED, Ordering::Relaxed);
}

/// Records that the selection could not be reported.
pub fn mark_failed() {
    EXIT.store(EXIT_FAILED, Ordering::Relaxed);
}

/// Exit status of the picker session.
pub fn exit_code() -> u8 {
    EXIT.load(Ordering::Relaxed)
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid save filename")]
    InvalidSaveName,
    #[error("invalid MIME filter: {0}")]
    InvalidMime(String),
    #[error("the filter {filter:?} has no glob pattern")]
    EmptyFilter { filter: String },
    #[error("invalid glob {pattern:?} in filter {filter:?}")]
    InvalidGlob {
        filter: String,
        pattern: String,
        #[source]
        source: globset::Error,
    },
    #[error("cannot determine the absolute path of {path:?}")]
    AbsolutePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot serialize the selection")]
    Serialize(#[source] serde_json::Error),
    #[error("cannot write the selection to stdout")]
    Output(#[source] std::io::Error),
}
