//! Typed startup arguments; picker filters retain their domain parser.
use crate::features::pick_files::{Filter, Options};
use clap::{ArgGroup, Parser};
use std::path::PathBuf;

#[derive(Clone, Debug, Parser)]
#[command(
    name = "gds-files",
    version,
    about = "A native GPUI file browser for Linux / Niri."
)]
#[command(group(ArgGroup::new("picker_mode").args(["pick_file", "pick_directory", "portal_request"])))]
#[command(
    after_help = "Run inside a graphical Wayland or X11 session.\nPicker exit status: 0 selected, 1 cancelled, 2 output or argument failure."
)]
pub struct Arguments {
    /// Directory to open (default: home). Use -- before names starting with '-'.
    pub directory: Option<PathBuf>,
    /// Pick files, print one JSON result on stdout and exit.
    #[arg(long)]
    pick_file: bool,
    /// Pick directories and print one JSON result on stdout.
    #[arg(long)]
    pick_directory: bool,
    /// Read a structured native FileChooser request from stdin.
    #[arg(long, hide = true)]
    pub portal_request: bool,
    /// Allow selecting multiple files or directories.
    #[arg(long, requires = "picker_mode")]
    multiple: bool,
    /// Switchable filename filters, e.g. --filter 'Text:*.txt,*.md'. Repeatable.
    #[arg(long = "filter", value_name = "[LABEL:]GLOB[,GLOB...]", requires = "pick_file", conflicts_with = "pick_directory", value_parser = Filter::from_spec)]
    filters: Vec<Filter>,
    /// Return the selected file with its read-only flag set.
    #[arg(long, requires = "pick_file", conflicts_with = "pick_directory")]
    read_only: bool,
    /// Start on demand as the file-manager D-Bus service.
    #[arg(long, hide = true, conflicts_with = "picker_mode")]
    pub dbus_service: bool,
    /// Enable the in-memory log collector and log viewer.
    #[arg(long)]
    pub enable_logs: bool,
}

impl Arguments {
    pub fn is_picker(&self) -> bool {
        self.pick_file || self.pick_directory || self.portal_request
    }

    pub fn picker(&self) -> Option<Options> {
        self.is_picker().then(|| Options {
            directory: self.pick_directory,
            multiple: self.multiple,
            ..Options::new(self.filters.clone(), self.read_only)
        })
    }
}
