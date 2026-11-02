//! Xterm-compatible input encoding, independent of GPUI dispatch and PTY I/O.
use alacritty_terminal::term::TermMode;
/// Physical modifiers supplied by an input adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub platform: bool,
    pub function: bool,
}

/// Borrowed key data; text committed through an IME is sent separately.
#[derive(Clone, Copy, Debug)]
pub struct Keystroke<'a> {
    pub key: &'a str,
    pub key_char: Option<&'a str>,
    pub modifiers: Modifiers,
}

/// Committed text is sent separately, never duplicated by a physical key event.
pub fn key_bytes(key: &Keystroke<'_>, mode: TermMode, alt_escape: bool) -> Option<Vec<u8>> {
    let m = key.modifiers;
    if m.platform || m.function {
        return None;
    }
    let modifier = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.control);
    let modified = modifier != 1;
    let final_char = match key.key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(last) = final_char {
        return Some(
            if modified {
                format!("\x1b[1;{modifier}{last}")
            } else if mode.contains(TermMode::APP_CURSOR) {
                format!("\x1bO{last}")
            } else {
                format!("\x1b[{last}")
            }
            .into_bytes(),
        );
    }
    let numbered = match key.key {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    };
    if let Some(n) = numbered {
        return Some(
            if modified {
                format!("\x1b[{n};{modifier}~")
            } else {
                format!("\x1b[{n}~")
            }
            .into_bytes(),
        );
    }
    let function = match key.key {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    };
    if let Some(last) = function {
        return Some(
            if modified {
                format!("\x1b[1;{modifier}{last}")
            } else {
                format!("\x1bO{last}")
            }
            .into_bytes(),
        );
    }
    let mut bytes = match key.key {
        "enter" if mode.contains(TermMode::LINE_FEED_NEW_LINE) => b"\r\n".to_vec(),
        "enter" => vec![b'\r'],
        "backspace" if m.control => vec![8],
        "backspace" => vec![127],
        "escape" => vec![27],
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => vec![b'\t'],
        _ if m.control && !m.alt => {
            let c = match key.key {
                "space" | "@" | "2" => 0,
                "[" | "3" => 27,
                "\\" | "4" => 28,
                "]" | "5" => 29,
                "^" | "6" => 30,
                "_" | "-" | "7" => 31,
                "?" | "8" => 127,
                s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic() => {
                    s.as_bytes()[0].to_ascii_lowercase() - b'a' + 1
                }
                _ => return None,
            };
            vec![c]
        }
        _ if m.alt && !m.control && alt_escape => key
            .key_char
            .filter(|s| !s.is_empty())
            .map(|s| s.as_bytes().to_vec())?,
        _ => return None,
    };
    // Ctrl+Alt can be AltGr. Never turn it into an ESC-prefixed ASCII character.
    if m.alt && !m.control && alt_escape {
        bytes.insert(0, 27);
    }
    Some(bytes)
}

pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    // A pasted escape must not terminate bracketed paste and inject executable control input.
    let text = text
        .replace('\x1b', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if mode.contains(TermMode::BRACKETED_PASTE) {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.replace('\n', "\r").into_bytes()
    }
}

pub fn mouse_bytes(
    button: u8,
    column: usize,
    row: usize,
    released: bool,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let code = button
        + 4 * u8::from(modifiers.shift)
        + 8 * u8::from(modifiers.alt)
        + 16 * u8::from(modifiers.control);
    if mode.contains(TermMode::SGR_MOUSE) {
        let suffix = if released { 'm' } else { 'M' };
        Some(format!("\x1b[<{code};{};{}{suffix}", column + 1, row + 1).into_bytes())
    } else if column < 223 && row < 223 {
        Some(vec![
            27,
            b'[',
            b'M',
            32 + if released { 3 } else { code },
            33 + column as u8,
            33 + row as u8,
        ])
    } else {
        None
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
#[error("File path cannot be represented as terminal input: {0:?}")]
pub struct PathInputError(std::path::PathBuf);

/// Quote paths for POSIX shells and fish, without a line terminator.
///
/// # Errors
/// Rejects paths containing control characters or non-Unicode bytes.
pub fn path_text(paths: &[std::path::PathBuf]) -> Result<String, PathInputError> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    paths
        .iter()
        .map(|path| {
            let value = path
                .to_str()
                .filter(|value| !value.chars().any(char::is_control))
                .ok_or_else(|| PathInputError(path.clone()))?;
            let mut quoted = String::from("'");
            for ch in value.chars() {
                match ch {
                    '\'' => quoted.push_str("'\\''"),
                    '\\' => quoted.push_str("'\\\\'"),
                    other => quoted.push(other),
                }
            }
            quoted.push('\'');
            Ok(quoted)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join(" ") + " ")
}
