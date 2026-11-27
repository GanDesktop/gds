//! One history for filesystem and virtual locations. Loads commit atomically.
use super::address::Location;
use crate::entities::file::x::location::{DirectorySnapshot, ReadOptions, directory};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct Visit<V> {
    location: Location,
    view: V,
}

#[derive(Debug)]
pub struct Navigation<V> {
    directory: Option<Arc<DirectorySnapshot>>,
    path: Option<PathBuf>,
    history: Vec<Visit<V>>,
    history_index: usize,
    pending: Option<LoadRequest>,
    pending_committed: bool,
    revision: u64,
}

#[derive(Debug, Clone, Copy)]
enum Destination {
    Open,
    History { index: usize },
    Refresh,
}

#[derive(Debug, Clone)]
pub struct LoadRequest {
    id: u64,
    location: Location,
    path: Option<PathBuf>,
    options: ReadOptions,
    destination: Destination,
    record_recent: bool,
}

impl LoadRequest {
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn location(&self) -> &Location {
        &self.location
    }
    pub fn records_recent(&self) -> bool {
        self.record_recent
    }
    /// Read the requested local directory with full synchronous metadata.
    ///
    /// # Errors
    /// Reports a failed filesystem read. Virtual and unavailable locations do no I/O.
    #[cfg(test)]
    pub fn load(&self) -> Result<Option<Arc<DirectorySnapshot>>, directory::Error> {
        self.path
            .as_ref()
            .map(|path| directory::read_directory(path, self.options).map(Arc::new))
            .transpose()
    }
    /// Read the requested local directory's base metadata without secondary child-count,
    /// header-sniff, or symlink-target probes.
    ///
    /// # Errors
    /// Reports [`directory::Error::Cancelled`] when `cancel` fires, or a failed filesystem read.
    #[cfg(test)]
    pub fn load_base(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<DirectorySnapshot>>, directory::Error> {
        self.path
            .as_ref()
            .map(|path| directory::read_directory_base(path, self.options, cancel).map(Arc::new))
            .transpose()
    }
    /// Read the requested local directory's base metadata, emitting progressive snapshots
    /// through `on_progress` when enumeration exceeds `stream` thresholds.
    ///
    /// # Errors
    /// Reports [`directory::Error::Cancelled`] when `cancel` fires, or a failed filesystem read.
    pub fn load_base_streaming(
        &self,
        stream: directory::StreamOptions,
        cancel: &CancellationToken,
        on_progress: impl FnMut(Arc<DirectorySnapshot>),
    ) -> Result<Option<Arc<DirectorySnapshot>>, directory::Error> {
        self.path
            .as_ref()
            .map(|path| {
                directory::read_directory_base_streaming(
                    path,
                    self.options,
                    stream,
                    cancel,
                    on_progress,
                )
                .map(Arc::new)
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    Applied,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialLoadOutcome {
    FirstCommit,
    Updated,
    Stale,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a location is required")]
    EmptyInput,
    #[error("URI locations are not supported: {input}")]
    UnsupportedUri { input: String },
    #[error("only ~ and ~/ home expansion are supported: {input}")]
    UnsupportedHomeExpansion { input: String },
    #[error("the home directory is not available")]
    MissingHome,
    #[error("the base directory must be absolute: {path}")]
    RelativeBase { path: PathBuf },
    #[error(transparent)]
    Load(#[from] directory::Error),
    #[error("the loaded directory does not match the requested location")]
    UnexpectedSnapshot,
}

impl Error {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Load(error) if error.is_cancelled())
    }
}

/// Resolve typed local input without filesystem access or shell expansion.
///
/// Relative paths use the tab's current directory, not the process working
/// directory. Whitespace and `..` are preserved: lexical removal of `..` would
/// change the meaning of paths traversing a symlink. Non-UTF-8 paths obtained
/// from directory entries can instead be passed directly to `prepare_open`.
///
/// # Errors
/// Rejects empty input, URI scheme prefixes, unsupported ~user expansion,
/// missing home directories, or a relative base/home directory. A literal
/// relative filename containing a scheme-like colon can be entered with `./`.
pub fn resolve_input(
    input: &str,
    current_directory: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, Error> {
    if input.is_empty() {
        return Err(Error::EmptyInput);
    }
    if Path::new(input).is_absolute() {
        return Ok(PathBuf::from(input));
    }
    if is_uri(input) {
        return Err(Error::UnsupportedUri {
            input: input.to_owned(),
        });
    }
    if input == "~" || input.starts_with("~/") {
        let home = home.ok_or(Error::MissingHome)?;
        require_absolute(home)?;
        return Ok(match input.strip_prefix("~/") {
            Some(relative) => home.join(relative.trim_start_matches('/')),
            None => home.to_path_buf(),
        });
    }
    if input.starts_with('~') {
        return Err(Error::UnsupportedHomeExpansion {
            input: input.to_owned(),
        });
    }
    require_absolute(current_directory)?;
    Ok(current_directory.join(input))
}

fn is_uri(input: &str) -> bool {
    let Some((scheme, _)) = input.split_once(':') else {
        return false;
    };
    let mut characters = scheme.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

/// # Errors
/// Returns `RelativeBase` for a non-absolute directory.
fn require_absolute(path: &Path) -> Result<(), Error> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(Error::RelativeBase {
            path: path.to_path_buf(),
        })
    }
}

impl<V: Default> Navigation<V> {
    pub fn new(location: Location, path: Option<PathBuf>) -> Self {
        Self {
            directory: None,
            path,
            history: vec![Visit {
                location,
                view: V::default(),
            }],
            history_index: 0,
            pending: None,
            pending_committed: false,
            revision: 0,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Apply watch or enrichment data without entering navigation/loading or altering view state.
    pub fn update_directory(
        &mut self,
        revision: u64,
        snapshot: impl Into<Arc<DirectorySnapshot>>,
    ) -> bool {
        let snapshot = snapshot.into();
        if self.pending.is_some()
            || self.revision != revision
            || self.path() != Some(snapshot.path.as_path())
        {
            return false;
        }
        self.directory = Some(snapshot);
        true
    }

    /// Mutate the active directory snapshot in place when `revision` is still current,
    /// without entering loading or bumping the snapshot `Arc`'s strong count.
    pub fn mutate_directory<R>(
        &mut self,
        revision: u64,
        f: impl FnOnce(&mut Arc<DirectorySnapshot>) -> R,
    ) -> Option<R> {
        if self.pending.is_some() || self.revision != revision {
            return None;
        }
        let snapshot = self.directory.as_mut()?;
        if self.path.as_deref() != Some(snapshot.path.as_path()) {
            return None;
        }
        Some(f(snapshot))
    }

    pub fn has_directory(&self) -> bool {
        self.directory.is_some()
    }
    pub fn directory_entry_count(&self) -> Option<usize> {
        self.directory.as_ref().map(|dir| dir.entries.len())
    }
    pub fn directory_entries(&self) -> Option<&[directory::Entry]> {
        self.directory.as_deref().map(|dir| dir.entries.as_ref())
    }
    pub fn directory_warnings(&self) -> &[directory::EntryWarning] {
        self.directory
            .as_deref()
            .map_or(&[], |dir| dir.warnings.as_slice())
    }
    pub fn directory_hidden_mismatch(&self, show_hidden: bool) -> bool {
        self.directory
            .as_ref()
            .is_some_and(|dir| dir.options.show_hidden != show_hidden)
    }
    pub fn directory_revision_snapshot(&self) -> Option<(u64, Arc<DirectorySnapshot>)> {
        self.directory
            .as_ref()
            .map(|dir| (self.revision, Arc::clone(dir)))
    }
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn location(&self) -> &Location {
        &self.history[self.history_index].location
    }
    pub fn view(&self) -> &V {
        &self.history[self.history_index].view
    }
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.history[self.history_index].view
    }
    pub fn back_entries(&self) -> Vec<(usize, Location)> {
        (0..self.history_index)
            .rev()
            .map(|index| (index, self.history[index].location.clone()))
            .collect()
    }
    pub fn forward_entries(&self) -> Vec<(usize, Location)> {
        ((self.history_index + 1)..self.history.len())
            .map(|index| (index, self.history[index].location.clone()))
            .collect()
    }
    pub fn back_target(&self) -> Option<(usize, &Location)> {
        let index = self.history_index.checked_sub(1)?;
        Some((index, &self.history.get(index)?.location))
    }
    pub fn forward_target(&self) -> Option<(usize, &Location)> {
        let index = self.history_index.checked_add(1)?;
        Some((index, &self.history.get(index)?.location))
    }
    #[cfg(test)]
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn pending_location(&self) -> Option<&Location> {
        self.pending.as_ref().map(LoadRequest::location)
    }
    pub fn pending_path(&self) -> Option<&Path> {
        self.pending.as_ref().and_then(LoadRequest::path)
    }

    pub fn prepare_open(
        &mut self,
        location: Location,
        path: Option<PathBuf>,
        options: ReadOptions,
    ) -> LoadRequest {
        self.begin(location, path, options, Destination::Open, true)
    }
    pub fn prepare_history(
        &mut self,
        index: usize,
        path: Option<PathBuf>,
        options: ReadOptions,
    ) -> Option<LoadRequest> {
        if index == self.history_index {
            return None;
        }
        let location = self.history.get(index)?.location.clone();
        Some(self.begin(
            location,
            path,
            options,
            Destination::History { index },
            true,
        ))
    }
    pub fn prepare_initial(
        &mut self,
        path: Option<PathBuf>,
        options: ReadOptions,
        record_recent: bool,
    ) -> LoadRequest {
        self.begin(
            self.location().clone(),
            path,
            options,
            Destination::Refresh,
            record_recent,
        )
    }
    pub fn prepare_refresh(&mut self, path: Option<PathBuf>, options: ReadOptions) -> LoadRequest {
        // Preserve a still-pending visit marker if a refresh supersedes an in-flight
        // load of the same location before it commits (e.g. desktop reveal requests).
        let record_recent = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.record_recent && pending.location == *self.location());
        self.begin(
            self.location().clone(),
            path,
            options,
            Destination::Refresh,
            record_recent,
        )
    }
    pub fn cancel_pending(&mut self) {
        self.pending = None;
        self.pending_committed = false;
    }
    pub fn is_current_request(&self, request: &LoadRequest) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.id == request.id)
    }
    #[cfg(test)]
    pub fn had_partial_commit(&self, request: &LoadRequest) -> bool {
        self.is_current_request(request) && self.pending_committed
    }
    /// Apply an intermediate progressive snapshot for an in-flight local directory scan
    /// while keeping [`Self::is_loading`] active until [`Self::finish_load`] completes.
    pub fn apply_partial_load(
        &mut self,
        request: &LoadRequest,
        snapshot: Arc<DirectorySnapshot>,
    ) -> PartialLoadOutcome {
        if !self.is_current_request(request) || Some(snapshot.path.as_path()) != request.path() {
            return PartialLoadOutcome::Stale;
        }
        let first = !self.pending_committed;
        if first {
            self.commit_destination(request);
            self.pending_committed = true;
        }
        self.directory = Some(snapshot);
        if first {
            PartialLoadOutcome::FirstCommit
        } else {
            PartialLoadOutcome::Updated
        }
    }
    /// # Errors
    /// Reports current read failures or mismatched results without committing the route.
    pub fn finish_load(
        &mut self,
        request: &LoadRequest,
        result: Result<Option<Arc<DirectorySnapshot>>, directory::Error>,
    ) -> Result<LoadOutcome, Error> {
        if !self.is_current_request(request) {
            return Ok(LoadOutcome::Stale);
        }
        let already_committed = self.pending_committed;
        self.pending = None;
        self.pending_committed = false;
        let directory = result?;
        if directory.as_ref().map(|directory| directory.path.as_path()) != request.path() {
            return Err(Error::UnexpectedSnapshot);
        }
        if !already_committed {
            self.commit_destination(request);
        }
        self.directory = directory;
        self.path = request.path.clone();
        Ok(LoadOutcome::Applied)
    }
    fn commit_destination(&mut self, request: &LoadRequest) {
        match request.destination {
            Destination::Open if self.location() != &request.location => {
                self.history.truncate(self.history_index + 1);
                self.history.push(Visit {
                    location: request.location.clone(),
                    view: V::default(),
                });
                self.history_index += 1;
            }
            Destination::History { index } => {
                self.history_index = index;
                self.history[index].location = request.location.clone();
            }
            Destination::Refresh => {
                self.history[self.history_index].location = request.location.clone()
            }
            _ => (),
        }
        self.path = request.path.clone();
    }
    fn begin(
        &mut self,
        location: Location,
        path: Option<PathBuf>,
        options: ReadOptions,
        destination: Destination,
        record_recent: bool,
    ) -> LoadRequest {
        let request = LoadRequest {
            id: NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            location,
            path,
            options,
            destination,
            record_recent,
        };
        self.revision = request.id;
        self.pending = Some(request.clone());
        self.pending_committed = false;
        request
    }
}

#[cfg(test)]
mod tests;
