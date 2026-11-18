//! Native tooltips share the toolkit's delay, popup, and focus behavior.
pub use anti_webui::widgets::{TooltipOptions, tooltip_with};
use anti_webui::{
    prelude::{Div, Styled as _},
    widgets::{TooltipParts, styled_tooltip},
};

pub fn tooltip(target: Div, label: impl Into<String>) -> Div {
    styled_tooltip(
        target,
        label,
        TooltipParts::new().bubble(|bubble, _| bubble.overflow_wrap_anywhere()),
    )
}
