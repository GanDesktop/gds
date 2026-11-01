use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::{
        fd::{AsRawFd, OwnedFd, RawFd},
        unix::process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand},
};

use super::super::session::{Error, SpawnOptions};
use nix::{
    fcntl::{OFlag, open},
    sys::stat::Mode,
    unistd::{AccessFlags, Uid, User, access},
};

pub(super) struct Command {
    builder: ProcessCommand,
    pub(super) directory_commands: bool,
    // Keep validation and the child's chdir on the same directory inode.
    _directory: OwnedFd,
}

impl Command {
    pub(super) fn prepare(options: &SpawnOptions) -> Result<Self, Error> {
        let cwd = &options.cwd;
        if !cwd.is_absolute() || !cwd.is_dir() {
            return Err(Error::InvalidDirectory(cwd.clone()));
        }
        #[cfg(target_os = "linux")]
        let flags = OFlag::O_PATH | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC;
        #[cfg(not(target_os = "linux"))]
        let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC;
        let directory =
            open(cwd, flags, Mode::empty()).map_err(|_| Error::InvalidDirectory(cwd.clone()))?;
        #[cfg(target_os = "linux")]
        let fd_root = "/proc/self/fd";
        #[cfg(not(target_os = "linux"))]
        let fd_root = "/dev/fd";
        let pinned_cwd = PathBuf::from(fd_root).join(directory.as_raw_fd().to_string());
        if !pinned_cwd.is_dir() {
            return Err(Error::Pty(anyhow::anyhow!(
                "This platform cannot preserve the requested PTY working directory through {fd_root}"
            )));
        }
        let shell = resolve_shell(options.shell.as_deref())?;
        let mut builder = ProcessCommand::new(&shell);
        builder.current_dir(&pinned_cwd);
        builder.env("SHELL", &shell);
        builder.env("PWD", cwd);
        builder.env("TERM", "xterm-256color");
        builder.env("COLORTERM", "truecolor");
        builder.env("TERM_PROGRAM", "gds");
        // A controlling TTY makes standard shells interactive automatically,
        // without imposing bash/zsh-specific flags or bypassing their rc files.
        if options.login {
            builder.arg(login_argument(&shell)?);
        }
        builder.args(&options.args);
        Ok(Self {
            builder,
            directory_commands: super::super::directory::supported_shell(&shell),
            _directory: directory,
        })
    }

    pub(super) fn spawn(mut self, slave_path: &Path) -> io::Result<Child> {
        let slave = open(
            slave_path,
            OFlag::O_RDWR | OFlag::O_NOCTTY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(io::Error::from)?;
        self.builder
            .stdin(slave.try_clone()?)
            .stdout(slave.try_clone()?)
            .stderr(slave);
        let descriptors = inherited_descriptors()?;
        // portable-pty 0.9's Unix spawn closes ALL fds > 2 in pre_exec,
        // including Rust's exec-error pipe. Keep portable-pty for all PTY I/O,
        // but use std's spawn with CLOEXEC so exec failures reach the caller.
        unsafe {
            self.builder.pre_exec(move || prepare_child(&descriptors));
        }
        self.builder.spawn()
    }
}

fn inherited_descriptors() -> io::Result<Vec<RawFd>> {
    #[cfg(target_os = "linux")]
    let directory = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    let directory = "/dev/fd";
    let mut descriptors = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<RawFd>().ok())
        {
            if fd > 2 {
                descriptors.push(fd);
            }
        }
    }
    Ok(descriptors)
}

// Runs between fork and exec: only async-signal-safe syscalls, no allocation/locks.
fn prepare_child(descriptors: &[RawFd]) -> io::Result<()> {
    use nix::libc;
    unsafe {
        for signal in [
            libc::SIGCHLD,
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTERM,
            libc::SIGALRM,
        ] {
            if libc::signal(signal, libc::SIG_DFL) == libc::SIG_ERR {
                return Err(io::Error::last_os_error());
            }
        }
        let mut mask: libc::sigset_t = std::mem::zeroed();
        if libc::sigemptyset(&mut mask) != 0
            || libc::sigprocmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut()) != 0
        {
            return Err(io::Error::last_os_error());
        }
        if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
            return Err(io::Error::last_os_error());
        }
        #[cfg(target_os = "linux")]
        {
            // Unlike close(), CLOEXEC preserves std's error pipe on failed exec.
            if libc::syscall(
                libc::SYS_close_range,
                3u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            ) == 0
            {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if !matches!(
                error.raw_os_error(),
                Some(libc::ENOSYS | libc::EINVAL | libc::EPERM)
            ) {
                return Err(error);
            }
        }
        // Older kernels/other Unix: snapshot was allocated on the parent side.
        for &fd in descriptors {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EBADF) {
                    continue;
                }
                return Err(error);
            }
            if libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

fn executable(path: &Path) -> bool {
    path.is_absolute() && path.is_file() && access(path, AccessFlags::X_OK).is_ok()
}

fn resolve_shell(configured: Option<&Path>) -> Result<PathBuf, Error> {
    if let Some(shell) = configured {
        return if executable(shell) {
            Ok(shell.to_owned())
        } else {
            Err(Error::InvalidShell(shell.to_owned()))
        };
    }
    let user = User::from_uid(Uid::current()).map_err(anyhow::Error::from)?;
    let environment = std::env::var_os("SHELL").map(PathBuf::from);
    default_shell(
        user.as_ref().map(|user| user.shell.as_path()),
        environment.as_deref(),
    )
}

fn default_shell(account: Option<&Path>, environment: Option<&Path>) -> Result<PathBuf, Error> {
    // Development tools may replace SHELL with a build-only shell, not the
    // user's chosen interactive shell. Prefer the account's configured shell.
    account
        .into_iter()
        .chain(environment)
        .find(|path| executable(path))
        .map(Path::to_owned)
        .ok_or(Error::ShellUnavailable)
}

#[cfg(test)]
mod tests;

fn login_argument(shell: &Path) -> Result<&'static str, Error> {
    match shell.file_name().and_then(OsStr::to_str) {
        Some("bash" | "zsh" | "fish" | "nu") => Ok("--login"),
        Some("sh" | "dash" | "ash" | "ksh" | "mksh" | "yash" | "csh" | "tcsh") => Ok("-l"),
        _ => Err(Error::UnsupportedLogin(shell.to_owned())),
    }
}
