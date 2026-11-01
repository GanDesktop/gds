use super::*;
use gpui_kit::InputEvent as _;
use gpui_kit::{Modifiers, ModifiersChangedEvent, component::WindowExt};

fn hold(fixture: &Fixture, modifiers: Modifiers, cx: &mut TestAppContext) {
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(
            ModifiersChangedEvent {
                modifiers,
                capslock: window.capslock(),
            }
            .to_platform_input(),
            cx,
        );
    })
    .expect("change held modifiers");
    draw(fixture, cx);
}

fn draw(fixture: &Fixture, cx: &mut TestAppContext) {
    cx.update_window(fixture.window.into(), |_, window, cx| {
        // The root layer paints after controls stage their current bounds.
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .expect("render shortcut layer");
}

#[gpui_kit::test]
fn held_groups_follow_enabled_controls_and_the_focused_scope(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let meta = session();
    attach(&fixture, meta.clone(), cx);
    hold(&fixture, Modifiers::control(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(window.find("keytip-sidebar-search").label(), Some("F"));
        assert_eq!(window.find("keytip-new-session").label(), Some("N"));
        assert_eq!(window.find("keytip-sidebar-toggle").label(), Some("B"));
        assert!(window.try_find("keytip-expand-message-editor").is_none());
        assert!(
            window.try_find("keytip-send-message").is_none(),
            "empty input cannot submit"
        );
        fixture.view.update(cx, |view, cx| {
            view.composer.input.update(cx, |input, cx| {
                input.set_value("keep this draft", window, cx);
            })
        });
    })
    .expect("Ctrl hints");
    draw(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(window.find("keytip-send-message").label(), Some("Enter"));
        fixture.view.update(cx, |view, cx| {
            view.sessions.pending.configuring.insert(meta.id);
            cx.notify();
        });
    })
    .expect("enabled send hint");
    draw(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, _| {
        assert!(
            window.try_find("keytip-send-message").is_none(),
            "pending options block submission"
        );
    })
    .expect("disabled hints disappear");

    hold(&fixture, Modifiers::alt(), cx);
    cx.update_window(fixture.window.into(), |_, window, _| {
        assert_eq!(
            window.find("keytip-expand-message-editor").label(),
            Some("Shift+Esc")
        );
        assert!(window.try_find("keytip-sidebar-search").is_none());
    })
    .expect("Alt hints");
    hold(
        &fixture,
        Modifiers {
            control: true,
            alt: true,
            ..Modifiers::default()
        },
        cx,
    );
    cx.update_window(fixture.window.into(), |_, window, _| {
        assert!(window.try_find("keytip-expand-message-editor").is_none());
        assert!(window.try_find("keytip-sidebar-search").is_none());
    })
    .expect("mixed modifiers do not arm either group");

    hold(&fixture, Modifiers::control(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.open_dialog(cx, |dialog, _, _| {
            dialog.title("Modal scope").child("Keep focus here")
        });
    })
    .expect("open modal");
    draw(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert!(window.try_find("keytip-sidebar-search").is_none());
        assert!(window.try_find("keytip-sidebar-toggle").is_none());
        assert_eq!(
            fixture.view.read(cx).composer.input.read(cx).value(),
            "keep this draft"
        );
        window.press("escape", cx);
    })
    .expect("modal masks workspace shortcuts");
    hold(&fixture, Modifiers::default(), cx);
    cx.update_window(fixture.window.into(), |_, window, _| {
        assert!(window.try_find("keytip-sidebar-search").is_none());
        assert!(window.try_find("keytip-expand-message-editor").is_none());
    })
    .expect("release clears hints");
}

#[gpui_kit::test]
fn sidebar_cycle_hints_track_the_actual_keyboard_target_in_both_layouts(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let first = session();
    let second = session();
    attach(&fixture, first.clone(), cx);
    attach(&fixture, second.clone(), cx);
    hold(&fixture, Modifiers::control(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(
            window
                .find(format!("keytip-thread-next-{}", first.id))
                .label(),
            Some("Tab")
        );
        assert_eq!(
            window
                .find(format!("keytip-thread-previous-{}", first.id))
                .label(),
            Some("Shift+Tab")
        );
        window.press("ctrl-tab", cx);
    })
    .expect("follow the next-thread hint");
    draw(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(fixture.view.read(cx).selected.id, Some(first.id));
        assert!(
            window
                .try_find(format!("keytip-thread-next-{}", first.id))
                .is_none()
        );
        assert_eq!(
            window
                .find(format!("keytip-thread-next-{}", second.id))
                .label(),
            Some("Tab")
        );
        window.press("ctrl-b", cx);
    })
    .expect("collapse sidebar while hints are held");
    draw(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert!(!fixture.view.read(cx).window.sidebar_visible);
        assert_eq!(
            window
                .find(format!("keytip-thread-next-{}", second.id))
                .label(),
            Some("Tab")
        );
        assert_eq!(window.find("keytip-rail-new-session").label(), Some("N"));
        window.press("ctrl-shift-tab", cx);
    })
    .expect("follow the previous-thread hint in the rail");
    cx.update(|cx| assert_eq!(fixture.view.read(cx).selected.id, Some(second.id)));
}

#[gpui_kit::test]
fn deactivation_clears_hints_without_a_modifier_release(cx: &mut TestAppContext) {
    let first = fixture(cx);
    cx.update_window(first.window.into(), |_, window, _| window.activate_window())
        .expect("activate workspace");
    cx.run_until_parked();
    hold(&first, Modifiers::control(), cx);
    cx.update_window(first.window.into(), |_, window, _| {
        assert!(window.find("keytip-sidebar-search").visible());
    })
    .expect("armed hint");
    let second = fixture(cx);
    cx.update_window(second.window.into(), |_, window, _| {
        window.activate_window()
    })
    .expect("activate another window");
    cx.run_until_parked();
    draw(&first, cx);
    cx.update_window(first.window.into(), |_, window, _| {
        assert!(window.try_find("keytip-sidebar-search").is_none());
    })
    .expect("deactivated window clears hints");
}
