use super::*;
use gpui_kit::{SharedString, px};

fn details(count: usize) -> Vec<SharedString> {
    (0..count)
        .map(|index| format!("item {index}").into())
        .collect()
}

#[test]
fn report_details_never_exceed_the_visible_limit() {
    let items = details(DETAIL_LIMIT + 3);
    let (shown, remaining) = visible_details(&items);
    assert_eq!(shown.len(), DETAIL_LIMIT);
    assert_eq!(remaining, 3);

    let items = details(2);
    let (shown, remaining) = visible_details(&items);
    assert_eq!(shown.len(), 2);
    assert_eq!(remaining, 0);
}

#[test]
fn content_height_keeps_the_card_inside_a_short_window() {
    assert_eq!(content_height(px(820.)), px(MAX_CONTENT_HEIGHT));

    // A window with less room than the cap shrinks the text area instead of
    // letting the card run past the bottom edge.
    let short = content_height(px(320.));
    assert!(short < px(MAX_CONTENT_HEIGHT));
    let card_bottom = px(crate::appearance::tokens::TOOLBAR_HEIGHT + TOAST_TOP_GAP)
        + short
        + px(CARD_CHROME_HEIGHT);
    assert!(card_bottom <= px(320.));
}

#[cfg(feature = "gui-tests")]
#[gpui::test]
fn reduced_motion_preserves_notification_timeout_and_manual_close(
    cx: &mut gpui_kit::TestAppContext,
) {
    use std::{cell::Cell, rc::Rc, time::Duration};

    struct NotificationHost(Entity<Overlay>);
    impl Render for NotificationHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.0.clone())
        }
    }
    struct Timed;
    struct Persistent;

    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    let timed_closed = Rc::new(Cell::new(false));
    let manual_closed = Rc::new(Cell::new(false));
    let window = cx.open_window(gpui_kit::size(px(640.), px(480.)), |window, cx| {
        NotificationHost(Overlay::for_window(window, cx))
    });
    window
        .update(cx, |host, window, cx| {
            let timed_closed = timed_closed.clone();
            let manual_closed = manual_closed.clone();
            host.0.update(cx, |overlay, cx| {
                overlay.push(
                    Notification::info("Finished")
                        .id::<Timed>()
                        .on_close(move |_, _| timed_closed.set(true)),
                    window,
                    cx,
                );
                overlay.push(
                    Notification::info("Working")
                        .id::<Persistent>()
                        .autohide(false)
                        .on_close(move |_, _| manual_closed.set(true)),
                    window,
                    cx,
                );
            });
        })
        .expect("push real notification queue entries");
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    window
        .update(cx, |host, window, cx| {
            assert_eq!(host.0.read(cx).count(cx), 2);
            assert_eq!(
                window.simulate_next_frame(cx),
                0,
                "decoration is static while the timeout remains live"
            );
        })
        .expect("static notices remain mounted");
    cx.executor().advance_clock(Duration::from_secs(6));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(timed_closed.get(), "auto-hide must still complete");
    assert!(
        !manual_closed.get(),
        "persistent work notice must not expire"
    );
    window
        .update(cx, |host, window, cx| {
            assert_eq!(host.0.read(cx).count(cx), 1);
            close_current(window, cx);
        })
        .expect("manually close persistent notification");
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(manual_closed.get());
    window
        .update(cx, |host, window, cx| {
            assert_eq!(host.0.read(cx).count(cx), 0);
            assert_eq!(window.simulate_next_frame(cx), 0);
        })
        .expect("notification lifecycle settles independently of motion");
}

#[test]
fn column_height_stops_at_the_bottom_inset() {
    let top = px(crate::appearance::tokens::TOOLBAR_HEIGHT + TOAST_TOP_GAP);
    let bottom = px(TOAST_INSET);
    assert_eq!(
        column_height(px(820.), top, bottom),
        px(820.) - top - bottom
    );

    // A window shorter than its own chrome gets no column instead of one that
    // runs past the bottom edge.
    assert_eq!(column_height(px(40.), top, bottom), px(0.));
}
