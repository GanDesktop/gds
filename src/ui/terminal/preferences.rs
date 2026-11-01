use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted by the owning application, not by the terminal library.
/// Shell changes affect future sessions; visual changes apply to existing views.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, crate::ConfigEditor)]
#[serde(default)]
#[config(validate = "Self::validate")]
pub struct Preferences {
    #[setting(
        label = "gds-terminal-settings-shell",
        help = "gds-terminal-settings-shell-hint"
    )]
    pub shell: Option<PathBuf>,
    // Literal arguments remain available in configuration; never parse a shell command string.
    pub shell_args: Vec<String>,
    #[setting(label = "gds-terminal-settings-login")]
    pub login_shell: bool,
    #[setting(label = "gds-terminal-settings-font", kind = "font")]
    pub font_family: String,
    #[setting(
        label = "gds-terminal-settings-font-size",
        min = 8.,
        max = 48.,
        unit = " px"
    )]
    pub font_size: f32,
    #[setting(
        label = "gds-terminal-settings-scrollback",
        min = 0,
        max = 100_000,
        step = 1000
    )]
    pub scrollback_lines: usize,
    #[setting(label = "gds-terminal-settings-cursor-blink")]
    pub cursor_blink: bool,
    #[setting(label = "gds-terminal-settings-alt-escape")]
    pub alt_sends_escape: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            shell: None,
            shell_args: Vec::new(),
            login_shell: false,
            font_family: "monospace".into(),
            font_size: 14.,
            scrollback_lines: 10_000,
            cursor_blink: true,
            alt_sends_escape: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Terminal font size must be finite and between 8 and 48 px")]
    FontSize,
    #[error("Terminal scrollback must not exceed 100,000 lines")]
    Scrollback,
    #[error("Shell executable must be an absolute path, or use the default")]
    ShellPath,
    #[error("Terminal font family must not be empty")]
    FontFamily,
}

impl Preferences {
    /// # Errors
    /// Rejects invalid numeric ranges, empty fonts, and relative shell paths.
    pub fn validate(&self) -> Result<(), Error> {
        if !self.font_size.is_finite() || !(8. ..=48.).contains(&self.font_size) {
            return Err(Error::FontSize);
        }
        if self.scrollback_lines > 100_000 {
            return Err(Error::Scrollback);
        }
        if self.shell.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(Error::ShellPath);
        }
        if self.font_family.trim().is_empty() {
            return Err(Error::FontFamily);
        }
        Ok(())
    }
    pub(crate) fn normalized(mut self) -> Self {
        if !self.font_size.is_finite() {
            self.font_size = Self::default().font_size;
        }
        self.font_size = self.font_size.clamp(8., 48.);
        if self.font_family.trim().is_empty() {
            self.font_family = Self::default().font_family;
        }
        self.scrollback_lines = self.scrollback_lines.min(100_000);
        self
    }
}

#[cfg(test)]
mod tests;
