//! Reactive clock display synchronized across windows.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local, Timelike};
use zgui::prelude::*;
use zgui::{component, view};

use super::tick_signal;

const NANOS_PER_SECOND: u64 = 1_000_000_000;
const MINUTE_NANOS: u64 = 60 * NANOS_PER_SECOND;
const MIN_TICK_DELAY: Duration = Duration::from_millis(50);

/// Remaining duration until the start of the next whole local minute.
pub(super) fn next_tick_delay(now: SystemTime) -> Duration {
    let local: DateTime<Local> = now.into();
    let elapsed_nanos = u64::from(local.second().min(59)) * NANOS_PER_SECOND
        + u64::from(local.nanosecond().min(999_999_999));
    let remaining_nanos = MINUTE_NANOS.saturating_sub(elapsed_nanos);
    Duration::from_nanos(remaining_nanos).max(MIN_TICK_DELAY)
}

/// Reactive clock component refreshing at minute ticks.
#[component]
pub fn Clock(#[prop(into, optional)] class: Classes, #[prop(attrs)] attrs: Attrs) -> impl IntoView {
    super::timestamp::install_time_stylesheets();
    let ticks = tick_signal();
    view! {
        text(class = "gds-clock", class = class, {..attrs}) {{move || {
            let _ = ticks.get();
            Local::now().format("%H:%M").to_string()
        }}}
    }
}
