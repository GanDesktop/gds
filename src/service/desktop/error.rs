use std::{ffi::OsString, io, path::PathBuf, process::ExitStatus};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("无法访问位置 {path}：{source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("不是普通文件：{0}")]
    NotFile(PathBuf),
    #[error("不是目录：{0}")]
    NotDirectory(PathBuf),
    #[error("请明确选择此文件的打开方式：{0}")]
    RequiresApplication(PathBuf),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Portal(#[from] super::PortalError),
    #[cfg(not(target_os = "linux"))]
    #[error("此平台尚不支持选择打开方式")]
    UnsupportedChooser,
    #[cfg(not(target_os = "linux"))]
    #[error("无法使用默认应用打开文件 {path}：{source}")]
    OpenFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("GDS_TERMINAL 为空；请设置为单个终端可执行文件路径")]
    EmptyTerminalCommand,
    #[error("无法启动终端 {program:?}，工作目录 {directory}：{source}")]
    TerminalLaunch {
        program: OsString,
        directory: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("终端 {program:?} 退出失败（{status}），工作目录 {directory}\n{stderr}\n{stdout}")]
    TerminalExit {
        program: OsString,
        directory: PathBuf,
        status: ExitStatus,
        stderr: String,
        stdout: String,
    },
}
