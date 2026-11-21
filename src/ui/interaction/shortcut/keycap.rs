use zgui::prelude::*;
use zgui::vocab::{Key, Modifiers, NamedKey};
use zgui::{component, css, view};

use super::HeldModifier;

pub(super) const SHEET: &str = css!(
    r#"
    .gds-keycap {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        height: 22px;
        min-width: 22px;
        padding: 0 6px;
        border-radius: 4px;
        border: 1px solid var(--gds-border);
        background: var(--gds-surface);
        color: var(--gds-muted-foreground);
        font-family: var(--gds-font-sans);
        font-size: calc(12px * var(--gds-font-scale, 1));
        font-weight: 400;
        line-height: 1;
        white-space: nowrap;
        flex-shrink: 0;
        user-select: none;
    }
    "#
);

pub(super) fn install_keycap_stylesheet() {
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-keycap", SHEET);
    }
}

/// First-party GDS `22px` physical keycap pill (`shortcuts.md` §3).
#[component]
pub(super) fn Keycap(label: String) -> impl IntoView {
    install_keycap_stylesheet();
    view! {
        text(class = "gds-keycap") {{label}}
    }
}

/// Formats a single [`Key`] into its user-facing keycap label.
pub(super) fn display_key(key: &Key) -> String {
    match key {
        Key::Named(named) => match *named {
            NamedKey::Escape => "Esc".to_owned(),
            NamedKey::Enter => "Enter".to_owned(),
            NamedKey::Space => "Space".to_owned(),
            NamedKey::Tab => "Tab".to_owned(),
            NamedKey::Backspace => "Backspace".to_owned(),
            NamedKey::Delete => "Del".to_owned(),
            NamedKey::PageUp => "PgUp".to_owned(),
            NamedKey::PageDown => "PgDn".to_owned(),
            NamedKey::ArrowUp => "↑".to_owned(),
            NamedKey::ArrowDown => "↓".to_owned(),
            NamedKey::ArrowLeft => "←".to_owned(),
            NamedKey::ArrowRight => "→".to_owned(),
            NamedKey::ContextMenu => "Menu".to_owned(),
            NamedKey::Control => "Ctrl".to_owned(),
            NamedKey::Alt => "Alt".to_owned(),
            NamedKey::Shift => "Shift".to_owned(),
            NamedKey::Meta | NamedKey::Super => {
                if cfg!(target_os = "macos") {
                    "Cmd".to_owned()
                } else {
                    "Super".to_owned()
                }
            }
            other => other.as_str().to_owned(),
        },
        Key::Character(text) | Key::Other(text) => {
            if text.as_str() == " " {
                "Space".to_owned()
            } else {
                text.as_str().to_uppercase()
            }
        }
        Key::Dead(Some(ch)) => ch.to_uppercase().to_string(),
        Key::Dead(None) | Key::Unidentified => "?".to_owned(),
        _ => "?".to_owned(),
    }
}

/// Formats modifier keys in canonical `Ctrl → Alt → Shift → Super/Cmd` display order,
/// omitting `held` when rendering compact modifier-hold badges.
pub(super) fn display_modifiers_omitting(
    modifiers: Modifiers,
    held: Option<HeldModifier>,
) -> Vec<String> {
    let mut tokens = Vec::new();
    if modifiers.control() && held != Some(HeldModifier::Ctrl) {
        tokens.push("Ctrl".to_owned());
    }
    if modifiers.alt() && held != Some(HeldModifier::Alt) {
        tokens.push("Alt".to_owned());
    }
    if modifiers.shift() {
        tokens.push("Shift".to_owned());
    }
    if modifiers.meta() {
        tokens.push(if cfg!(target_os = "macos") {
            "Cmd".to_owned()
        } else {
            "Super".to_owned()
        });
    }
    tokens
}

/// Formats modifier keys in canonical cross-platform display order.
pub(super) fn display_modifiers(modifiers: Modifiers) -> Vec<String> {
    display_modifiers_omitting(modifiers, None)
}

pub(super) fn is_space_key(key: &Key) -> bool {
    matches!(key, Key::Named(NamedKey::Space))
        || matches!(key, Key::Character(text) if text.as_str() == " ")
}

pub(super) fn parse_key_token(token: &str) -> Option<Key> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let named = match lower.as_str() {
        "esc" | "escape" => Some(NamedKey::Escape),
        "enter" | "return" => Some(NamedKey::Enter),
        "tab" => Some(NamedKey::Tab),
        "space" | "spacebar" => Some(NamedKey::Space),
        "backspace" | "bs" => Some(NamedKey::Backspace),
        "del" | "delete" => Some(NamedKey::Delete),
        "ins" | "insert" => Some(NamedKey::Insert),
        "home" => Some(NamedKey::Home),
        "end" => Some(NamedKey::End),
        "up" | "arrowup" => Some(NamedKey::ArrowUp),
        "down" | "arrowdown" => Some(NamedKey::ArrowDown),
        "left" | "arrowleft" => Some(NamedKey::ArrowLeft),
        "right" | "arrowright" => Some(NamedKey::ArrowRight),
        "pgup" | "pageup" => Some(NamedKey::PageUp),
        "pgdn" | "pagedown" => Some(NamedKey::PageDown),
        "menu" | "contextmenu" => Some(NamedKey::ContextMenu),
        _ => None,
    };
    if let Some(named) = named {
        return Some(Key::Named(named));
    }

    if let Ok(named) = trimmed.parse::<NamedKey>() {
        return Some(Key::Named(named));
    }
    if lower.starts_with('f')
        && lower.len() <= 3
        && lower[1..].chars().all(|c| c.is_ascii_digit())
        && let Ok(named) = lower.to_ascii_uppercase().parse::<NamedKey>()
    {
        return Some(Key::Named(named));
    }

    Some(Key::character(lower))
}
