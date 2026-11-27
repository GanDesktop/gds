//! Shared timestamp presentation and reactive application clock for GDS.

mod clock;
mod presentation;
#[cfg(test)]
mod tests;
mod timestamp;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use zgui::reactive::{LocalStorage, Owner, RwSignal, on_cleanup_local, prelude::*};
use zgui::view::{TimeoutHandle, Timers};

use crate::app::Gds;
pub use clock::{Clock, ClockProps};
pub use presentation::{Presentation, precise};
pub use timestamp::{Timestamp, TimestampView, TimestampViewProps};

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

#[derive(Clone)]
struct ClockState {
    id: u64,
    format: RwSignal<Format, LocalStorage>,
    ticks: RwSignal<u64, LocalStorage>,
    started: Rc<Cell<bool>>,
    alive: Rc<Cell<bool>>,
    timer: Rc<RefCell<Option<TimeoutHandle>>>,
}

thread_local! {
    static NEXT_CLOCK_ID: Cell<u64> = const { Cell::new(1) };
    static SHARED_CLOCK: RefCell<Option<ClockState>> = const { RefCell::new(None) };
}

fn is_state_live(state: &ClockState) -> bool {
    state.alive.get() && state.ticks.try_get_untracked().is_some()
}

fn init_clock_state() -> ClockState {
    let id = NEXT_CLOCK_ID.with(|next| {
        let current = next.get();
        next.set(current.wrapping_add(1));
        current
    });
    let format = RwSignal::new_local(Format::default());
    let ticks = RwSignal::new_local(0_u64);
    let started = Rc::new(Cell::new(false));
    let alive = Rc::new(Cell::new(true));
    let timer = Rc::new(RefCell::new(None));

    if Owner::current().is_some() {
        let alive_cleanup = Rc::clone(&alive);
        let timer_cleanup = Rc::clone(&timer);
        on_cleanup_local(move || {
            alive_cleanup.set(false);
            *timer_cleanup.borrow_mut() = None;
            SHARED_CLOCK.with(|slot| {
                let matches = slot.borrow().as_ref().is_some_and(|cur| cur.id == id);
                if matches {
                    *slot.borrow_mut() = None;
                }
            });
        });
    }

    ClockState {
        id,
        format,
        ticks,
        started,
        alive,
        timer,
    }
}

fn ensure_clock_state() -> ClockState {
    if let Some(state) = SHARED_CLOCK.with(|slot| {
        let current = slot.borrow();
        current.as_ref().filter(|s| is_state_live(s)).cloned()
    }) {
        return state;
    }

    let state = if let Some(gds) = Gds::try_current() {
        gds.with(init_clock_state)
    } else {
        init_clock_state()
    };

    SHARED_CLOCK.with(|slot| {
        *slot.borrow_mut() = Some(state.clone());
    });
    state
}

fn schedule_next_tick(state: ClockState) {
    if !state.alive.get() {
        return;
    }
    let delay = clock::next_tick_delay(now());

    if let Some(timers) = Timers::current() {
        let state_cb = state.clone();
        let handle = timers.set_timeout(delay, move || {
            if !state_cb.alive.get() {
                return;
            }
            let _ = state_cb.ticks.try_update(|n| *n = n.wrapping_add(1));
            schedule_next_tick(state_cb);
        });
        *state.timer.borrow_mut() = Some(handle);
        state.started.set(true);
        return;
    }

    if let Some(gds) = Gds::try_current()
        && gds.tasks().scope().has_runtime()
    {
        state.started.set(true);
        gds.tasks().spawn_ui(
            "ui.time.clock",
            async move { tokio::time::sleep(delay).await },
            move |result| {
                if result.is_ok() && state.alive.get() {
                    let _ = state.ticks.try_update(|n| *n = n.wrapping_add(1));
                    schedule_next_tick(state);
                }
            },
        );
    }
}

pub(crate) fn tick_signal() -> RwSignal<u64, LocalStorage> {
    let state = ensure_clock_state();
    if !state.started.get() {
        schedule_next_tick(state.clone());
    }
    state.ticks
}

/// Install or reconfigure the shared timestamp format.
pub fn apply(format: Format) {
    ensure_clock_state().format.set(format);
}

/// Read the active shared timestamp format.
#[must_use]
pub fn format() -> Format {
    ensure_clock_state().format.get()
}

/// Current wall-clock timestamp used when classifying relative times.
#[must_use]
pub fn now() -> SystemTime {
    SystemTime::now()
}
