//! Explicit shell-directory commands. Navigation must never call this implicitly.
use std::path::Path;

use super::session::Error;

pub(super) fn command(path: &Path) -> Result<Vec<u8>, Error> {
    let Some(path_text) = path.to_str() else {
        return Err(Error::InvalidDirectory(path.to_owned()));
    };
    // Control bytes are interpreted by terminal line disciplines/editors before
    // shell quoting. Refuse them rather than risk submitting extra commands.
    if !path.is_absolute() || path_text.chars().any(char::is_control) || !path.is_dir() {
        return Err(Error::InvalidDirectory(path.to_owned()));
    }
    let mut command = String::from("cd -- '");
    for character in path_text.chars() {
        match character {
            '\'' => command.push_str("'\\''"),
            // Fish interprets backslashes inside single quotes, unlike POSIX sh.
            // Quote them outside the string so the same command works in both.
            '\\' => command.push_str("'\\\\'"),
            other => command.push(other),
        }
    }
    command.push_str("'\r");
    Ok(command.into_bytes())
}

pub(super) fn supported_shell(shell: &Path) -> bool {
    matches!(
        shell.file_name().and_then(|name| name.to_str()),
        Some("sh" | "bash" | "dash" | "ash" | "zsh" | "ksh" | "mksh" | "yash" | "fish")
    )
}

#[cfg(test)]
mod tests;
