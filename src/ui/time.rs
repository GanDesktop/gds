//! Shared timestamp presentation for GDS applications.
//!
//! [`Timestamp`] renders one value as a single line of text and inherits the
//! surrounding text style. It uses the shared application [`Format`] unless the
//! caller overrides it, and a single application clock refreshes relative text
//! once a minute, so an open window never keeps showing a stale "1 minute ago".

mod clock;
mod presentation;
mod timestamp;

use std::time::SystemTime;

use gpui_kit::{App, AppContext, Entity, Global};
use serde::{Deserialize, Serialize};

use clock::Clock;
pub use presentation::{Presentation, precise};
pub use timestamp::Timestamp;

/// How shared timestamp elements present a value.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, crate::ConfigEditor,
)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Exact local date and time, e.g. `2026-10-25 01:54`.
    #[setting(label = "form-time-absolute")]
    Absolute,
    /// Localized distance from the current time, e.g. `3 minutes ago`.
    #[default]
    #[setting(label = "form-time-relative")]
    Relative,
}

struct Shared {
    clock: Entity<Clock>,
}

impl Global for Shared {}

/// Install or reconfigure the shared timestamp format.
///
/// The first call creates the application clock; later calls only reconfigure
/// it, so preference changes never accumulate timers. Applications call this
/// once at startup with the stored preference, then again whenever the
/// preference changes.
pub(crate) fn apply(format: Format, cx: &mut App) {
    let existing = cx.try_global::<Shared>().map(|shared| shared.clock.clone());
    match existing {
        Some(clock) => clock.update(cx, |clock, cx| clock.set_format(format, cx)),
        None => {
            let clock = cx.new(|cx| Clock::new(format, cx));
            cx.set_global(Shared { clock });
        }
    }
}

/// The configured shared format, used by [`Timestamp`] by default.
pub(crate) fn format(cx: &App) -> Format {
    cx.try_global::<Shared>()
        .map_or_else(Format::default, |shared| shared.clock.read(cx).format())
}

/// Current wall-clock time for a render. The shared clock schedules refreshes,
/// but a newly modified file must not look like a future value between ticks.
pub(crate) fn now(_: &App) -> SystemTime {
    SystemTime::now()
}

#[cfg(test)]
mod tests;
