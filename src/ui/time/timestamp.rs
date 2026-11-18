//! A timestamp label retaining exact-time hover text.
use super::{Format, Presentation};
use anti_webui::prelude::*;
use fluent_bundle::FluentArgs;
use gds_uim::el;
use std::time::SystemTime;

pub struct Timestamp {
    value: Option<SystemTime>,
    format: Option<Format>,
}
impl Timestamp {
    pub fn new(value: Option<SystemTime>) -> Self {
        Self {
            value,
            format: None,
        }
    }
    pub fn format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }
    pub fn label(&self) -> String {
        self.format
            .unwrap_or_else(super::format)
            .presentation(self.value, super::now())
            .text()
    }
}
impl Presentation {
    pub fn text(&self) -> String {
        let gds = crate::app::Gds::current();
        let messages = gds.i18n();
        match self {
            Self::Missing => "—".into(),
            Self::Epoch => messages.text("time-epoch"),
            Self::JustNow => messages.text("time-just-now"),
            Self::Minutes(count) => messages.count("time-minutes-ago", *count as usize),
            Self::Days(count) => messages.count("time-days-ago", *count as usize),
            Self::Weeks(count) => messages.count("time-weeks-ago", *count as usize),
            Self::Months(count) => messages.count("time-months-ago", *count as usize),
            Self::Years(count) => messages.count("time-years-ago", *count as usize),
            Self::Today { time } | Self::Yesterday { time } => {
                let mut args = FluentArgs::new();
                args.set("time", time.as_str());
                messages.format(
                    if matches!(self, Self::Today { .. }) {
                        "time-today"
                    } else {
                        "time-yesterday"
                    },
                    Some(&args),
                )
            }
            Self::Absolute { text } => text.clone(),
        }
    }
}
impl IntoView for Timestamp {
    fn into_view(self) -> View {
        let precise = self.value.map(super::precise);
        let label = el! { .div.[ (move () => ${
            self.label()
        }).text ] };
        match precise {
            Some(precise) => tooltip(label, precise),
            None => label,
        }
        .into_view()
    }
}
