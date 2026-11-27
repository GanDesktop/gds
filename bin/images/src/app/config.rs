//! Startup mode resolution, bounded pre-window stdin ingestion, and dual Wayland App ID routing.

#[cfg(test)]
mod tests;

use std::{
    io::{self, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use super::cli::Arguments;

pub const STANDARD_APP_ID: &str = "io.github.GanDesktop.GdsImages";
pub const CAPTURE_APP_ID: &str = "io.github.GanDesktop.GdsImages.Capture";
pub const MAX_STDIN_BYTES: usize = 256 * 1024 * 1024;

use crate::pages::workspace::{CaptureWorkflow, LaunchMode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConfig {
    pub mode: LaunchMode,
    pub workflow: CaptureWorkflow,
    pub app_id: &'static str,
    pub is_capture: bool,
    pub enable_logs: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartupOutcome {
    Launch(AppConfig),
    /// Emitted when `--stdin` / `-` hits clean 0-byte EOF (e.g., user cancelled screenshot selection).
    SilentExit,
}

/// Resolves startup configuration from parsed CLI flags and process stdin.
///
/// # Errors
/// Returns an error if resolving relative CLI paths against `current_dir` fails, or if reading
/// from stdin fails or exceeds [`MAX_STDIN_BYTES`].
pub fn resolve_startup(args: &Arguments) -> anyhow::Result<StartupOutcome> {
    if wants_stdin(args) {
        let stdin = io::stdin();
        let locked = stdin.lock();
        resolve_startup_with_reader(args, locked)
    } else {
        resolve_startup_with_reader(args, io::empty())
    }
}

pub(crate) fn resolve_startup_with_reader<R: Read>(
    args: &Arguments,
    reader: R,
) -> anyhow::Result<StartupOutcome> {
    let from_stdin = wants_stdin(args);
    let is_capture = from_stdin
        || args.clipboard
        || args.copy_on_save
        || args.early_exit
        || args.output.is_some();
    let app_id = if is_capture {
        CAPTURE_APP_ID
    } else {
        STANDARD_APP_ID
    };

    let mode = if from_stdin {
        let Some(bytes) = read_bounded_stdin(reader, MAX_STDIN_BYTES)? else {
            return Ok(StartupOutcome::SilentExit);
        };
        LaunchMode::Stdin(bytes)
    } else if args.clipboard {
        LaunchMode::Clipboard
    } else if let Some(path) = &args.file {
        LaunchMode::OpenFile(resolve_cli_path(path)?)
    } else {
        LaunchMode::EmptySmartState
    };

    let output = args.output.as_deref().map(resolve_cli_path).transpose()?;

    Ok(StartupOutcome::Launch(AppConfig {
        mode,
        workflow: CaptureWorkflow {
            output,
            copy_on_save: args.copy_on_save,
            early_exit: args.early_exit,
        },
        app_id,
        is_capture,
        enable_logs: args.enable_logs,
    }))
}

fn wants_stdin(args: &Arguments) -> bool {
    args.stdin || args.file.as_deref() == Some(Path::new("-"))
}

fn resolve_cli_path(path: &Path) -> anyhow::Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(normalize_cur_dir(abs))
}

fn normalize_cur_dir(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        if component != Component::CurDir {
            out.push(component.as_os_str());
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

fn read_bounded_stdin<R: Read>(mut reader: R, limit: usize) -> anyhow::Result<Option<Arc<[u8]>>> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if out.len().saturating_add(count) > limit {
            anyhow::bail!("stdin image payload exceeds {limit} byte limit");
        }
        out.extend_from_slice(&chunk[..count]);
    }
    if out.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::from(out)))
}
