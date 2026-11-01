//! Translate GPUI events into the service's terminal input protocol.
use crate::service::terminal::input;
use alacritty_terminal::term::TermMode;
use gpui_kit::{Keystroke, Modifiers};
pub(super) use input::paste_bytes;

fn modifiers(value: Modifiers) -> input::Modifiers {
    input::Modifiers {
        shift: value.shift,
        control: value.control,
        alt: value.alt,
        platform: value.platform,
        function: value.function,
    }
}
pub(super) fn key_bytes(key: &Keystroke, mode: TermMode, alt_escape: bool) -> Option<Vec<u8>> {
    input::key_bytes(
        &input::Keystroke {
            key: &key.key,
            key_char: key.key_char.as_deref(),
            modifiers: modifiers(key.modifiers),
        },
        mode,
        alt_escape,
    )
}
pub(super) fn mouse_bytes(
    button: u8,
    column: usize,
    row: usize,
    released: bool,
    value: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    input::mouse_bytes(button, column, row, released, modifiers(value), mode)
}
