use std::{cell::Cell, rc::Rc};

use gpui_kit::{
    Context, Render, TestAppContext, TestSupportExt, component::spinner::Spinner, size,
    test::TestWindowExt,
};

use super::*;

struct Host {
    expanded: bool,
    progress: Rc<Cell<f32>>,
    spinner: bool,
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon = DisclosureIcon::default()
            .id("arrow")
            .expanded(self.expanded);
        self.progress.set(icon.progress(window, cx));
        div()
            .flex()
            .child(div().id("icon-slot").test_support().child(icon))
            .child(div().id("label-slot").test_support().child("Section"))
            .when(self.spinner, |this| this.child(Spinner::new()))
    }
}

#[gpui::test]
fn disclosure_reverses_without_moving_slots_and_stops_at_rest(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let progress = Rc::new(Cell::new(0.));
    let window = cx.open_window(size(px(300.), px(80.)), {
        let progress = progress.clone();
        move |_, _| Host {
            expanded: true,
            progress,
            spinner: false,
        }
    });
    cx.run_until_parked();
    assert_eq!(progress.get(), 1.);
    let slots = window
        .update(cx, |_, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0);
            (
                window.find("icon-slot").bounds(),
                window.find("label-slot").bounds(),
            )
        })
        .expect("first display settles immediately");
    window
        .update(cx, |host, window, _| {
            host.expanded = false;
            window.refresh();
        })
        .expect("collapse section");
    cx.run_until_parked();
    assert_eq!(progress.get(), 1.);
    cx.executor().advance_clock(Duration::from_millis(50));
    window
        .update(cx, |_, window, cx| {
            // Both the sampled probe and rendered icon request this active frame.
            assert!(window.simulate_next_frame(cx) > 0);
        })
        .expect("advance arrow");
    cx.run_until_parked();
    let middle = progress.get();
    assert!(middle > 0. && middle < 1.);
    window
        .update(cx, |host, window, _| {
            assert_eq!(
                slots,
                (
                    window.find("icon-slot").bounds(),
                    window.find("label-slot").bounds()
                )
            );
            host.expanded = true;
            window.refresh();
        })
        .expect("reverse section");
    cx.run_until_parked();
    assert_eq!(progress.get(), middle);
    cx.executor().advance_clock(DURATION);
    window
        .update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .expect("finish arrow");
    cx.run_until_parked();
    assert_eq!(progress.get(), 1.);
    window
        .update(cx, |_, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0);
            assert_eq!(
                slots,
                (
                    window.find("icon-slot").bounds(),
                    window.find("label-slot").bounds()
                )
            );
        })
        .expect("stable final geometry");
}

#[gpui::test]
fn reducing_motion_settles_arrow_and_stops_kit_spinner(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let progress = Rc::new(Cell::new(0.));
    let window = cx.open_window(size(px(300.), px(80.)), {
        let progress = progress.clone();
        move |_, _| Host {
            expanded: false,
            progress,
            spinner: true,
        }
    });
    cx.run_until_parked();
    assert_eq!(progress.get(), 0.);
    window
        .update(cx, |host, window, _| {
            host.expanded = true;
            window.refresh();
        })
        .expect("expand section");
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(40));
    window
        .update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .expect("advance active motion");
    cx.run_until_parked();
    assert!(progress.get() > 0. && progress.get() < 1.);
    cx.update(|cx| cx.set_reduce_motion(true));
    window
        .update(cx, |_, window, _| window.refresh())
        .expect("apply reduced motion");
    cx.run_until_parked();
    assert_eq!(progress.get(), 1.);
    window
        .update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .expect("drain pending frame");
    cx.run_until_parked();
    window
        .update(cx, |host, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0);
            assert!(window.find("icon-slot").visible());
            host.expanded = false;
            window.refresh();
        })
        .expect("static artwork remains visible");
    cx.run_until_parked();
    assert_eq!(progress.get(), 0.);
    window
        .update(cx, |_, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0)
        })
        .expect("no decoration frames");
}
