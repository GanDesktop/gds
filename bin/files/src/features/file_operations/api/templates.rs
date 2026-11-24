//! Discovery and instantiation of FreeDesktop `XDG_TEMPLATES_DIR` document templates.

use super::names;
use crate::features::file_operations::history;
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

const MAX_TEMPLATES: usize = 32;
const MAX_NAME_ATTEMPTS: u32 = 10_000;

/// One discoverable regular-file template from `XDG_TEMPLATES_DIR`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Template {
    pub label: String,
    pub path: PathBuf,
    pub file_name: OsString,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error(transparent)]
    Name(#[from] names::NameError),
    #[error("template is not a regular file: {path}")]
    NotAFile { path: PathBuf },
    #[error("no unused name was found for {name} in {directory}")]
    Exhausted { directory: PathBuf, name: String },
    #[error("cannot create {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Created {
    pub path: PathBuf,
    pub snapshot: Option<history::Snapshot>,
}

pub(crate) struct Request {
    pub template: PathBuf,
    pub directory: PathBuf,
}

/// Resolve the user's `XDG_TEMPLATES_DIR`, treating `$HOME` itself as disabled
/// per the `xdg-user-dirs` convention.
pub(crate) fn user_template_dir() -> Option<PathBuf> {
    let dirs = directories::UserDirs::new()?;
    let template = dirs.template_dir()?;
    let home = dirs.home_dir();
    if is_same_directory(template, home) || !template.is_dir() {
        return None;
    }
    Some(template.to_path_buf())
}

fn is_same_directory(candidate: &Path, home: &Path) -> bool {
    if candidate == home {
        return true;
    }
    match (fs::canonicalize(candidate), fs::canonicalize(home)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Discover regular-file templates directly inside `directory`.
pub(crate) fn discover(directory: &Path) -> Vec<Template> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut templates = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let bytes = file_name.as_encoded_bytes();
        if bytes.starts_with(b".") || bytes.ends_with(b"~") {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let label = file_name.to_string_lossy().into_owned();
        templates.push(Template {
            label,
            path,
            file_name,
        });
    }
    templates.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    templates.truncate(MAX_TEMPLATES);
    templates
}

/// Discover templates from the user's configured `XDG_TEMPLATES_DIR`.
pub(crate) fn discover_user() -> Vec<Template> {
    user_template_dir()
        .map(|directory| discover(&directory))
        .unwrap_or_default()
}

impl Request {
    /// Copy `self.template` into `self.directory` using a free conflict-safe filename.
    ///
    /// # Errors
    /// Rejects non-file templates or invalid names and reports filesystem failures.
    pub fn execute(self) -> Result<Created, Error> {
        let base_name = self.template.file_name().ok_or_else(|| Error::NotAFile {
            path: self.template.clone(),
        })?;
        names::validate(base_name)?;

        let mut source = fs::File::open(&self.template).map_err(|source| Error::Filesystem {
            path: self.template.clone(),
            source,
        })?;
        let source_meta = source.metadata().map_err(|source| Error::Filesystem {
            path: self.template.clone(),
            source,
        })?;
        if !source_meta.is_file() {
            return Err(Error::NotAFile {
                path: self.template,
            });
        }

        let (destination, mut target) = self.create_unique_file(base_name)?;
        if let Err(error) = io::copy(&mut source, &mut target) {
            drop(target);
            let _ = fs::remove_file(&destination);
            return Err(Error::Filesystem {
                path: destination,
                source: error,
            });
        }
        if let Err(error) = target.set_permissions(source_meta.permissions()) {
            drop(target);
            let _ = fs::remove_file(&destination);
            return Err(Error::Filesystem {
                path: destination,
                source: error,
            });
        }
        drop(target);

        let _ = gds::service::file_tags::copy(&self.template, &destination);
        let snapshot = history::Snapshot::capture(&destination).ok();
        Ok(Created {
            path: destination,
            snapshot,
        })
    }

    fn create_unique_file(
        &self,
        base_name: &std::ffi::OsStr,
    ) -> Result<(PathBuf, fs::File), Error> {
        for attempt in 1..=MAX_NAME_ATTEMPTS {
            let name = if attempt == 1 {
                base_name.to_os_string()
            } else {
                names::numbered(base_name, attempt)
            };
            let destination = self.directory.join(&name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
            {
                Ok(file) => return Ok((destination, file)),
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(Error::Filesystem {
                        path: destination,
                        source,
                    });
                }
            }
        }
        Err(Error::Exhausted {
            directory: self.directory.clone(),
            name: base_name.to_string_lossy().escape_debug().to_string(),
        })
    }

    pub fn spawn(
        self,
        tasks: &gds::service::task_manager::Scope,
        cancel: tokio_util::sync::CancellationToken,
    ) -> gds::service::task_manager::Task<Result<Created, Error>> {
        tasks.spawn_blocking_named("Create file from template", move || {
            if cancel.is_cancelled() {
                Err(Error::Cancelled)
            } else {
                self.execute()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_filters_hidden_backups_and_directories_and_sorts() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("Notes.md"), "# Notes\n").expect("write md");
        fs::write(dir.path().join("alpha.txt"), "hello").expect("write txt");
        fs::write(dir.path().join(".hidden.txt"), "secret").expect("write hidden");
        fs::write(dir.path().join("Notes.md~"), "backup").expect("write backup");
        fs::create_dir(dir.path().join("Subdir")).expect("create subdir");

        let found = discover(dir.path());
        let labels: Vec<&str> = found.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec!["alpha.txt", "Notes.md"]);
    }

    #[test]
    fn create_from_template_copies_contents_and_avoids_overwriting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tpl_dir = dir.path().join("templates");
        let work_dir = dir.path().join("work");
        fs::create_dir(&tpl_dir).expect("create templates dir");
        fs::create_dir(&work_dir).expect("create work dir");

        let tpl = tpl_dir.join("Report.md");
        fs::write(&tpl, b"# Template\n").expect("write template");

        let first = Request {
            template: tpl.clone(),
            directory: work_dir.clone(),
        }
        .execute()
        .expect("first template instantiation");
        assert_eq!(first.path, work_dir.join("Report.md"));
        assert_eq!(fs::read(&first.path).expect("read first"), b"# Template\n");
        assert!(first.snapshot.is_some());

        fs::write(&first.path, b"user edits").expect("modify first");

        let second = Request {
            template: tpl,
            directory: work_dir.clone(),
        }
        .execute()
        .expect("second template instantiation");
        assert_eq!(second.path, work_dir.join("Report (2).md"));
        assert_eq!(
            fs::read(&second.path).expect("read second"),
            b"# Template\n"
        );
        assert_eq!(fs::read(&first.path).expect("first intact"), b"user edits");
    }
}
