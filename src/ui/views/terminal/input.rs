//! Translate zgui input events into the terminal service's PTY byte protocol.

use alacritty_terminal::term::TermMode;
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey};

use crate::service::terminal::input;

fn to_service_modifiers(value: Modifiers) -> input::Modifiers {
    input::Modifiers {
        shift: value.shift(),
        control: value.control(),
        alt: value.alt(),
        platform: value.meta(),
        function: false,
    }
}

fn named_key_name(key: NamedKey) -> Option<&'static str> {
    Some(match key {
        NamedKey::Enter => "enter",
        NamedKey::Tab => "tab",
        NamedKey::Backspace => "backspace",
        NamedKey::Escape => "escape",
        NamedKey::Space => "space",
        NamedKey::ArrowUp => "up",
        NamedKey::ArrowDown => "down",
        NamedKey::ArrowLeft => "left",
        NamedKey::ArrowRight => "right",
        NamedKey::Home => "home",
        NamedKey::End => "end",
        NamedKey::Insert => "insert",
        NamedKey::Delete => "delete",
        NamedKey::PageUp => "pageup",
        NamedKey::PageDown => "pagedown",
        NamedKey::F1 => "f1",
        NamedKey::F2 => "f2",
        NamedKey::F3 => "f3",
        NamedKey::F4 => "f4",
        NamedKey::F5 => "f5",
        NamedKey::F6 => "f6",
        NamedKey::F7 => "f7",
        NamedKey::F8 => "f8",
        NamedKey::F9 => "f9",
        NamedKey::F10 => "f10",
        NamedKey::F11 => "f11",
        NamedKey::F12 => "f12",
        _ => return None,
    })
}

fn is_non_control_text(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

fn key_parts<'a>(key: &'a Key, fallback: Option<&'a Key>) -> Option<(&'a str, Option<&'a str>)> {
    match key {
        Key::Named(named) => named_key_name(*named).map(|name| (name, None)),
        Key::Character(text) if text.as_str() == " " => Some(("space", Some(" "))),
        Key::Character(text) if is_non_control_text(text.as_str()) => {
            Some((text.as_str(), Some(text.as_str())))
        }
        _ => fallback.and_then(|base| key_parts(base, None)),
    }
}

/// Encode a high-level `zgui` key press and modifier set using default terminal mode.
#[cfg(test)]
#[must_use]
fn encode_key(key: &Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    let (key_name, key_char) = key_parts(key, None)?;
    if let Some(bytes) = input::key_bytes(
        &input::Keystroke {
            key: key_name,
            key_char,
            modifiers: to_service_modifiers(modifiers),
        },
        TermMode::empty(),
        true,
    ) {
        return Some(bytes);
    }
    plain_text_bytes(key, modifiers)
}

fn plain_text_bytes(key: &Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    if modifiers.control() || modifiers.alt() || modifiers.meta() {
        return None;
    }
    let text = key.inserted_text()?;
    is_non_control_text(text).then(|| text.as_bytes().to_vec())
}

pub(super) fn key_bytes(
    event: &KeyEvent,
    modifiers: Modifiers,
    mode: TermMode,
    alt_escape: bool,
) -> Option<Vec<u8>> {
    let primary = if modifiers.control() && matches!(event.key, Key::Character(_)) {
        // Prefer the un-controlified base key so Ctrl+Shift+[ or Ctrl+a maps cleanly.
        match &event.key_without_modifiers {
            Key::Character(text) if is_non_control_text(text.as_str()) => {
                &event.key_without_modifiers
            }
            _ => &event.key,
        }
    } else {
        &event.key
    };
    let (key_name, key_char) = key_parts(primary, Some(&event.key_without_modifiers))?;
    if let Some(bytes) = input::key_bytes(
        &input::Keystroke {
            key: key_name,
            key_char,
            modifiers: to_service_modifiers(modifiers),
        },
        mode,
        alt_escape,
    ) {
        return Some(bytes);
    }
    plain_text_bytes(&event.key, modifiers)
}

pub(super) fn mouse_bytes(
    button: u8,
    column: usize,
    row: usize,
    released: bool,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    input::mouse_bytes(
        button,
        column,
        row,
        released,
        to_service_modifiers(modifiers),
        mode,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use zgui::vocab::{KeyCode, PhysicalKey};

    fn key_event(key: Key, unmodified: Key) -> KeyEvent {
        let mut event = KeyEvent::character("");
        event.key = key;
        event.key_without_modifiers = unmodified;
        event.physical = PhysicalKey::Code(KeyCode::KeyA);
        event
    }

    #[test]
    fn encodes_named_keys_and_modifiers() {
        assert_eq!(
            encode_key(&Key::Named(NamedKey::Enter), Modifiers::NONE),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_key(&Key::Named(NamedKey::Tab), Modifiers::NONE),
            Some(b"\t".to_vec())
        );
        assert_eq!(
            encode_key(&Key::Named(NamedKey::Tab), Modifiers::SHIFT),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            encode_key(&Key::Character("c".into()), Modifiers::CONTROL),
            Some(vec![0x03])
        );
        assert_eq!(
            encode_key(&Key::Character(" ".into()), Modifiers::CONTROL),
            Some(vec![0x00])
        );
        assert_eq!(
            encode_key(&Key::Character("x".into()), Modifiers::ALT),
            Some(b"\x1bx".to_vec())
        );
        assert_eq!(
            encode_key(&Key::Character("a".into()), Modifiers::NONE),
            Some(b"a".to_vec())
        );
        assert_eq!(
            encode_key(&Key::Character("A".into()), Modifiers::SHIFT),
            Some(b"A".to_vec())
        );
        assert_eq!(
            encode_key(&Key::Named(NamedKey::Space), Modifiers::NONE),
            Some(b" ".to_vec())
        );
    }

    #[test]
    fn respects_app_cursor_and_control_fallback() {
        let up = key_event(Key::Named(NamedKey::ArrowUp), Key::Named(NamedKey::ArrowUp));
        assert_eq!(
            key_bytes(&up, Modifiers::NONE, TermMode::empty(), true),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(&up, Modifiers::NONE, TermMode::APP_CURSOR, true),
            Some(b"\x1bOA".to_vec())
        );

        let ctrl_c_raw = key_event(Key::Character("\x03".into()), Key::Character("c".into()));
        assert_eq!(
            key_bytes(&ctrl_c_raw, Modifiers::CONTROL, TermMode::empty(), true),
            Some(vec![0x03])
        );
    }

    #[test]
    fn encodes_mouse_reports_in_sgr_mode() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_bytes(0, 4, 2, false, Modifiers::NONE, mode),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            mouse_bytes(0, 4, 2, true, Modifiers::NONE, mode),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
    }
}
