use std::{ffi::OsString, io, path::PathBuf, process::ExitStatus};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot access location {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("not a regular file: {0}")]
    NotFile(PathBuf),
    #[error("not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("an application must be selected to open this file: {0}")]
    RequiresApplication(PathBuf),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Portal(#[from] super::PortalError),
    #[cfg(not(target_os = "linux"))]
    #[error("choosing an application is not supported on this platform")]
    UnsupportedChooser,
    #[cfg(not(target_os = "linux"))]
    #[error("cannot open file {path} with the default application: {source}")]
    OpenFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("GDS_TERMINAL is empty; set it to a single terminal executable path")]
    EmptyTerminalCommand,
    #[error("cannot start terminal {program:?} in {directory}: {source}")]
    TerminalLaunch {
        program: OsString,
        directory: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "terminal {program:?} exited unsuccessfully ({status}) in {directory}\n{stderr}\n{stdout}"
    )]
    TerminalExit {
        program: OsString,
        directory: PathBuf,
        status: ExitStatus,
        stderr: String,
        stdout: String,
    },
}
