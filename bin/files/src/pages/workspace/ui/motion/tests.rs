use super::*;
use gpui_kit::{Context, IntoElement, Render, TestAppContext, div, px, size};
use std::{cell::Cell, rc::Rc};

struct Host {
    tab: u64,
    opacity: Rc<Cell<f32>>,
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.opacity.set(tab_content(self.tab, window, cx));
        div()
    }
}

#[gpui::test]
fn tab_switch_replays_on_return_and_reduced_motion_settles(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let opacity = Rc::new(Cell::new(0.));
    let window = cx.open_window(size(px(300.), px(100.)), {
        let opacity = opacity.clone();
        move |_, _| Host { tab: 1, opacity }
    });
    cx.run_until_parked();
    assert_eq!(opacity.get(), 1.);
    window
        .update(cx, |host, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0);
            host.tab = 2;
            window.refresh();
        })
        .expect("switch tab");
    cx.run_until_parked();
    assert_eq!(opacity.get(), TAB_ENTER_OPACITY);
    cx.executor().advance_clock(DURATION / 2);
    window
        .update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .expect("advance transition");
    cx.run_until_parked();
    assert!(opacity.get() > TAB_ENTER_OPACITY && opacity.get() < 1.);
    window
        .update(cx, |host, window, _| {
            host.tab = 1;
            window.refresh();
        })
        .expect("return to the previous tab");
    cx.run_until_parked();
    assert_eq!(opacity.get(), TAB_ENTER_OPACITY);
    cx.update(|cx| cx.set_reduce_motion(true));
    window
        .update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.refresh();
        })
        .expect("settle reduced motion");
    cx.run_until_parked();
    assert_eq!(opacity.get(), 1.);
    window
        .update(cx, |host, window, _| {
            host.tab = 2;
            window.refresh();
        })
        .expect("switch with motion disabled");
    cx.run_until_parked();
    assert_eq!(opacity.get(), 1.);
    window
        .update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .expect("drain scheduled frame");
    cx.run_until_parked();
    window
        .update(cx, |_, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0);
        })
        .expect("no continuing decorative frames");
}
