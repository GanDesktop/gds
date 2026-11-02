//! The shared timestamp element used by tables and detail panels.

use std::time::SystemTime;

use fluent_bundle::FluentArgs;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{IntoElement, prelude::*, *};

use crate::{
    appearance::tokens::TOOLTIP_DELAY,
    i18n,
    ui::time::{self, Format, Presentation},
};

/// A single line of timestamp text that inherits the surrounding text style.
///
/// `None` means the timestamp is unknown and renders as a placeholder;
/// `Some(UNIX_EPOCH)` is a real value and renders as the epoch. Hovering a
/// known value shows the exact local time with seconds and UTC offset.
#[derive(IntoElement)]
pub struct Timestamp {
    id: ElementId,
    value: Option<SystemTime>,
    format: Option<Format>,
}

impl Timestamp {
    pub fn new(id: impl Into<ElementId>, value: Option<SystemTime>) -> Self {
        Self {
            id: id.into(),
            value,
            format: None,
        }
    }

    /// Present this value in `format` instead of the shared application format.
    pub fn format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }
}

impl RenderOnce for Timestamp {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let format = self.format.unwrap_or_else(|| time::format(cx));
        let presentation = format.presentation(self.value, time::now(cx));
        let text = presentation.text(cx);
        let element = div().id(self.id).min_w_0().truncate();
        match self.value {
            Some(value) => {
                let precise: SharedString = time::precise(value).into();
                element
                    .tooltip_show_delay(TOOLTIP_DELAY)
                    .tooltip(move |window, cx| Tooltip::new(precise.clone()).build(window, cx))
                    .child(text)
            }
            None => element.child(text),
        }
    }
}

impl Presentation {
    /// Localized label for this presentation.
    pub fn text(&self, cx: &App) -> SharedString {
        match self {
            Self::Missing => i18n::text("time-placeholder", cx),
            Self::Epoch => i18n::text("time-epoch", cx),
            Self::JustNow => i18n::text("time-just-now", cx),
            Self::Minutes(count) => i18n::count("time-minutes-ago", *count as usize, cx),
            Self::Today { time } => clock_text("time-today", time, cx),
            Self::Yesterday { time } => clock_text("time-yesterday", time, cx),
            Self::Days(count) => i18n::count("time-days-ago", *count as usize, cx),
            Self::Weeks(count) => i18n::count("time-weeks-ago", *count as usize, cx),
            Self::Months(count) => i18n::count("time-months-ago", *count as usize, cx),
            Self::Years(count) => i18n::count("time-years-ago", *count as usize, cx),
            Self::Absolute { text } => text.clone().into(),
        }
    }
}

fn clock_text(id: &str, time: &str, cx: &App) -> SharedString {
    let mut arguments = FluentArgs::new();
    arguments.set("time", time.to_owned());
    i18n::format(id, Some(&arguments), cx)
}
