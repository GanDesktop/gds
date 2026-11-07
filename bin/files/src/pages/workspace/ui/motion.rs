//! Match the sidebar timing for workspace transitions.
use gpui_kit::{
    App, Window,
    base::motion::{self, Sequence, Transition, TransitionId},
};
use std::time::Duration;

pub(super) const DURATION: Duration = Duration::from_millis(160);

pub(super) fn state_progress(
    id: impl Into<TransitionId>,
    expanded: bool,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    motion::transition(
        id,
        if expanded { 1. } else { 0. },
        Transition::new(DURATION),
        window,
        cx,
    )
}

const TAB_ENTER_OPACITY: f32 = 0.88;

struct TabContent {
    tab: u64,
    generation: usize,
}

/// Change the content immediately; only its presentation settles after a tab switch.
pub(super) fn tab_content(tab: u64, window: &mut Window, cx: &mut App) -> f32 {
    let state = window.use_keyed_state("files-tab-content", cx, |_, _| TabContent {
        tab,
        generation: 0,
    });
    state.update(cx, |state, _| {
        if state.tab != tab {
            state.tab = tab;
            state.generation += 1;
        }
    });
    let snapshot = state.read(cx);
    if snapshot.generation == 0 {
        return 1.;
    }
    Sequence::new(
        format!("files-tab-enter-{}", snapshot.generation),
        TAB_ENTER_OPACITY,
    )
    .with_step(1., Transition::new(DURATION))
    .sample(window, cx)
    .into_value()
}
