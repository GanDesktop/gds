use super::*;
use gpui_kit::SharedString;
#[cfg(feature = "gui-tests")]
use gpui_kit::px;

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

#[cfg(feature = "gui-tests")]
#[gpui::test]
fn keyboard_cursor_pointer_activation_and_close_restore_the_previous_target(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::app::AppExt as _;
    use crate::ui::actions::{CloseNotification, FocusNotifications};
    use gpui_kit::{component::Root, test::TestWindowExt};

    struct Host {
        overlay: Entity<Overlay>,
        origin: FocusHandle,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .on_action(|_: &FocusNotifications, window, cx| focus(window, cx))
                .on_action(|_: &CloseNotification, window, cx| close_current(window, cx))
                .child(
                    div()
                        .id("notification-origin")
                        .test_support()
                        .track_focus(&self.origin)
                        .child("Origin"),
                )
                .child(self.overlay.clone())
        }
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut()
            .appearance()
            .theme()
            .apply(&Default::default())
            .expect("application theme");
        cx.set_reduce_motion(true);
        cx.bind_keys([
            gpui_kit::KeyBinding::new("alt-n", FocusNotifications, None),
            gpui_kit::KeyBinding::new("alt-c", CloseNotification, None),
        ]);
    });
    let mut host = None;
    let window = cx.open_window(gpui_kit::size(px(640.), px(480.)), |window, cx| {
        let view = cx.new(|cx| Host {
            overlay: Overlay::for_window(window, cx),
            origin: cx.focus_handle(),
        });
        host = Some(view.clone());
        Root::new(view, window, cx)
    });
    let host = host.expect("notification host");
    let overlay = cx.update(|cx| host.read(cx).overlay.clone());
    cx.update_window(window.into(), |_, window, cx| {
        overlay.update(cx, |overlay, cx| {
            overlay.push(Notification::info("First").autohide(false), window, cx);
            overlay.push(Notification::info("Second").autohide(false), window, cx);
        });
        host.read(cx).origin.clone().focus(window, cx);
    })
    .expect("mount notifications");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        let notes = overlay.read(cx).available(cx);
        let first = notes[0].entity_id();
        let second = notes[1].entity_id();
        let id = ("gds-notification", second);
        let bounds = window.find(id).bounds();
        let has_cursor = |window: &Window, id, cx: &App| {
            let bounds = window
                .find(("gds-notification", id))
                .bounds()
                .scale(window.scale_factor());
            window.painted_quads().iter().any(|quad| {
                quad.bounds == bounds
                    && quad.border_color == cx.theme().ring
                    && quad.border_widths.top > px(0.).scale(window.scale_factor())
            })
        };
        window.press("alt-n", cx);
        window.render_frame(cx);
        assert_eq!(overlay.read(cx).selected, Some(second));
        assert!(has_cursor(window, second, cx));
        assert!(!has_cursor(window, first, cx));

        // Pointer activity changes presentation, not the notification target.
        window.click_at(
            id,
            gpui_kit::point(bounds.size.width / 2., bounds.size.height / 2.),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(overlay.read(cx).selected, Some(second));
        assert!(!has_cursor(window, second, cx));
        assert_eq!(window.find(id).bounds(), bounds);
        window.press("up", cx);
        window.render_frame(cx);
        assert_eq!(overlay.read(cx).selected, Some(first));
        assert!(has_cursor(window, first, cx));
        assert!(!has_cursor(window, second, cx));
        window.press("alt-c", cx);
        window.render_frame(cx);
        assert_eq!(overlay.read(cx).available(cx).len(), 1);
        assert_eq!(overlay.read(cx).selected, Some(second));
        assert!(has_cursor(window, second, cx));

        window.press("alt-n", cx);
        assert!(host.read(cx).origin.is_focused(window));
        window.press("alt-n", cx);
        window.press("alt-c", cx);
        assert!(overlay.read(cx).available(cx).is_empty());
        assert!(host.read(cx).origin.is_focused(window));
    })
    .expect("notification focus and close lifecycle");
}
