//! One history for filesystem and virtual locations. Loads commit atomically.
use super::address::Location;
use crate::entities::file::x::location::{DirectorySnapshot, ReadOptions, directory};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct Visit<V> {
    pub location: Location,
    pub view: V,
}

#[derive(Debug)]
pub struct Navigation<V> {
    directory: Option<DirectorySnapshot>,
    path: Option<PathBuf>,
    history: Vec<Visit<V>>,
    history_index: usize,
    pending: Option<LoadRequest>,
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
}

impl LoadRequest {
    pub fn resolve_remote(mut self, address: gds::service::remote::Address) -> Self {
        self.location = Location::Remote(address);
        self.path = None;
        self
    }
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn location(&self) -> &Location {
        &self.location
    }
    /// # Errors
    /// Reports a failed filesystem read. Virtual and unavailable locations do no I/O.
    pub fn load(&self) -> Result<Option<DirectorySnapshot>, directory::Error> {
        self.path
            .as_ref()
            .map(|path| directory::read_directory(path, self.options))
            .transpose()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    Applied,
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
        }
    }
    pub fn directory(&self) -> Option<&DirectorySnapshot> {
        self.directory.as_ref()
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
    pub fn history(&self) -> &[Visit<V>] {
        &self.history
    }
    pub fn history_index(&self) -> usize {
        self.history_index
    }
    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }
    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn pending_location(&self) -> Option<&Location> {
        self.pending.as_ref().map(LoadRequest::location)
    }
    pub fn back_location(&self) -> Option<&Location> {
        self.history_index
            .checked_sub(1)
            .map(|index| &self.history[index].location)
    }
    pub fn forward_location(&self) -> Option<&Location> {
        self.history
            .get(self.history_index + 1)
            .map(|visit| &visit.location)
    }

    pub fn prepare_open(
        &mut self,
        location: Location,
        path: Option<PathBuf>,
        options: ReadOptions,
    ) -> LoadRequest {
        self.begin(location, path, options, Destination::Open)
    }
    pub fn prepare_back(
        &mut self,
        path: Option<PathBuf>,
        options: ReadOptions,
    ) -> Option<LoadRequest> {
        let index = self.history_index.checked_sub(1)?;
        self.prepare_history(index, path, options)
    }
    pub fn prepare_forward(
        &mut self,
        path: Option<PathBuf>,
        options: ReadOptions,
    ) -> Option<LoadRequest> {
        self.prepare_history(self.history_index + 1, path, options)
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
        Some(self.begin(location, path, options, Destination::History { index }))
    }
    pub fn prepare_refresh(&mut self, path: Option<PathBuf>, options: ReadOptions) -> LoadRequest {
        self.begin(self.location().clone(), path, options, Destination::Refresh)
    }
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }
    pub fn is_current_request(&self, request: &LoadRequest) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.id == request.id)
    }
    pub fn cancel_request(&mut self, request: &LoadRequest) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == request.id)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }
    /// # Errors
    /// Reports current read failures or mismatched results without committing the route.
    pub fn finish_load(
        &mut self,
        request: &LoadRequest,
        result: Result<Option<DirectorySnapshot>, directory::Error>,
    ) -> Result<LoadOutcome, Error> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == request.id)
        {
            return Ok(LoadOutcome::Stale);
        }
        self.pending = None;
        let directory = result?;
        if directory.as_ref().map(|directory| directory.path.as_path()) != request.path() {
            return Err(Error::UnexpectedSnapshot);
        }
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
        self.directory = directory;
        self.path = request.path.clone();
        Ok(LoadOutcome::Applied)
    }
    fn begin(
        &mut self,
        location: Location,
        path: Option<PathBuf>,
        options: ReadOptions,
        destination: Destination,
    ) -> LoadRequest {
        let request = LoadRequest {
            id: NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            location,
            path,
            options,
            destination,
        };
        self.pending = Some(request.clone());
        request
    }
}

#[cfg(test)]
mod tests;
