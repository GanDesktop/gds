use super::*;
use std::{ffi::OsStr, path::Path, sync::Arc, time::Duration};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const STAT_VALUE_COLUMN: usize = 22;
const MAX_CONCURRENT_STATS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Executable not found: {0}")]
    ExecutableNotFound(String),
    #[error("{} timed out", .0.display())]
    TimedOut(PathBuf),
    #[error("{}: {detail}", .program.display())]
    CommandFailed { program: PathBuf, detail: String },
    #[error("Invalid UTF-8 command output: {0}")]
    InvalidUtf8(String),
    #[error("{0}")]
    Disabled(String),
    #[error("The Baloo tags worker cannot reliably query tag names containing a double quote")]
    QuotedTag,
    #[error("Cannot delete the tag root or a tag containing a double quote")]
    InvalidDeleteTarget,
    #[error(transparent)]
    InvalidAddress(#[from] crate::entities::location::x::tags::TagAddressError),
    #[error("Baloo returned a non-local result")]
    NonLocalResult,
    #[error("Cannot resolve Baloo entry: {0}")]
    UnresolvedEntry(String),
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}

enum ChildEntry {
    Tag(String),
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Baloo {
    control: PathBuf,
    kio: PathBuf,
    root_tags: Vec<String>,
}

impl Baloo {
    /// Verifies that the Baloo index and KIO `tags:/` worker are available and returns a client.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when `balooctl` or `kioclient` cannot be resolved, Baloo indexing is
    /// disabled, or the `tags:/` KIO worker fails to list root tags.
    pub async fn connect(commands: &Commands) -> Result<Self, Error> {
        let control = Self::resolve(commands.control.as_deref(), &["balooctl6", "balooctl"])?;
        let kio = Self::resolve(
            commands.kio_client.as_deref(),
            &["kioclient6", "kioclient", "kioclient5"],
        )?;
        let ((), root_tags) =
            tokio::try_join!(Self::check_status(&control), Self::list_root_tags(&kio))?;
        Ok(Self {
            control,
            kio,
            root_tags,
        })
    }

    /// Lists sub-tags and tagged local files under `tag` (or root tags when `tag` is empty).
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when `tag` is invalid or contains a double quote, or when KIO queries
    /// fail or return non-local entries.
    pub async fn browse(&self, tag: &str) -> Result<Listing, Error> {
        // The upstream tags worker interpolates names into its query string.
        // Baloo 6.31 silently returns no files for a tag containing a quote.
        if tag.contains('"') {
            return Err(Error::QuotedTag);
        }
        if tag.is_empty() {
            return Ok(Listing {
                tags: self.root_tags.clone(),
                files: Vec::new(),
            });
        }
        let uri = crate::entities::location::x::tags::TagAddress::new(tag)?.uri();
        let names = Self::run(&self.kio, &["ls", &uri]).await?;
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_STATS));
        let mut tasks = tokio::task::JoinSet::new();
        for name in names.lines().filter(|s| !matches!(*s, "" | "." | "..")) {
            let child = format!("{tag}/{name}");
            let kio = self.kio.clone();
            let semaphore = Arc::clone(&semaphore);
            tasks.spawn(async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("local stat semaphore never closes");
                Self::stat_child(kio, child).await
            });
        }
        let mut listing = Listing::default();
        while let Some(joined) = tasks.join_next().await {
            match joined.expect("stat task never panics")? {
                ChildEntry::Tag(child) => listing.tags.push(child),
                ChildEntry::File(path) => listing.files.push(path),
            }
        }
        listing.tags.sort();
        listing.tags.dedup();
        listing.files.sort();
        listing.files.dedup();
        Ok(listing)
    }

    /// Removes `tag` and its descendants from indexed files, preserving the files themselves.
    ///
    /// # Errors
    ///
    /// Returns an error description when `tag` is empty or invalid, Baloo is disabled, or KIO
    /// removal fails.
    pub async fn delete_tag(&self, tag: &str) -> Result<(), String> {
        self.remove_tag(tag).await.map_err(Into::into)
    }

    /// Removes `tag` and its descendants from indexed files, preserving the files themselves.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when `tag` is empty or invalid, Baloo is disabled, or KIO removal fails.
    pub async fn remove_tag(&self, tag: &str) -> Result<(), Error> {
        if tag.is_empty() || tag.contains('"') {
            return Err(Error::InvalidDeleteTarget);
        }
        let address = crate::entities::location::x::tags::TagAddress::new(tag)?;
        Self::check_status(&self.control).await?;
        // KIO's tags worker removes associations recursively, as in Dolphin.
        Self::run(&self.kio, &["remove", &address.uri()]).await?;
        Ok(())
    }

    async fn check_status(control: &Path) -> Result<(), Error> {
        let status = Self::run(control, &["status"]).await?;
        if status.to_ascii_lowercase().contains("disabled") {
            return Err(Error::Disabled(status.trim().to_owned()));
        }
        Ok(())
    }

    async fn list_root_tags(kio: &Path) -> Result<Vec<String>, Error> {
        // Listing `tags:/` also verifies that Baloo's tags KIO worker is installed and loadable.
        let names = Self::run(kio, &["ls", "tags:/"]).await?;
        let mut tags: Vec<String> = names
            .lines()
            .filter(|s| !matches!(*s, "" | "." | ".."))
            .map(str::to_owned)
            .collect();
        tags.sort();
        tags.dedup();
        Ok(tags)
    }

    async fn stat_child(kio: PathBuf, child: String) -> Result<ChildEntry, Error> {
        let child_uri = crate::entities::location::x::tags::TagAddress::new(&child)?.uri();
        let stat = Self::run(&kio, &["stat", &child_uri]).await?;
        if let Some(path) = Self::field(&stat, "LOCAL_PATH") {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(Error::NonLocalResult);
            }
            Ok(ChildEntry::File(path))
        } else if matches!(
            Self::field(&stat, "DISPLAY_TYPE"),
            Some("Tag" | "Tag Fragment")
        ) {
            Ok(ChildEntry::Tag(child))
        } else {
            Err(Error::UnresolvedEntry(child_uri))
        }
    }

    pub(super) fn field<'a>(stat: &'a str, name: &str) -> Option<&'a str> {
        stat.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?;
            // kioclient uses a 20-column field name followed by two spaces.
            let padding = STAT_VALUE_COLUMN.checked_sub(name.len())?;
            if rest.len() < padding || !rest.as_bytes()[..padding].iter().all(|b| *b == b' ') {
                return None;
            }
            Some(&rest[padding..])
        })
    }

    pub(super) fn resolve(configured: Option<&Path>, defaults: &[&str]) -> Result<PathBuf, Error> {
        Self::resolve_in_path(
            configured,
            defaults,
            &std::env::var_os("PATH").unwrap_or_default(),
        )
    }

    pub(super) fn resolve_in_path(
        configured: Option<&Path>,
        defaults: &[&str],
        search_path: &OsStr,
    ) -> Result<PathBuf, Error> {
        let names: Vec<&OsStr> = configured.map_or_else(
            || defaults.iter().map(OsStr::new).collect(),
            |p| vec![p.as_os_str()],
        );
        for name in names {
            let path = Path::new(name);
            let candidates = if path.components().count() > 1 || path.is_absolute() {
                vec![path.to_owned()]
            } else {
                std::env::split_paths(search_path)
                    .map(|dir| dir.join(path))
                    .collect()
            };
            for candidate in candidates {
                if Self::executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        Err(Error::ExecutableNotFound(
            configured
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| defaults.join(" / ")),
        ))
    }

    fn executable(path: &Path) -> bool {
        let Ok(metadata) = path.metadata() else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    async fn run(program: &Path, args: &[&str]) -> Result<String, Error> {
        let output = tokio::time::timeout(
            COMMAND_TIMEOUT,
            tokio::process::Command::new(program)
                .args(args)
                .env("LC_ALL", "C.UTF-8")
                .env("LANGUAGE", "C")
                .env("QT_QPA_PLATFORM", "offscreen")
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| Error::TimedOut(program.to_path_buf()))?
        .map_err(|error| Error::CommandFailed {
            program: program.to_path_buf(),
            detail: error.to_string(),
        })?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: program.to_path_buf(),
                detail: format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }
        String::from_utf8(output.stdout).map_err(|error| Error::InvalidUtf8(error.to_string()))
    }
}

#[cfg(test)]
mod tests;
