use super::*;
use std::collections::HashSet;

const MAX_PATHS: usize = 256;

#[derive(Default)]
pub(super) struct Changes {
    paths: HashSet<PathBuf>,
    full: bool,
}

impl Changes {
    pub fn rescan(&mut self) {
        self.full = true;
        self.paths.clear();
    }

    pub fn push(&mut self, event: notify::Event) {
        if event.need_rescan()
            || event.paths.is_empty()
            || matches!(event.kind, EventKind::Any | EventKind::Other)
        {
            self.rescan();
        }
        if self.full {
            return;
        }
        self.paths.extend(event.paths);
        if self.paths.len() > MAX_PATHS {
            self.rescan();
        }
    }

    pub fn apply(
        self,
        mut snapshot: directory::DirectorySnapshot,
    ) -> Result<directory::DirectorySnapshot, directory::Error> {
        if self.full || self.paths.contains(&snapshot.path) {
            return directory::read_directory(&snapshot.path, snapshot.options);
        }
        for path in self.paths {
            if path.parent() != Some(snapshot.path.as_path())
                || (!snapshot.options.show_hidden
                    && path
                        .file_name()
                        .is_some_and(|name| name.as_encoded_bytes().starts_with(b".")))
            {
                continue;
            }
            match directory::Entry::inspect(&path) {
                Ok(entry) => {
                    if let Some(previous) =
                        snapshot.entries.iter_mut().find(|entry| entry.path == path)
                    {
                        *previous = entry;
                    } else {
                        snapshot.entries.push(entry);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    snapshot.entries.retain(|entry| entry.path != path)
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "cannot update watched entry; rescanning");
                    return directory::read_directory(&snapshot.path, snapshot.options);
                }
            }
        }
        Ok(snapshot)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
