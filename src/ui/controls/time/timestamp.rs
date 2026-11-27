use std::time::SystemTime;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal, on_cleanup_local};
use zgui::{component, css, view};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipTriggerProps};

use super::{Format, Presentation, format, now, precise, tick_signal};
use crate::app::Gds;
use crate::i18n::FluentArgs;
use crate::ui::tooltip::{TOOLTIP_DELAY, TooltipTextProps};

const TIME_SHEET: &str = "gds-ui-time";

const TIME_CSS: &str = css!(
    ".gds-timestamp.gds-timestamp {
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-variant-numeric: tabular-nums;
    }
    .gds-clock.gds-clock {
        font-variant-numeric: tabular-nums;
    }"
);

pub(super) fn install_time_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(TIME_SHEET, TIME_CSS);
}

/// Timestamp builder usable directly inside `view!` expressions or via [`TimestampView`].
#[derive(Clone, Copy, Debug)]
pub struct Timestamp {
    value: Option<SystemTime>,
    override_format: Option<Format>,
}

impl Timestamp {
    #[must_use]
    pub fn new(value: Option<SystemTime>) -> Self {
        Self {
            value,
            override_format: None,
        }
    }

    #[must_use]
    pub fn format(mut self, format: Format) -> Self {
        self.override_format = Some(format);
        self
    }

    #[must_use]
    pub fn render_text(self) -> String {
        let fmt = self.override_format.unwrap_or_else(format);
        format_presentation(fmt.presentation(self.value, now()))
    }
}

impl IntoView for Timestamp {
    type Output = AnyView;

    fn into_view(self) -> Self::Output {
        AnyView::new(view! {
            TimestampView(value = self.value, override_format = self.override_format)
        })
    }
}

/// Reactive timestamp component rendering localized relative or absolute dates with a
/// precise timestamp tooltip whenever the timestamp is known.
#[component]
pub fn TimestampView(
    #[prop(into)] value: Signal<Option<SystemTime>, LocalStorage>,
    #[prop(optional)] override_format: Option<Format>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_time_stylesheets();
    let ticks = tick_signal();
    let has_value = RwSignal::new_local(value.get_untracked().is_some());
    let sync_has_value = RenderEffect::new(move |_| {
        let next = value.get().is_some();
        if has_value.get_untracked() != next {
            has_value.set(next);
        }
    });
    on_cleanup_local(move || drop(sync_has_value));
    let tooltip_text = Signal::derive_local(move || value.get().map(precise).unwrap_or_default());
    let label_ref = node_ref.unwrap_or_default();

    move || {
        let label_class = class.clone();
        let label_attrs = attrs.clone();
        let label = view! {
            text(class = "gds-timestamp", node_ref = label_ref, class = label_class, {..label_attrs}) {{move || {
                let fmt = override_format.unwrap_or_else(format);
                if fmt == Format::Relative {
                    let _ = ticks.get();
                }
                format_presentation(fmt.presentation(value.get(), now()))
            }}}
        };

        if has_value.get() {
            AnyView::new(view! {
                Tooltip(delay = TOOLTIP_DELAY) {
                    TooltipTrigger {{label}}
                    TooltipContent {
                        TooltipText(text = tooltip_text)
                    }
                }
            })
        } else {
            AnyView::new(label)
        }
    }
}

pub(super) fn format_presentation(presentation: Presentation) -> String {
    let gds = Gds::try_current();
    match presentation {
        Presentation::Missing => gds.as_ref().map_or_else(
            || "\u{2014}".to_owned(),
            |g| g.i18n().text("time-placeholder"),
        ),
        Presentation::Epoch => gds
            .as_ref()
            .map_or_else(|| "Unix epoch".to_owned(), |g| g.i18n().text("time-epoch")),
        Presentation::JustNow => gds
            .as_ref()
            .map_or_else(|| "Just now".to_owned(), |g| g.i18n().text("time-just-now")),
        Presentation::Minutes(n) => gds.as_ref().map_or_else(
            || format!("{n}m ago"),
            |g| g.i18n().count("time-minutes-ago", n as usize),
        ),
        Presentation::Today { time } => gds.as_ref().map_or_else(
            || time.clone(),
            |g| {
                let mut args = FluentArgs::new();
                args.set("time", time.as_str());
                g.i18n().format("time-today", Some(&args))
            },
        ),
        Presentation::Yesterday { time } => gds.as_ref().map_or_else(
            || format!("Yesterday {time}"),
            |g| {
                let mut args = FluentArgs::new();
                args.set("time", time.as_str());
                g.i18n().format("time-yesterday", Some(&args))
            },
        ),
        Presentation::Days(n) => gds.as_ref().map_or_else(
            || format!("{n}d ago"),
            |g| g.i18n().count("time-days-ago", n as usize),
        ),
        Presentation::Weeks(n) => gds.as_ref().map_or_else(
            || format!("{n}w ago"),
            |g| g.i18n().count("time-weeks-ago", n as usize),
        ),
        Presentation::Months(n) => gds.as_ref().map_or_else(
            || format!("{n}mo ago"),
            |g| g.i18n().count("time-months-ago", n as usize),
        ),
        Presentation::Years(n) => gds.as_ref().map_or_else(
            || format!("{n}y ago"),
            |g| g.i18n().count("time-years-ago", n as usize),
        ),
        Presentation::Absolute { text } => text,
    }
}
