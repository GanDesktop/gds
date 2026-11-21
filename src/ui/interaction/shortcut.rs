//! Effective keybindings and their shared badge and tooltip presentation.

mod badge;
mod hint;
mod keycap;

pub use badge::{ShortcutBadge, ShortcutBadgeProps};
pub(crate) use hint::install_shortcut_hint_stylesheet;
pub use hint::{ShortcutHint, ShortcutHintProps, command_tooltip, shortcut_tooltip};

use zgui::vocab::{Key, KeyEvent, Modifiers};

use crate::ui::actions::Command;

/// The modifier group a press reveals keytip labels for (`shortcuts.md` §3).
///
/// Holding `Shift` alongside `Ctrl` or `Alt` (`Ctrl+Shift+N`, `Alt+Shift+X`) stays in that group,
/// while `Ctrl+Alt` (such as `AltGr`), `Meta`/`Super`, or `Shift` alone selects neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeldModifier {
    Ctrl,
    Alt,
}

impl HeldModifier {
    /// Returns the active modifier group for `modifiers`, if exactly one of `Ctrl` or `Alt` is
    /// held without `Meta`/`Super`.
    #[must_use]
    pub const fn from_modifiers(modifiers: Modifiers) -> Option<Self> {
        let held = modifiers.held();
        if held.meta() {
            return None;
        }
        match (held.control(), held.alt()) {
            (true, false) => Some(Self::Ctrl),
            (false, true) => Some(Self::Alt),
            _ => None,
        }
    }

    /// Returns the primary [`Modifiers`] bit for this modifier group.
    #[must_use]
    pub const fn as_modifier(self) -> Modifiers {
        match self {
            Self::Ctrl => Modifiers::CONTROL,
            Self::Alt => Modifiers::ALT,
        }
    }

    /// Returns `true` when `modifiers` belongs to this held modifier group (allowing `Shift`,
    /// rejecting `Ctrl+Alt` and `Meta`).
    #[must_use]
    pub const fn matches(self, modifiers: Modifiers) -> bool {
        let held = modifiers.held();
        if held.meta() || (held.control() && held.alt()) {
            return false;
        }
        match self {
            Self::Ctrl => held.control(),
            Self::Alt => held.alt(),
        }
    }

    /// Returns the canonical display label for this modifier group.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
        }
    }
}

/// Platform-aware keyboard shortcut binding.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl Shortcut {
    /// Creates a shortcut from a key and modifier set.
    #[must_use]
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    /// Returns the default shortcut bound to `command`.
    #[must_use]
    pub fn command(command: Command) -> Self {
        command.default_stroke()
    }

    /// Parses a human-readable shortcut specification such as `"Ctrl+Shift+C"` or `"Alt+N"`.
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parts = split_spec_parts(trimmed)?;
        let (key_part, mod_parts) = parts.split_last()?;

        let mut modifiers = Modifiers::NONE;
        for part in mod_parts {
            let modifier = parse_modifier(part)?;
            modifiers |= modifier;
        }

        if let Some(only_modifier) = parse_modifier(key_part) {
            let _ = only_modifier;
            return None;
        }

        let key = keycap::parse_key_token(key_part)?;
        Some(Self { key, modifiers })
    }

    /// Returns `true` when `key` and `modifiers` match this shortcut (ignoring lock bits).
    #[must_use]
    pub fn matches(&self, key: &Key, modifiers: Modifiers) -> bool {
        if modifiers.held().bits() != self.modifiers.held().bits() {
            return false;
        }
        keys_match(&self.key, key)
    }

    /// Returns `true` when a non-repeating key event matches this shortcut, checking both the
    /// shifted key and `key_without_modifiers` when modifiers are required.
    #[must_use]
    pub fn matches_event(&self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        if event.repeat || modifiers.held().bits() != self.modifiers.held().bits() {
            return false;
        }
        keys_match(&self.key, &event.key)
            || (!self.modifiers.held().is_empty()
                && keys_match(&self.key, &event.key_without_modifiers))
    }

    /// Returns `true` when dispatching this shortcut globally must respect `!focus_is_text_entry()`.
    ///
    /// Unmodified and Shift-only bindings yield to focused text inputs so typing and editing keys
    /// are never intercepted.
    #[must_use]
    pub const fn requires_non_text_focus(&self) -> bool {
        let held = self.modifiers.held();
        !(held.control() || held.alt() || held.meta())
    }

    /// Returns the individual keycap strings to display for this shortcut (`Ctrl → Alt → Shift → Key`).
    #[must_use]
    pub fn display_tokens(&self) -> Vec<String> {
        let mut tokens = keycap::display_modifiers(self.modifiers);
        tokens.push(keycap::display_key(&self.key));
        tokens
    }

    /// Returns the keycap tokens with `held` omitted (`Ctrl+Shift+Tab` under `Ctrl` -> `["Shift", "Tab"]`).
    #[must_use]
    pub fn display_tokens_omitting(&self, held: Option<HeldModifier>) -> Vec<String> {
        let mut tokens = keycap::display_modifiers_omitting(self.modifiers, held);
        tokens.push(keycap::display_key(&self.key));
        tokens
    }

    /// Returns the compact hold-to-reveal suffix string when this shortcut belongs to `held`
    /// (`Ctrl+R` -> `Some("R")`, `Ctrl+Shift+Tab` -> `Some("Shift+Tab")`, `Alt+A` -> `Some("A")`,
    /// `F2` -> `None`).
    #[must_use]
    pub fn compact_label(&self, held: HeldModifier) -> Option<String> {
        if !held.matches(self.modifiers) {
            return None;
        }
        let tokens = self.display_tokens_omitting(Some(held));
        (!tokens.is_empty()).then(|| tokens.join("+"))
    }

    /// Formats this shortcut as a joined keycap string such as `"Alt+Shift+X"`.
    #[must_use]
    pub fn label(&self) -> String {
        self.display_tokens().join("+")
    }

    /// Appends this shortcut's label to `label` using GDS tooltip formatting.
    #[must_use]
    pub fn hint(&self, label: &str) -> String {
        format!("{label} · {}", self.label())
    }
}

impl From<Command> for Shortcut {
    fn from(command: Command) -> Self {
        Self::command(command)
    }
}

/// Ordered sequence of one or more [`Shortcut`] strokes representing a key chord.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub strokes: Vec<Shortcut>,
}

#[cfg(test)]
impl KeyChord {
    /// Creates a chord from one or more strokes, returning `None` if `strokes` is empty.
    #[must_use]
    pub fn new(strokes: Vec<Shortcut>) -> Option<Self> {
        if strokes.is_empty() {
            None
        } else {
            Some(Self { strokes })
        }
    }

    /// Creates a single-stroke [`KeyChord`].
    #[must_use]
    pub fn single(stroke: Shortcut) -> Self {
        Self {
            strokes: vec![stroke],
        }
    }

    /// Parses a whitespace-separated sequence of shortcuts such as `"Alt+N X"` or `"g d"`.
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let mut strokes = Vec::new();
        for token in spec.split_whitespace() {
            strokes.push(Shortcut::parse(token)?);
        }
        Self::new(strokes)
    }

    /// Formats this chord as space-separated shortcut labels.
    #[must_use]
    pub fn label(&self) -> String {
        self.strokes
            .iter()
            .map(Shortcut::label)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Appends this chord's label to `label` using GDS tooltip formatting.
    #[must_use]
    pub fn hint(&self, label: &str) -> String {
        format!("{label} · {}", self.label())
    }
}

#[cfg(test)]
impl From<Shortcut> for KeyChord {
    fn from(stroke: Shortcut) -> Self {
        Self::single(stroke)
    }
}

#[cfg(test)]
impl From<Command> for KeyChord {
    fn from(command: Command) -> Self {
        Self::single(Shortcut::command(command))
    }
}

fn split_spec_parts(spec: &str) -> Option<Vec<&str>> {
    if spec == "+" || spec == "-" {
        return Some(vec![spec]);
    }
    if let Some(prefix) = spec.strip_suffix("++") {
        let mut parts: Vec<&str> = prefix
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        parts.push("+");
        return Some(parts);
    }
    if spec.contains('+') {
        if spec.ends_with('+') {
            return None;
        }
        let parts: Vec<&str> = spec.split('+').map(str::trim).collect();
        if parts.iter().any(|part| part.is_empty()) {
            return None;
        }
        return Some(parts);
    }
    if spec.contains('-') && !spec.ends_with('-') {
        let candidate: Vec<&str> = spec.split('-').map(str::trim).collect();
        if candidate.len() > 1
            && candidate[..candidate.len() - 1]
                .iter()
                .all(|part| parse_modifier(part).is_some())
        {
            return Some(candidate);
        }
    }
    Some(vec![spec])
}

fn parse_modifier(token: &str) -> Option<Modifiers> {
    match token.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(Modifiers::CONTROL),
        "alt" | "option" => Some(Modifiers::ALT),
        "shift" => Some(Modifiers::SHIFT),
        "super" | "meta" | "cmd" | "win" => Some(Modifiers::META),
        "primary" => Some(Modifiers::PRIMARY),
        _ => None,
    }
}

fn keys_match(expected: &Key, actual: &Key) -> bool {
    if keycap::is_space_key(expected) && keycap::is_space_key(actual) {
        return true;
    }
    match (expected, actual) {
        (Key::Character(a), Key::Character(b)) => {
            let a_str = a.as_str();
            let b_str = b.as_str();
            a_str.eq_ignore_ascii_case(b_str) || a_str.to_lowercase() == b_str.to_lowercase()
        }
        _ => expected == actual,
    }
}

#[cfg(test)]
mod tests {
    use zgui::vocab::{Key, Modifiers, NamedKey};

    use super::{HeldModifier, KeyChord, Shortcut, ShortcutBadge};

    #[test]
    fn shortcut_parses_modifiers_named_keys_and_matches_space() {
        let copy = Shortcut::parse("Ctrl+Shift+C").expect("Ctrl+Shift+C");
        assert!(copy.matches(&Key::character("c"), Modifiers::CONTROL | Modifiers::SHIFT));
        assert!(!copy.matches(&Key::character("c"), Modifiers::CONTROL));
        assert_eq!(copy.display_tokens(), vec!["Ctrl", "Shift", "C"]);

        let dash = Shortcut::parse("alt-shift-x").expect("alt-shift-x");
        assert!(dash.matches(&Key::character("X"), Modifiers::ALT | Modifiers::SHIFT));
        assert_eq!(dash.display_tokens(), vec!["Alt", "Shift", "X"]);

        let space = Shortcut::parse("Space").expect("Space");
        assert!(space.matches(&Key::Named(NamedKey::Space), Modifiers::NONE));
        assert!(space.matches(&Key::character(" "), Modifiers::NONE));
    }

    #[test]
    fn held_modifier_retains_shift_and_rejects_mixed_or_meta() {
        assert_eq!(
            HeldModifier::from_modifiers(Modifiers::CONTROL),
            Some(HeldModifier::Ctrl)
        );
        assert_eq!(
            HeldModifier::from_modifiers(Modifiers::ALT),
            Some(HeldModifier::Alt)
        );
        assert_eq!(
            HeldModifier::from_modifiers(Modifiers::CONTROL | Modifiers::SHIFT),
            Some(HeldModifier::Ctrl)
        );
        assert_eq!(
            HeldModifier::from_modifiers(Modifiers::ALT | Modifiers::SHIFT),
            Some(HeldModifier::Alt)
        );
        assert_eq!(
            HeldModifier::from_modifiers(Modifiers::CONTROL | Modifiers::ALT),
            None
        );
        assert_eq!(HeldModifier::from_modifiers(Modifiers::META), None);
        assert_eq!(HeldModifier::from_modifiers(Modifiers::SHIFT), None);
        assert_eq!(HeldModifier::from_modifiers(Modifiers::NONE), None);

        let chord = KeyChord::parse("Alt+N Esc").expect("two-stroke chord");
        assert_eq!(chord.strokes.len(), 2);
        assert_eq!(chord.label(), "Alt+N Esc");
        assert_eq!(chord.hint("Focus"), "Focus · Alt+N Esc");
    }

    #[test]
    fn compact_labels_strip_held_modifier_and_gesture_badges_keep_tokens() {
        let ctrl_r = Shortcut::parse("Ctrl+R").expect("Ctrl+R");
        let ctrl_shift_tab = Shortcut::parse("Ctrl+Shift+Tab").expect("Ctrl+Shift+Tab");
        let alt_a = Shortcut::parse("Alt+A").expect("Alt+A");
        let alt_shift_x = Shortcut::parse("Alt+Shift+X").expect("Alt+Shift+X");
        let f2 = Shortcut::parse("F2").expect("F2");

        assert_eq!(
            ShortcutBadge::path_label(&ctrl_r, HeldModifier::Ctrl).as_deref(),
            Some("R")
        );
        assert_eq!(ShortcutBadge::path_label(&ctrl_r, HeldModifier::Alt), None);
        assert_eq!(
            ShortcutBadge::path_label(&ctrl_shift_tab, HeldModifier::Ctrl).as_deref(),
            Some("Shift+Tab")
        );
        assert_eq!(
            ShortcutBadge::path_label(&alt_a, HeldModifier::Alt).as_deref(),
            Some("A")
        );
        assert_eq!(
            ShortcutBadge::path_label(&alt_shift_x, HeldModifier::Alt).as_deref(),
            Some("Shift+X")
        );
        assert_eq!(ShortcutBadge::path_label(&f2, HeldModifier::Ctrl), None);

        let single_gesture = ShortcutBadge::gesture("Middle-click");
        assert_eq!(single_gesture.labels(), ["Middle-click"]);

        let chord_gesture = ShortcutBadge::gesture(["Ctrl", "Click"]);
        assert_eq!(chord_gesture.labels(), ["Ctrl", "Click"]);
    }
}
