//! One application-wide clock that keeps relative timestamps current.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local, Timelike};
use gpui_kit::Context;

use super::Format;

/// Never tick more often than this, so a value landing on a minute boundary
/// cannot spin the loop.
const MIN_TICK: Duration = Duration::from_secs(1);

pub struct Clock {
    format: Format,
}

impl Clock {
    pub fn new(format: Format, cx: &mut Context<Self>) -> Self {
        let clock = Self { format };
        clock.start(cx);
        clock
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn set_format(&mut self, format: Format, cx: &mut Context<Self>) {
        if self.format == format {
            return;
        }
        self.format = format;
        cx.refresh_windows();
    }

    fn start(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |clock, cx| {
            loop {
                let timer = cx
                    .background_executor()
                    .timer(next_tick_delay(SystemTime::now()));
                timer.await;
                if clock.update(cx, |clock, cx| clock.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        // Exact text never changes with time, so only relative labels redraw.
        if self.format == Format::Relative {
            cx.refresh_windows();
        }
    }
}

/// Delay to the next whole minute of local wall clock time.
fn next_tick_delay(now: SystemTime) -> Duration {
    let local = DateTime::<Local>::from(now);
    let remaining = Duration::from_secs(u64::from(60 - local.second()));
    remaining
        .saturating_sub(Duration::from_nanos(u64::from(local.nanosecond())))
        .max(MIN_TICK)
}
