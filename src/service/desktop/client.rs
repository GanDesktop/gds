//! Native desktop document and terminal operations.

#[cfg(unix)]
use super::executable;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use super::portal;
use super::{Error, OpenContext, OpenMode, OpenOutcome};

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
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    portal: std::sync::Arc<tokio::sync::OnceCell<portal::Portal>>,
}

struct OpenedDocument {
    path: PathBuf,
    file: fs::File,
    writable: bool,
}

impl Desktop {
    /// Open a regular document, optionally asking the desktop to choose an application.
    ///
    /// Executable permissions on non-executable documents (for example on FAT/NTFS/SMB
    /// mounts) do not turn them into programs; executable binaries, scripts, and desktop
    /// launchers require the application chooser. Symbolic links are resolved before
    /// checking the target. On Linux and FreeBSD, execute this future on Tokio so the
    /// Portal connection uses its I/O backend.
    ///
    /// # Errors
    /// Returns an error for inaccessible/non-regular files, executables opened without
    /// a chooser, an unavailable chooser, or failures reported by the desktop opener.
    pub async fn open_file(&self, path: &Path, mode: OpenMode) -> Result<OpenOutcome, Error> {
        self.open_file_with_context(path, mode, OpenContext::default())
            .await
    }

    /// Open a regular document with optional XDG portal parent-window and activation-token
    /// context.
    ///
    /// # Errors
    /// Returns an error for inaccessible/non-regular files, executables opened without
    /// a chooser, an unavailable chooser, or failures reported by the desktop opener.
    pub async fn open_file_with_context(
        &self,
        path: &Path,
        mode: OpenMode,
        context: OpenContext,
    ) -> Result<OpenOutcome, Error> {
        let path_owned = path.to_owned();
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            let opened =
                tokio::task::spawn_blocking(move || Self::open_document(&path_owned, mode))
                    .await
                    .map_err(|source| Error::Io {
                        path: path.to_owned(),
                        source: std::io::Error::other(source),
                    })??;
            self.portal
                .get_or_try_init(portal::Portal::connect)
                .await?
                .open_file(&opened.path, &opened.file, opened.writable, mode, &context)
                .await
                .map_err(Error::from)
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            let _ = context;
            tokio::task::spawn_blocking(move || {
                let opened = Self::open_document(&path_owned, mode)?;
                drop(opened.file);
                if mode == OpenMode::ChooseApplication {
                    return Err(Error::UnsupportedChooser);
                }
                open::that_detached(&opened.path).map_err(|source| Error::OpenFile {
                    path: opened.path,
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
        Self::open_document(path, mode).map(|opened| opened.path)
    }

    /// # Errors
    /// Reports unavailable files and targets that require an explicit application.
    fn open_document(path: &Path, mode: OpenMode) -> Result<OpenedDocument, Error> {
        let resolved = Self::resolve(path)?;
        #[cfg_attr(not(unix), allow(unused_mut))]
        let (mut file, opened_rw) =
            Self::open_for_portal(&resolved).map_err(|source| Error::Io {
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
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        Self::clear_nonblock(&file).map_err(|source| Error::Io {
            path: resolved.clone(),
            source,
        })?;
        let writable = {
            let base = opened_rw && !metadata.permissions().readonly();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                base && (metadata.permissions().mode() & 0o222 != 0)
            }
            #[cfg(not(unix))]
            {
                base
            }
        };
        if mode == OpenMode::ChooseApplication {
            return Ok(OpenedDocument {
                path: resolved,
                file,
                writable,
            });
        }
        if Self::is_desktop_launcher(path) || Self::is_desktop_launcher(&resolved) {
            return Err(Error::RequiresApplication(path.to_owned()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 {
                let header = Self::read_header_and_rewind(&mut file, &resolved)?;
                if executable::requires_application_for_executable(path, &resolved, &header) {
                    return Err(Error::RequiresApplication(path.to_owned()));
                }
            }
        }
        Ok(OpenedDocument {
            path: resolved,
            file,
            writable,
        })
    }

    /// Open `path` read-write when possible so XDG `OpenURI.OpenFile` can pass a writable
    /// document descriptor to sandboxed editors, falling back to read-only otherwise.
    fn open_for_portal(path: &Path) -> std::io::Result<(fs::File, bool)> {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            use rustix::{
                fs::{CWD, Mode, OFlags, openat},
                io::Errno,
            };
            let flags_rw = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NONBLOCK;
            let flags_ro = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK;
            loop {
                match openat(CWD, path, flags_rw, Mode::empty()) {
                    Ok(fd) => return Ok((fs::File::from(fd), true)),
                    Err(Errno::INTR) => continue,
                    Err(
                        Errno::ACCESS | Errno::ROFS | Errno::TXTBSY | Errno::PERM | Errno::ISDIR,
                    ) => break,
                    Err(error) => return Err(error.into()),
                }
            }
            loop {
                match openat(CWD, path, flags_ro, Mode::empty()) {
                    Ok(fd) => return Ok((fs::File::from(fd), false)),
                    Err(Errno::INTR) => continue,
                    Err(error) => return Err(error.into()),
                }
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            // std::fs::File::open sets O_CLOEXEC on Unix platforms.
            fs::File::open(path).map(|file| (file, false))
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
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
    fn read_header_and_rewind(file: &mut fs::File, path: &Path) -> Result<Vec<u8>, Error> {
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
        Ok(header)
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
