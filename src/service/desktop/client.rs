//! Native desktop document and terminal operations.

use super::Error;
#[cfg(target_os = "linux")]
use super::portal;

use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const TERMINAL_PROBE_WINDOW: Duration = Duration::from_millis(200);
const TERMINAL_PROBE_POLL: Duration = Duration::from_millis(10);
const TERMINAL_OUTPUT_LIMIT: u64 = 8 * 1024;

struct TempOutput {
    file: fs::File,
    #[cfg(not(unix))]
    path: Option<PathBuf>,
}

impl TempOutput {
    fn create(label: &str) -> std::io::Result<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        let temp_dir = env::temp_dir();
        let pid = std::process::id();
        for _ in 0..64 {
            let seq = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = temp_dir.join(format!("gds-terminal-{pid}-{seq}-{label}.log"));
            let mut options = fs::OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    #[cfg(unix)]
                    let _ = fs::remove_file(&path);
                    return Ok(Self {
                        file,
                        #[cfg(not(unix))]
                        path: Some(path),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "unable to allocate temporary terminal output file",
        ))
    }

    fn stdio(&self) -> std::io::Result<Stdio> {
        self.file.try_clone().map(Stdio::from)
    }

    fn read_bounded(&mut self, limit: u64) -> String {
        use std::io::{Read, Seek, SeekFrom};

        if self.file.seek(SeekFrom::Start(0)).is_err() {
            return String::new();
        }
        let mut bytes = Vec::new();
        if self
            .file
            .by_ref()
            .take(limit)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return String::new();
        }
        String::from_utf8_lossy(&bytes).trim().to_owned()
    }
}

#[cfg(not(unix))]
impl Drop for TempOutput {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

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
        let path_owned = path.to_owned();
        #[cfg(target_os = "linux")]
        {
            let (resolved, file) =
                tokio::task::spawn_blocking(move || Self::open_document(&path_owned, mode))
                    .await
                    .map_err(|source| Error::Io {
                        path: path.to_owned(),
                        source: std::io::Error::other(source),
                    })??;
            self.portal
                .get_or_try_init(portal::Portal::connect)
                .await?
                .open_file(&resolved, &file, mode)
                .await
                .map_err(Error::from)
        }
        #[cfg(not(target_os = "linux"))]
        {
            tokio::task::spawn_blocking(move || {
                let (resolved, file) = Self::open_document(&path_owned, mode)?;
                drop(file);
                if mode == OpenMode::ChooseApplication {
                    return Err(Error::UnsupportedChooser);
                }
                open::that_detached(&resolved).map_err(|source| Error::OpenFile {
                    path: resolved,
                    source,
                })?;
                Ok(OpenOutcome::Opened)
            })
            .await
            .map_err(|source| Error::Io {
                path: path.to_owned(),
                source: std::io::Error::other(source),
            })?
        }
    }

    #[cfg(test)]
    fn document_path(path: &Path, mode: OpenMode) -> Result<PathBuf, Error> {
        Self::open_document(path, mode).map(|(resolved, _)| resolved)
    }

    /// # Errors
    /// Reports unavailable files and targets that require an explicit application.
    fn open_document(path: &Path, mode: OpenMode) -> Result<(PathBuf, fs::File), Error> {
        let resolved = Self::resolve(path)?;
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut file = Self::open_readonly(&resolved).map_err(|source| Error::Io {
            path: resolved.clone(),
            source,
        })?;
        let metadata = file.metadata().map_err(|source| Error::Io {
            path: resolved.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(Error::NotFile(path.to_owned()));
        }
        #[cfg(target_os = "linux")]
        Self::clear_nonblock(&file).map_err(|source| Error::Io {
            path: resolved.clone(),
            source,
        })?;
        if mode == OpenMode::ChooseApplication {
            return Ok((resolved, file));
        }
        if Self::is_desktop_launcher(path) || Self::is_desktop_launcher(&resolved) {
            return Err(Error::RequiresApplication(path.to_owned()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 && !Self::is_media(&mut file, &resolved)?
            {
                return Err(Error::RequiresApplication(path.to_owned()));
            }
        }
        Ok((resolved, file))
    }

    fn open_readonly(path: &Path) -> std::io::Result<fs::File> {
        #[cfg(target_os = "linux")]
        {
            use rustix::{
                fs::{CWD, Mode, OFlags, openat},
                io::Errno,
            };
            loop {
                match openat(
                    CWD,
                    path,
                    OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
                    Mode::empty(),
                ) {
                    Ok(fd) => return Ok(fs::File::from(fd)),
                    Err(Errno::INTR) => continue,
                    Err(error) => return Err(error.into()),
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            // std::fs::File::open sets O_CLOEXEC on Unix platforms.
            fs::File::open(path)
        }
    }

    #[cfg(target_os = "linux")]
    fn clear_nonblock(file: &fs::File) -> std::io::Result<()> {
        use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

        let flags = fcntl_getfl(file)?;
        if flags.contains(OFlags::NONBLOCK) {
            fcntl_setfl(file, flags & !OFlags::NONBLOCK)?;
        }
        Ok(())
    }

    /// # Errors
    /// Reports errors reading or rewinding the file header.
    #[cfg(unix)]
    fn is_media(file: &mut fs::File, path: &Path) -> Result<bool, Error> {
        use std::io::{Read, Seek, SeekFrom};

        const HEADER_LIMIT: u64 = 8 * 1024;
        let mut header = Vec::new();
        file.by_ref()
            .take(HEADER_LIMIT)
            .read_to_end(&mut header)
            .and_then(|_| file.seek(SeekFrom::Start(0)))
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
    /// guessed emulator-specific argument or hard-coded shell. This probes startup
    /// briefly (~200 ms) to report immediate launch failures, then detaches so
    /// non-daemonizing terminals do not block callers or task shutdown.
    ///
    /// # Errors
    /// Returns an error for an inaccessible/non-directory target, an empty override,
    /// a missing terminal launcher, or a launcher failing to start or exiting quickly
    /// with a non-success status. External stderr/stdout are included in failure context.
    pub fn open_terminal(&self, directory: &Path) -> Result<(), Error> {
        self.open_terminal_with_override(directory, env::var_os("GDS_TERMINAL"))
    }

    fn open_terminal_with_override(
        &self,
        directory: &Path,
        terminal_override: Option<OsString>,
    ) -> Result<(), Error> {
        let directory = Self::resolve(directory)?;
        let metadata = fs::metadata(&directory).map_err(|source| Error::Io {
            path: directory.clone(),
            source,
        })?;
        if !metadata.is_dir() {
            return Err(Error::NotDirectory(directory));
        }
        let mut command = match terminal_override {
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
        let program = command.get_program().to_owned();
        let launch_error = |source| Error::TerminalLaunch {
            program: program.clone(),
            directory: directory.clone(),
            source,
        };
        let mut stdout_capture = TempOutput::create("stdout").map_err(&launch_error)?;
        let mut stderr_capture = TempOutput::create("stderr").map_err(&launch_error)?;
        let stdout_stdio = stdout_capture.stdio().map_err(&launch_error)?;
        let stderr_stdio = stderr_capture.stdio().map_err(&launch_error)?;
        command
            .current_dir(&directory)
            .stdin(Stdio::null())
            .stdout(stdout_stdio)
            .stderr(stderr_stdio);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(&launch_error)?;
        let deadline = Instant::now() + TERMINAL_PROBE_WINDOW;
        loop {
            match child.try_wait().map_err(&launch_error)? {
                Some(status) => {
                    if status.success() {
                        return Ok(());
                    }
                    return Err(Error::TerminalExit {
                        program,
                        directory,
                        status,
                        stderr: stderr_capture.read_bounded(TERMINAL_OUTPUT_LIMIT),
                        stdout: stdout_capture.read_bounded(TERMINAL_OUTPUT_LIMIT),
                    });
                }
                None => {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    std::thread::sleep(TERMINAL_PROBE_POLL.min(deadline - now));
                }
            }
        }
        #[cfg(unix)]
        drop((stdout_capture, stderr_capture));
        if let Err(err) = std::thread::Builder::new()
            .name("gds-terminal-reap".into())
            .spawn(move || {
                let _ = child.wait();
                #[cfg(not(unix))]
                drop((stdout_capture, stderr_capture));
            })
        {
            tracing::warn!(%err, "failed to spawn terminal reaper thread");
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
