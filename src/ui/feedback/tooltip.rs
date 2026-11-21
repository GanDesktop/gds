//! Shared GDS tooltip wrappers around `zgui-ui::Tooltip` (`README.md` §1, `shortcuts.md` §3).

use unicode_segmentation::UnicodeSegmentation;
use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, css, view};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipProviderProps, TooltipTriggerProps};

pub use crate::appearance::tokens::TOOLTIP_DELAY;

const WORD_SEGMENT: usize = 16;

const SHEET: &str = css!(
    r#"
    .zui-tooltip,
    .zui-tooltip.gds-tooltip-content {
        --zui-surface-fill: var(--gds-panel);
        --zui-surface-ink: var(--gds-panel-foreground);
        --zui-surface-border: 1px solid var(--gds-border);
        --zui-surface-radius: 8px;
        background-color: var(--gds-panel);
        color: var(--gds-panel-foreground);
        border: 1px solid var(--gds-border);
        border-radius: 8px;
        padding: 6px 10px;
        max-width: 360px;
        max-height: 320px;
        overflow-y: auto;
        font-family: var(--gds-font-sans);
        font-size: var(--gds-font-size-small, 14px);
        font-weight: 400;
        line-height: 1.35;
        box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18);
    }

    .zui-tooltip .zui-tooltip__arrow {
        display: none;
    }

    .zui-tooltip .gds-tooltip-text,
    .gds-tooltip-text {
        color: var(--gds-panel-foreground);
        font-size: var(--gds-font-size-small, 14px);
        line-height: 1.35;
        white-space: normal;
        overflow-wrap: anywhere;
    }

    .gds-tooltip-details {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-width: 340px;
        max-height: 304px;
        overflow-y: auto;
    }

    .gds-tooltip-details__title {
        color: var(--gds-panel-foreground);
        font-size: var(--gds-font-size-small, 14px);
        font-weight: 600;
        line-height: 1.35;
        white-space: normal;
        overflow-wrap: anywhere;
    }

    .gds-tooltip-details__item {
        font-size: var(--gds-font-size-small, 14px);
        color: var(--gds-muted-foreground);
        line-height: 1.35;
        white-space: normal;
        overflow-wrap: anywhere;
    }
    "#
);

/// Inserts zero-width break opportunities (`\u{200B}`) after path/token separators (`/`, `\`, `:`,
/// `-`, `_`) AND every 16 graphemes inside unbroken non-whitespace runs (`main:tooltip.rs`).
#[must_use]
pub fn wrap_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0usize;
    for grapheme in text.graphemes(true) {
        if grapheme.chars().all(char::is_whitespace) {
            run = 0;
            out.push_str(grapheme);
            continue;
        }
        if matches!(grapheme, "/" | "\\" | ":" | "-" | "_") {
            out.push_str(grapheme);
            out.push('\u{200B}');
            run = 0;
            continue;
        }
        if run == WORD_SEGMENT {
            out.push('\u{200B}');
            run = 0;
        }
        run += 1;
        out.push_str(grapheme);
    }
    out
}

pub(crate) fn install_tooltip_stylesheet() {
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-tooltip", SHEET);
    }
}

/// Window-root tooltip configuration provider with GDS `400ms` hover delay.
#[component]
pub fn GdsTooltipProvider(children: Children) -> impl IntoView {
    install_tooltip_stylesheet();
    view! {
        TooltipProvider(delay = TOOLTIP_DELAY) {
            {children.into_view_once()}
        }
    }
}

/// Plain localized text content for a GDS tooltip bubble (`14px` `--gds-panel-foreground` with
/// CSS `overflow-wrap: anywhere` and clean AT-SPI text projection).
#[component]
pub fn TooltipText(#[prop(into)] text: Signal<String, LocalStorage>) -> impl IntoView {
    install_tooltip_stylesheet();
    view! {
        text(class = "gds-tooltip-text") {{move || text.get()}}
    }
}

/// Convenience helper pairing a trigger child with plain tooltip text using the `400ms` GDS hover
/// delay and immediate keyboard focus reveal.
#[component]
pub fn HintTooltip(
    #[prop(into)] text: Signal<String, LocalStorage>,
    #[prop(into, optional)] trigger_class: Classes,
    children: Children,
) -> impl IntoView {
    install_tooltip_stylesheet();
    view! {
        Tooltip(delay = TOOLTIP_DELAY) {
            TooltipTrigger(class = trigger_class) {{children.into_view_once()}}
            TooltipContent(class = "gds-tooltip-content", arrow = false) {
                TooltipText(text = text)
            }
        }
    }
}

/// Tooltip wrapper pairing a semibold summary line with secondary muted detail lines.
#[component]
pub fn DetailsTooltip(
    #[prop(into)] summary: Signal<String, LocalStorage>,
    #[prop(into)] details: Signal<Vec<String>, LocalStorage>,
    children: Children,
) -> impl IntoView {
    install_tooltip_stylesheet();
    view! {
        Tooltip(delay = TOOLTIP_DELAY) {
            TooltipTrigger {{children.into_view_once()}}
            TooltipContent(class = "gds-tooltip-content", arrow = false) {
                column(class = "gds-tooltip-details") {
                    text(class = "gds-tooltip-details__title") {{move || wrap_tokens(&summary.get())}}
                    for entry in move || details.get().into_iter().enumerate().collect::<Vec<_>>(), key = |entry: &(usize, String)| entry.0 {
                        text(class = "gds-tooltip-details__item") {{wrap_tokens(&entry.1)}}
                    }
                }
            }
        }
    }
}

/// Build plain tooltip body content matching legacy `tooltip::view(text)` callers.
#[must_use]
pub fn view(text: impl Into<String>) -> AnyView {
    let text = text.into();
    AnyView::new(view! { TooltipText(text = text) })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{TOOLTIP_DELAY, wrap_tokens};

    #[test]
    fn tooltip_delay_and_token_wrapping_break_separators_and_16_grapheme_runs() {
        assert_eq!(TOOLTIP_DELAY, Duration::from_millis(400));
        assert_eq!(
            wrap_tokens("/home/very-long"),
            "/\u{200B}home/\u{200B}very-\u{200B}long"
        );
        assert_eq!(
            wrap_tokens("0123456789abcdefGHIJKLMNOPQRSTUVwxyz"),
            "0123456789abcdef\u{200B}GHIJKLMNOPQRSTUV\u{200B}wxyz"
        );
        assert_eq!(
            wrap_tokens("short words stay intact"),
            "short words stay intact"
        );
    }
}
