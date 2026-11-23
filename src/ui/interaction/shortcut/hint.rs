use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, css, view};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipTriggerProps};

use super::{Shortcut, ShortcutBadgeProps};
use crate::appearance::tokens::TOOLTIP_DELAY;
use crate::ui::actions::Command;
use crate::ui::feedback::tooltip::{TooltipTextProps, install_tooltip_stylesheet};

pub(crate) const SHEET: &str = css!(
    r#"
    .zui-tooltip .gds-shortcut-hint,
    .gds-shortcut-hint {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        min-width: 0;
        gap: 16px;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-panel-foreground);
    }

    .gds-shortcut-hint__label {
        min-width: 0;
        flex: 1 1 auto;
    }

    .gds-shortcut-hint__keys {
        display: inline-flex;
        align-items: center;
        flex-shrink: 0;
    }
    "#
);

pub(crate) fn install_shortcut_hint_stylesheet() {
    if zgui::view::current_host().is_some() {
        install_tooltip_stylesheet();
        zgui::view::install_stylesheet("gds-ui-shortcut-hint", SHEET);
    }
}

/// Tooltip wrapper displaying an action label on the left (`14px`, `--gds-panel-foreground`)
/// paired with its `22px` [`ShortcutBadge`] on the right (`gap: 16px`).
#[component]
pub fn ShortcutHint(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(optional)] shortcut: Option<Shortcut>,
    #[prop(into, default = Signal::stored_local(true))] shortcut_enabled: Signal<
        bool,
        LocalStorage,
    >,
    children: Children,
) -> impl IntoView {
    install_shortcut_hint_stylesheet();
    view! {
        Tooltip(delay = TOOLTIP_DELAY) {
            TooltipTrigger {{children.into_view_once()}}
            TooltipContent(class = "gds-tooltip-content", arrow = false) {
                row(class = "gds-shortcut-hint") {
                    box(class = "gds-shortcut-hint__label") {
                        TooltipText(text = label)
                    }
                    {{
                        let shortcut = shortcut.clone();
                        move || {
                            shortcut
                                .as_ref()
                                .filter(|_| shortcut_enabled.get())
                                .map(|key| {
                                    view! {
                                        box(class = "gds-shortcut-hint__keys") {
                                            ShortcutBadge(shortcut = key.clone())
                                        }
                                    }
                                    .into_view()
                                })
                        }
                    }}
                }
            }
        }
    }
}

/// Format a localized label with a command's default shortcut.
#[must_use]
pub fn command_tooltip(label: &str, command: Command) -> String {
    command.default_stroke().hint(label)
}

/// Format a localized label with an explicit shortcut.
#[must_use]
pub fn shortcut_tooltip(label: &str, shortcut: &Shortcut) -> String {
    shortcut.hint(label)
}
