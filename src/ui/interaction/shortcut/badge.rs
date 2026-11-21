use zgui::prelude::*;
use zgui::{component, css, view};

use super::keycap::{KeycapProps, install_keycap_stylesheet};
use super::{HeldModifier, Shortcut};

const SHEET: &str = css!(
    r#"
    .gds-shortcut-badge {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        flex-wrap: nowrap;
        flex-shrink: 0;
    }
    "#
);

fn install_badge_stylesheets() {
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-shortcut-badge", SHEET);
        install_keycap_stylesheet();
    }
}

/// Converts a single gesture label or a sequence of keycap labels into discrete keycap strings.
pub trait IntoKeycapTokens {
    /// Expands `self` into ordered keycap labels.
    fn into_keycap_tokens(self) -> Vec<String>;
}

impl IntoKeycapTokens for &str {
    fn into_keycap_tokens(self) -> Vec<String> {
        if self.is_empty() {
            Vec::new()
        } else {
            vec![self.to_owned()]
        }
    }
}

impl IntoKeycapTokens for String {
    fn into_keycap_tokens(self) -> Vec<String> {
        if self.is_empty() {
            Vec::new()
        } else {
            vec![self]
        }
    }
}

impl<S: AsRef<str>, const N: usize> IntoKeycapTokens for [S; N] {
    fn into_keycap_tokens(self) -> Vec<String> {
        self.into_iter()
            .map(|token| token.as_ref().to_owned())
            .filter(|token| !token.is_empty())
            .collect()
    }
}

impl<S: AsRef<str>> IntoKeycapTokens for &[S] {
    fn into_keycap_tokens(self) -> Vec<String> {
        self.iter()
            .map(|token| token.as_ref().to_owned())
            .filter(|token| !token.is_empty())
            .collect()
    }
}

impl<S: Into<String>> IntoKeycapTokens for Vec<S> {
    fn into_keycap_tokens(self) -> Vec<String> {
        self.into_iter()
            .map(Into::into)
            .filter(|token| !token.is_empty())
            .collect()
    }
}

/// Visual cluster of independent `22px` `.gds-keycap` pills representing a keyboard shortcut or
/// pointer gesture (`shortcuts.md` §3).
#[derive(Clone, Debug)]
pub struct ShortcutBadge {
    tokens: Vec<String>,
    class: Classes,
}

impl ShortcutBadge {
    /// Creates a keycap cluster from a keyboard [`Shortcut`].
    #[must_use]
    pub fn new(shortcut: impl Into<Shortcut>) -> Self {
        Self {
            tokens: shortcut.into().display_tokens(),
            class: Classes::default(),
        }
    }

    /// Creates a keycap cluster for pointer/mouse gestures (e.g. `"Middle-click"` or
    /// `["Ctrl", "Click"]`) that share the suite's `22px` keycap presentation in Help views.
    #[must_use]
    pub fn gesture(keys: impl IntoKeycapTokens) -> Self {
        Self {
            tokens: keys.into_keycap_tokens(),
            class: Classes::default(),
        }
    }

    /// Appends additional CSS classes to the badge container.
    #[must_use]
    pub fn with_class(mut self, class: impl Into<Classes>) -> Self {
        self.class = self.class.merged(class.into());
        self
    }

    /// Returns the discrete keycap labels rendered by this badge.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.tokens
    }

    /// Formats `shortcut` as a compact keytip string with `held` omitted (`Ctrl+R` -> `"R"`,
    /// `Ctrl+Shift+Tab` -> `"Shift+Tab"`), returning `None` when `shortcut` does not belong to
    /// `held`'s modifier group.
    #[must_use]
    pub fn path_label(shortcut: &Shortcut, held: HeldModifier) -> Option<String> {
        shortcut.compact_label(held)
    }
}

fn render_tokens(tokens: Vec<String>, class: Classes) -> AnyView {
    install_badge_stylesheets();
    AnyView::new(view! {
        row(class = "gds-shortcut-badge", class = class) {
            for entry in move || tokens.clone().into_iter().enumerate().collect::<Vec<_>>(), key = |entry: &(usize, String)| entry.0 {
                Keycap(label = entry.1)
            }
        }
    })
}

impl IntoView for ShortcutBadge {
    type Output = AnyView;

    fn into_view(self) -> Self::Output {
        render_tokens(self.tokens, self.class)
    }
}

/// Declarative keycap cluster displaying a [`Shortcut`] or explicit gesture tokens.
#[component]
pub fn ShortcutBadge(
    #[prop(optional)] shortcut: Option<Shortcut>,
    #[prop(optional)] gesture: Option<Vec<String>>,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    let tokens = match (gesture, shortcut) {
        (Some(keys), _) => keys,
        (None, Some(key)) => key.display_tokens(),
        (None, None) => Vec::new(),
    };
    render_tokens(tokens, class)
}
