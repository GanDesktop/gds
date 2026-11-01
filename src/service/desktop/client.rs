//! Native desktop document and terminal operations.

use super::Error;
#[cfg(target_os = "linux")]
use super::portal;

use std::{
    env,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Default)]
pub struct Desktop {
    #[cfg(target_os = "linux")]
    portal: std::sync::Arc<tokio::sync::OnceCell<portal::Portal>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    DefaultApplication,
    ChooseApplication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened,
    Cancelled,
}

impl Desktop {
    /// Open a regular document, optionally asking the desktop to choose an application.
    ///
    /// Executable permissions on recognised media do not turn them into programs.
    /// Other executable files and desktop launchers require the application chooser.
    /// Symbolic links are resolved before checking the target. On Linux, execute
    /// this future on Tokio so the Portal connection uses its I/O backend.
    ///
    /// # Errors
    /// Returns an error for inaccessible/non-regular files, executables opened without
    /// a chooser, an unavailable chooser, or failures reported by the desktop opener.
    pub async fn open_file(&self, path: &Path, mode: OpenMode) -> Result<OpenOutcome, Error> {
        let resolved = Self::document_path(path, mode)?;
        #[cfg(target_os = "linux")]
        {
            self.portal
                .get_or_try_init(portal::Portal::connect)
                .await?
                .open_file(&resolved, mode)
                .await
                .map_err(Error::from)
        }
        #[cfg(not(target_os = "linux"))]
        {
            if mode == OpenMode::ChooseApplication {
                return Err(Error::UnsupportedChooser);
            }
            open::that_detached(&resolved).map_err(|source| Error::OpenFile {
                path: resolved,
                source,
            })?;
            Ok(OpenOutcome::Opened)
        }
    }

    /// # Errors
    /// Reports unavailable files and targets that require an explicit application.
    fn document_path(path: &Path, mode: OpenMode) -> Result<PathBuf, Error> {
        let resolved = Self::resolve(path)?;
        let metadata = fs::metadata(&resolved).map_err(|source| Error::Io {
            path: resolved.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(Error::NotFile(path.to_owned()));
        }
        if mode == OpenMode::ChooseApplication {
            return Ok(resolved);
        }
        if Self::is_desktop_launcher(path) || Self::is_desktop_launcher(&resolved) {
            return Err(Error::RequiresApplication(path.to_owned()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 && !Self::is_media(&resolved)? {
                return Err(Error::RequiresApplication(path.to_owned()));
            }
        }
        Ok(resolved)
    }

    /// # Errors
    /// Reports errors reading the file header.
    fn is_media(path: &Path) -> Result<bool, Error> {
        const HEADER_LIMIT: u64 = 8 * 1024;
        let mut header = Vec::new();
        fs::File::open(path)
            .and_then(|file| file.take(HEADER_LIMIT).read_to_end(&mut header))
            .map_err(|source| Error::Io {
                path: path.to_owned(),
                source,
            })?;
        Ok(infer::get(&header).is_some_and(|kind| {
            matches!(
                kind.matcher_type(),
                infer::MatcherType::Video | infer::MatcherType::Audio | infer::MatcherType::Image
            )
        }))
    }

    /// Open an external terminal at an existing local directory.
    ///
    /// `GDS_TERMINAL` is one executable path/name, never a shell command line. Its
    /// process receives the directory as cwd and inherits the application environment.
    /// Without that override, use `xdg-terminal-exec --dir=<directory>`; there is no
    /// guessed emulator-specific argument or hard-coded shell. This waits for the
    /// launcher to exit, which can be the terminal's lifetime for a non-daemonizing
    /// emulator, so callers must keep it off the UI thread.
    ///
    /// # Errors
    /// Returns an error for an inaccessible/non-directory target, an empty override,
    /// a missing terminal launcher, or a launcher failing to start or exiting with a
    /// non-success status. External stderr/stdout are included in failure context.
    pub fn open_terminal(&self, directory: &Path) -> Result<(), Error> {
        let directory = Self::resolve(directory)?;
        let metadata = fs::metadata(&directory).map_err(|source| Error::Io {
            path: directory.clone(),
            source,
        })?;
        if !metadata.is_dir() {
            return Err(Error::NotDirectory(directory));
        }
        let mut command = match env::var_os("GDS_TERMINAL") {
            Some(program) if program.is_empty() => return Err(Error::EmptyTerminalCommand),
            Some(program) => Command::new(program),
            None => {
                let mut argument = OsString::from("--dir=");
                argument.push(directory.as_os_str());
                let mut command = Command::new("xdg-terminal-exec");
                command.arg(argument);
                command
            }
        };
        command.current_dir(&directory).stdin(Stdio::null());
        let program = command.get_program().to_owned();
        let output = command.output().map_err(|source| Error::TerminalLaunch {
            program: program.clone(),
            directory: directory.clone(),
            source,
        })?;
        if !output.status.success() {
            return Err(Error::TerminalExit {
                program,
                directory,
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            });
        }
        Ok(())
    }

    fn is_desktop_launcher(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("desktop"))
    }

    /// # Errors
    /// Preserves the filesystem error for an unavailable target or broken link.
    fn resolve(path: &Path) -> Result<PathBuf, Error> {
        fs::canonicalize(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })
    }
}

#[cfg(test)]
mod tests;
