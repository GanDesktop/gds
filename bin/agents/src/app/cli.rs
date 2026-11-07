//! Command-line options for the Agents frontend.

use bas::service::tls::ConnectionOptions as Target;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gds-agents", version, about = "Agents 会话与 Agent 工作区")]
pub(crate) struct Args {
    /// Enable the in-memory log collector and log viewer.
    #[arg(long)]
    pub(crate) enable_logs: bool,
    /// Override the saved connection: local, unix:///path, pipe://NAME, or tcp://HOST[:PORT].
    #[arg(long, env = "BAS_SERVER")]
    pub(crate) server: Option<Target>,
    /// Default working directory for newly created sessions.
    #[arg(long)]
    pub(crate) cwd: Option<PathBuf>,
    /// Open the minimal input popup.
    #[arg(long, conflicts_with = "window")]
    pub(crate) compact: bool,
    /// Open the workspace and thread window (the default).
    #[arg(long)]
    pub(crate) window: bool,
    /// Agents frontend settings (default: gds/app/agents/settings.json in the user config directory).
    #[arg(long, env = "GDS_AGENTS_SETTINGS_FILE")]
    pub(crate) settings_file: Option<PathBuf>,
}

#[cfg(test)]
mod tests;
