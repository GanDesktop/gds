//! One application preference, combined with the system preference for every window.

use gpui_kit::base::{Root, RootPlugin};
use gpui_kit::{App, Context, Empty, Global, IntoElement, Render, Window};

#[cfg(target_os = "linux")]
mod portal;
#[cfg(all(test, feature = "gui-tests"))]
mod tests;

struct Policy {
    enabled: bool,
    system_reduced: bool,
    following: bool,
}

impl Global for Policy {}

impl Policy {
    fn reduced(&self) -> bool {
        !self.enabled || self.system_reduced
    }
}

/// Install before creating any windows, after Kit has read synchronous platform settings.
pub(super) fn init(cx: &mut App) {
    if cx.has_global::<Policy>() {
        return;
    }
    cx.set_global(Policy {
        enabled: true,
        system_reduced: cx.reduce_motion(),
        following: false,
    });
    Root::register_plugin(cx, |_, _| MotionPolicy);
}

/// Enable decoration following the system preference, or force it to be static.
/// This does not suspend redraws, input, notifications, timers or application work.
pub fn set_animations_enabled(enabled: bool, cx: &mut App) {
    init(cx);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if enabled && !cx.global::<Policy>().enabled {
        // Base's synchronous reader only accepts writes while its previous value
        // is restored. Keep that system value separate from our forced override.
        cx.set_reduce_motion(cx.global::<Policy>().system_reduced);
        gpui_kit::base::apply_system_reduce_motion(cx);
        cx.global_mut::<Policy>().system_reduced = cx.reduce_motion();
    }
    cx.global_mut::<Policy>().enabled = enabled;
    apply(cx);
}

/// Follow the Settings Portal using the application's existing Tokio runtime.
/// Without a portal, retain the last system reading (initially no reduction).
/// Other platforms use Kit's synchronous reading at startup and on re-enabling.
pub fn follow_system(tasks: &crate::service::task_manager::Scope, cx: &mut App) {
    init(cx);
    if cx.global::<Policy>().following || cx.is_test() {
        return;
    }
    cx.global_mut::<Policy>().following = true;
    #[cfg(target_os = "linux")]
    portal::follow(tasks, cx);
    #[cfg(not(target_os = "linux"))]
    let _ = runtime;
}

fn system_changed(reduced: bool, cx: &mut App) {
    cx.global_mut::<Policy>().system_reduced = reduced;
    apply(cx);
}

fn apply(cx: &mut App) {
    let reduced = cx.global::<Policy>().reduced();
    cx.set_reduce_motion(reduced);
}

// Base 0.8.1's portal watcher has a value-based ownership guard: a late system
// write can collide with an identical app override. Reassert the independently
// retained policy before Root builds content or Kit overlays. No platform I/O,
// timers or notifications occur here; the setter refreshes only on a mismatch.
struct MotionPolicy;

impl RootPlugin for MotionPolicy {
    fn prepare(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        apply(cx);
    }
}

impl Render for MotionPolicy {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}
