use super::{DragBarState, WindowControls};

#[test]
fn niri_session_hides_the_entire_control_group() {
    for desktop in ["niri", "Niri", "niri:GNOME", " custom : niri "] {
        assert!(WindowControls::session_hides_controls(Some(desktop), false));
    }
}

#[test]
fn declared_desktop_takes_precedence_over_inherited_niri_socket() {
    for desktop in ["GNOME", "KDE", "sway", "not-niri"] {
        assert!(!WindowControls::session_hides_controls(Some(desktop), true));
    }
}

#[test]
fn missing_desktop_uses_the_niri_socket_as_a_fallback() {
    assert!(WindowControls::session_hides_controls(None, true));
    assert!(WindowControls::session_hides_controls(Some(" "), true));
    assert!(!WindowControls::session_hides_controls(None, false));
}

#[test]
fn dragging_requires_left_button_still_held_on_mouse_move() {
    let state = DragBarState::default();
    state.mouse_down(false);
    // Released outside the drag bar and re-entered with no button held:
    // clears the armed flag without initiating a ghost window move.
    assert!(!state.should_start_window_move(false));
    assert!(!state.should_start_window_move(true));

    // Normal press + drag with left button held fires once, then clears.
    state.mouse_down(false);
    assert!(state.should_start_window_move(true));
    assert!(!state.should_start_window_move(true));
}

#[test]
fn prevented_left_press_blocks_double_click_across_mouse_up() {
    let state = DragBarState::default();
    state.mouse_down(true);
    state.mouse_up();
    assert!(!state.take_double_click(false));

    // Unprevented press survives `mouse_up` until `take_double_click` consumes it once.
    state.mouse_down(false);
    state.mouse_up();
    assert!(state.take_double_click(false));
    assert!(!state.take_double_click(false));

    // A mouse-down outside or a prevented release still suppresses double-click.
    state.mouse_down(false);
    state.mouse_down_out();
    assert!(!state.take_double_click(false));

    state.mouse_down(false);
    state.mouse_up();
    assert!(!state.take_double_click(true));
}

#[cfg(feature = "gui-tests")]
mod gui_tests {
    use super::super::TitleBar;
    use crate::{app::AppExt as _, ui::WindowDragArea};
    use gpui_kit::{
        AppContext as _, Context, InteractiveElement as _, IntoElement, Modifiers, MouseButton,
        MouseDownEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, Styled as _,
        TestAppContext, TestSupportExt as _, VisualTestContext, Window, div, point, px, size,
        test::TestWindowExt,
    };

    fn simulate_click_count(
        visual: &mut VisualTestContext,
        position: Point<Pixels>,
        click_count: usize,
    ) {
        visual.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        visual.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
        });
    }

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    TitleBar::default()
                        .child(div().id("title-bar-slot").test_support().size_full()),
                )
                .child(
                    div()
                        .relative()
                        .h(px(40.))
                        .w_full()
                        .child(
                            div()
                                .id("drag-area-slot")
                                .test_support()
                                .absolute()
                                .inset_0()
                                .child(WindowDragArea),
                        )
                        .child(
                            div()
                                .id("drag-blocker")
                                .test_support()
                                .relative()
                                .w(px(100.))
                                .h_full()
                                .on_mouse_down(MouseButton::Left, |_, window, _| {
                                    window.prevent_default();
                                }),
                        ),
                )
        }
    }

    fn open_test_host(
        cx: &mut TestAppContext,
    ) -> (
        VisualTestContext,
        Point<Pixels>,
        Point<Pixels>,
        Point<Pixels>,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.gds_mut()
                .i18n()
                .install(crate::i18n::Language::English)
                .expect("translations");
            cx.gds_mut()
                .appearance()
                .theme()
                .apply(&crate::appearance::theme::Appearance {
                    source: crate::appearance::theme::Source::Builtin,
                    ..Default::default()
                })
                .expect("appearance");
        });
        let window = cx.open_window(size(px(400.), px(200.)), |_, _| Host);
        let (title_center, drag_center, blocker_center) = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                (
                    window.find("title-bar-slot").bounds().center(),
                    window.find("drag-area-slot").bounds().center(),
                    window.find("drag-blocker").bounds().center(),
                )
            })
            .expect("rendered bounds");
        (
            VisualTestContext::from_window(window.into(), cx),
            title_center,
            drag_center,
            blocker_center,
        )
    }

    #[gpui_kit::test]
    fn titlebar_and_drag_area_clear_armed_drag_without_ghost_move(cx: &mut TestAppContext) {
        let (mut visual, title_center, drag_center, blocker_center) = open_test_host(cx);
        let outside = point(px(200.), px(150.));

        for target in [title_center, drag_center] {
            // Press inside the drag bar, release outside, then hover back unpressed:
            // `TestWindow::start_window_move` is `unimplemented!()`, so calling it would panic.
            visual.simulate_mouse_down(target, MouseButton::Left, Modifiers::default());
            visual.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
            visual.simulate_mouse_move(target, None, Modifiers::default());
            // Re-verify `dragging` was cleared so a later left-held move over it still does not move.
            visual.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
        }

        // Pressing over a control above `WindowDragArea` that calls `prevent_default()` must
        // not arm drag either.
        visual.simulate_mouse_down(blocker_center, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(drag_center, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(drag_center, MouseButton::Left, Modifiers::default());
    }

    #[gpui_kit::test]
    fn double_clicking_prevented_control_above_drag_area_does_not_zoom(cx: &mut TestAppContext) {
        let (mut visual, _, drag_center, blocker_center) = open_test_host(cx);

        // `drag-blocker` only calls `window.prevent_default()` on `MouseDownEvent`, not
        // `MouseUpEvent`. `TestWindow::zoom` is `unimplemented!()` on non-macOS, so dispatching
        // double-click without honoring press prevention would panic here.
        simulate_click_count(&mut visual, blocker_center, 1);
        simulate_click_count(&mut visual, blocker_center, 2);

        // Even if the first click landed on the empty drag area, a prevented second press must
        // overwrite `press_allowed` and still suppress double-click zoom.
        simulate_click_count(&mut visual, drag_center, 1);
        simulate_click_count(&mut visual, blocker_center, 2);
    }
}
