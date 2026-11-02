use super::*;
use gpui_kit::{
    AppContext, ParentElement, TestAppContext, WindowHandle, component::spinner::Spinner, div, px,
    size,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

struct Probe(Rc<RefCell<Vec<bool>>>);
impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0.borrow_mut().push(cx.reduce_motion());
        div().child(Spinner::new())
    }
}

fn open(cx: &mut TestAppContext, samples: Rc<RefCell<Vec<bool>>>) -> WindowHandle<Root> {
    cx.open_window(size(px(300.), px(200.)), |window, cx| {
        let probe = cx.new(|_| Probe(samples));
        Root::new(probe, window, cx)
    })
}

#[gpui::test]
fn preference_off_on_restores_system_and_late_system_updates_cannot_enable(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::appearance::init(cx);
        for system in [false, true] {
            system_changed(system, cx);
            set_animations_enabled(false, cx);
            assert!(cx.reduce_motion());
            system_changed(!system, cx);
            assert!(cx.reduce_motion());
            set_animations_enabled(true, cx);
            assert_eq!(cx.reduce_motion(), !system);
            set_animations_enabled(false, cx);
            system_changed(system, cx);
            set_animations_enabled(true, cx);
            assert_eq!(cx.reduce_motion(), system);
        }
    });
}

#[gpui::test]
fn all_roots_reassert_policy_before_render_and_static_spinners_stop_frames(
    cx: &mut TestAppContext,
) {
    cx.update(crate::appearance::init);
    let first_samples = Rc::new(RefCell::new(Vec::new()));
    let second_samples = Rc::new(RefCell::new(Vec::new()));
    let first = open(cx, first_samples.clone());
    let second = open(cx, second_samples.clone());
    cx.run_until_parked();
    // Start a native Kit spinner in both existing windows before disabling.
    for window in [first, second] {
        assert!(
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .expect("motion window frame")
                > 0
        );
    }
    cx.update(|cx| set_animations_enabled(false, cx));
    cx.run_until_parked();
    first_samples.borrow_mut().clear();
    second_samples.borrow_mut().clear();
    // Model Base's value-ownership collision: a callback writes false after the
    // app forced true. Root must repair it before cached children/overlays draw.
    cx.update(|cx| cx.set_reduce_motion(false));
    cx.run_until_parked();
    for samples in [&first_samples, &second_samples] {
        assert!(!samples.borrow().is_empty());
        assert!(samples.borrow().iter().all(|reduced| *reduced));
    }
    let new_samples = Rc::new(RefCell::new(Vec::new()));
    let third = open(cx, new_samples.clone());
    cx.run_until_parked();
    assert!(new_samples.borrow().iter().all(|reduced| *reduced));
    cx.executor().advance_clock(Duration::from_secs(1));
    // Drain at most the already requested frame, then no decorative requests.
    for window in [first, second, third] {
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .expect("motion window frame");
    }
    cx.run_until_parked();
    for window in [first, second, third] {
        assert_eq!(
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .expect("motion window frame"),
            0
        );
    }
    cx.update(|cx| {
        system_changed(false, cx);
        set_animations_enabled(true, cx);
    });
    cx.run_until_parked();
    for window in [first, second, third] {
        assert!(
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .expect("motion window frame")
                > 0
        );
    }
}
