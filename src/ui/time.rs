//! Shared timestamp preferences and a single application-owned minute clock.
mod presentation;
mod timestamp;
use anti_webui::prelude::*;
pub use presentation::{Presentation, precise};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    time::{Duration, SystemTime},
};
pub use timestamp::Timestamp;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, crate::ConfigEditor,
)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[setting(label = "form-time-absolute")]
    Absolute,
    #[default]
    #[setting(label = "form-time-relative")]
    Relative,
}
#[derive(Clone, Copy)]
struct Clock {
    format: Signal<Format>,
    now: Signal<SystemTime>,
}
impl Clock {
    fn new(format: Format) -> Self {
        let clock = Self {
            format: signal(format),
            now: signal(SystemTime::now()),
        };
        let sender = ui_channel(move |now| clock.now.set(now));
        let scope = crate::app::Gds::current()
            .tasks()
            .owner_scope("Relative timestamps");
        scope
            .spawn_cancellable_named("Refresh timestamps", async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    sender.send(SystemTime::now());
                }
            })
            .on_complete(&scope, |result: Result<(), _>| {
                if let Err(error) = result {
                    tracing::debug!(%error, "timestamp clock stopped");
                }
            });
        clock
    }
}
thread_local! { static CLOCK: Cell<Option<Clock>> = const { Cell::new(None) }; }
pub(crate) fn apply(format: Format) {
    let gds = crate::app::Gds::current();
    gds.with(|| {
        CLOCK.with(|slot| match slot.get() {
            Some(clock) => clock.format.set_if_changed(format),
            None => {
                slot.set(Some(Clock::new(format)));
                on_cleanup(|| CLOCK.with(|slot| slot.set(None)));
            }
        })
    });
}
pub fn format() -> Format {
    CLOCK.with(|slot| {
        slot.get()
            .map_or(Format::default(), |clock| clock.format.get())
    })
}
pub fn now() -> SystemTime {
    CLOCK.with(|slot| {
        if let Some(clock) = slot.get() {
            clock.now.get();
        }
    });
    SystemTime::now()
}
#[cfg(test)]
mod tests;
