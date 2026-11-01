//! Effective bindings and their shared visual and text presentation.
mod badge;
mod hint;
mod keycap;

pub use badge::ShortcutBadge;
pub use hint::{ShortcutHint, command_tooltip, shortcut_tooltip};

use crate::ui::actions::Command;
use gpui_kit::{prelude::*, *};
use std::rc::Rc;
const KEY_GAP: f32 = 4.;

/// An action together with the key context whose binding applies to it.
#[derive(Clone)]
pub struct Shortcut {
    action: Rc<dyn Action>,
    context: Option<&'static str>,
    prefix: Option<PrefixTarget>,
}

/// A shortcut's entry action when it is reached through a keyboard prefix.
#[derive(Clone)]
pub struct PrefixTarget {
    entry: Rc<Shortcut>,
}

impl PrefixTarget {
    pub fn entry(&self) -> &Shortcut {
        &self.entry
    }
}
impl PartialEq for PrefixTarget {
    fn eq(&self, other: &Self) -> bool {
        self.entry.same_scope(&other.entry)
    }
}
impl Eq for PrefixTarget {}

impl Shortcut {
    pub fn new(action: Box<dyn Action>, context: Option<&'static str>) -> Self {
        Self {
            action: Rc::from(action),
            context,
            prefix: None,
        }
    }

    /// The same key names used by badges and held-modifier labels.
    pub fn label(&self, window: &Window) -> Option<String> {
        self.binding(window)
            .map(|binding| Self::binding_label(&binding))
    }

    pub fn binding_label(binding: &KeyBinding) -> String {
        binding
            .keystrokes()
            .iter()
            .map(|stroke| {
                stroke_labels(stroke.as_keystroke(), None)
                    .iter()
                    .map(AsRef::<str>::as_ref)
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn hint(&self, label: &str, window: &Window) -> String {
        self.label(window)
            .map_or_else(|| label.to_owned(), |keys| format!("{label} · {keys}"))
    }

    pub fn context(&self) -> Option<&'static str> {
        self.context
    }

    pub fn action(&self) -> &dyn Action {
        &*self.action
    }

    /// Mark this as a page key that only runs behind the prefix `entry` arms.
    pub fn via_prefix(mut self, entry: impl Into<Shortcut>) -> Self {
        self.prefix = Some(PrefixTarget {
            entry: Rc::new(entry.into()),
        });
        self
    }

    /// The prefix this key belongs behind, for a page key; `None` for an
    /// ordinary shortcut that runs from the plain layer itself.
    pub fn prefix(&self) -> Option<PrefixTarget> {
        self.prefix.clone()
    }

    /// Whether both shortcuts name the same action in the same scope, so a
    /// cached presentation does not have to be rebuilt.
    pub fn same_scope(&self, other: &Self) -> bool {
        self.context == other.context
            && self.prefix == other.prefix
            && self.action.partial_eq(&*other.action)
    }

    /// The highest-precedence binding currently in effect for this action.
    pub fn binding(&self, window: &Window) -> Option<KeyBinding> {
        let context = self
            .context
            .map(|value| KeyContext::parse(value).expect("bundled action contexts are valid"))
            .unwrap_or_default();
        window.highest_precedence_binding_for_action_in_context(&*self.action, context)
    }
}

impl From<Command> for Shortcut {
    fn from(command: Command) -> Self {
        Self::new(command.action(), command.context())
    }
}

/// The modifier group a press reveals hint labels for.
///
/// The two groups never mix. Ctrl+Alt (what AltGr reports) selects neither, so
/// such a press shows no labels instead of lighting the other group's keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeldModifier {
    Ctrl,
    Alt,
}

impl HeldModifier {
    /// The group the current modifier state asks for, if any.
    pub fn from_modifiers(modifiers: Modifiers) -> Option<Self> {
        if modifiers.platform {
            return None;
        }
        match (modifiers.control, modifiers.alt) {
            (true, false) => Some(Self::Ctrl),
            (false, true) => Some(Self::Alt),
            _ => None,
        }
    }

    /// Whether a stroke carries this group's modifier.
    pub fn matches(self, modifiers: Modifiers) -> bool {
        match self {
            Self::Ctrl => modifiers.control,
            Self::Alt => modifiers.alt,
        }
    }
}

fn key_labels(binding: Option<&KeyBinding>) -> Vec<SharedString> {
    key_labels_omitting(binding, None)
}

/// The key names of a binding's first stroke, minus the prefix of `held`.
fn key_labels_omitting(
    binding: Option<&KeyBinding>,
    held: Option<HeldModifier>,
) -> Vec<SharedString> {
    binding
        .and_then(|binding| binding.keystrokes().first())
        .map(|stroke| stroke_labels(stroke.as_keystroke(), held))
        .unwrap_or_default()
}

fn stroke_labels(stroke: &Keystroke, held: Option<HeldModifier>) -> Vec<SharedString> {
    let mut labels = Vec::new();
    if stroke.modifiers.control && held != Some(HeldModifier::Ctrl) {
        labels.push("Ctrl".into());
    }
    if stroke.modifiers.alt && held != Some(HeldModifier::Alt) {
        labels.push("Alt".into());
    }
    if stroke.modifiers.shift {
        labels.push("Shift".into());
    }
    if stroke.modifiers.platform {
        labels.push("Super".into());
    }
    labels.push(match stroke.key.as_str() {
        "left" => "←".into(),
        "right" => "→".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "enter" => "Enter".into(),
        "escape" => "Esc".into(),
        "tab" => "Tab".into(),
        key => key.to_uppercase().into(),
    });
    labels
}
