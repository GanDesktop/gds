//! Dwell-based tab switching while a file drag rests over a tab row.
//!
//! Every tab row in a window shares one [`TabHoverSpring`]. The first move that
//! finds the pointer over a row arms a single background wait; further moves
//! over the same row only refresh its bounds, so the wait can actually elapse.
//! Leaving the row, a released or cancelled drag, an inactive window and a row
//! that already shows its folder all drop the pending switch, and every armed
//! wait carries the generation it was started with so a stale one never fires.

use std::time::Duration;

use gpui_kit::component::WindowExt;
use gpui_kit::*;

use crate::pages::workspace::ui::Browser;

/// How long a file drag must rest over another tab before it switches.
const TAB_HOVER_DELAY: Duration = Duration::from_millis(600);

/// One drag-move observation over a tab row, in window coordinates.
pub(super) struct TabHover {
    pub(super) session: u64,
    pub(super) tab_id: u64,
    /// The row's bounds: the wait re-checks the real pointer against them.
    pub(super) bounds: Bounds<Pixels>,
    pub(super) pointer: Point<Pixels>,
    /// Whether the row already shows the folder, so a switch would do nothing.
    pub(super) on_current: bool,
}

/// The pending tab switch of one window's tab strip.
#[derive(Default)]
pub(super) struct TabHoverSpring {
    generation: u64,
    pending: Option<PendingSwitch>,
}

/// The one switch an armed wait may still perform.
pub(super) struct PendingSwitch {
    pub(super) session: u64,
    pub(super) tab_id: u64,
    pub(super) bounds: Bounds<Pixels>,
}

impl TabHoverSpring {
    /// Fold one observation in.
    ///
    /// Returns the generation of a wait the caller must start, which happens
    /// only when the pointer newly reaches an inactive row. A move that stays
    /// on the armed row keeps the running wait and only refreshes its bounds.
    pub(super) fn observe(&mut self, hover: TabHover) -> Option<u64> {
        let armed_for_row = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.tab_id == hover.tab_id);
        if hover.bounds.contains(&hover.pointer) && !hover.on_current {
            if armed_for_row
                && self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.session == hover.session)
            {
                if let Some(pending) = self.pending.as_mut() {
                    pending.bounds = hover.bounds;
                }
                return None;
            }
            self.pending = Some(PendingSwitch {
                session: hover.session,
                tab_id: hover.tab_id,
                bounds: hover.bounds,
            });
            self.generation += 1;
            return Some(self.generation);
        }
        if armed_for_row {
            self.clear();
        }
        None
    }

    /// Take the pending switch an armed wait asked for.
    ///
    /// `None` means the wait is stale: the pointer left the row, reached another
    /// row, or another wait already claimed it.
    pub(super) fn claim(&mut self, generation: u64) -> Option<PendingSwitch> {
        if generation != self.generation {
            return None;
        }
        let pending = self.pending.take()?;
        self.generation += 1;
        Some(pending)
    }

    fn clear(&mut self) {
        self.pending = None;
        self.generation += 1;
    }
}

/// Wait out [`TAB_HOVER_DELAY`] once, then switch to the armed tab if it is
/// still the right thing to do.
pub(super) fn wait_then_switch(
    spring: WeakEntity<TabHoverSpring>,
    browser: WeakEntity<Browser>,
    generation: u64,
    window: &Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(TAB_HOVER_DELAY).await;
            // A window closed while the wait ran has no tab strip left to switch.
            let _ = spring.update_in(cx, |spring, window, cx| {
                let Some(pending) = spring.claim(generation) else {
                    return;
                };
                // A drag ends into a drop or a cancel, and a window that lost
                // focus is no place to switch behind the user's back.
                if !window.is_window_active()
                    || !cx.has_active_drag()
                    || window.has_active_dialog(cx)
                    || pending.session != super::file_drag_session(cx)
                {
                    return;
                }
                if !pending.bounds.contains(&window.mouse_position()) {
                    return;
                }
                let Some(browser) = browser.upgrade() else {
                    return;
                };
                browser.update(cx, |browser, cx| {
                    // The row may have been closed while the wait ran.
                    if browser.tabs.iter().any(|tab| tab.id == pending.tab_id) {
                        browser.switch_tab(pending.tab_id, window, cx);
                    }
                });
            });
        })
        .detach();
}
