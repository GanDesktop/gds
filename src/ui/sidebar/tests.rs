use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use crate::ui::{Frame, navigation::NavItem};
use gpui_kit::{Context, IntoElement, Render, TestAppContext, Window, WindowHandle, px, size};
use gpui_kit::{TestSupportExt, assets::IconName, div, prelude::*, test::TestWindowExt};

use super::{SideBar, SidebarPresentation, SidebarState};

struct SidebarFixture {
    window: WindowHandle<SidebarView>,
    state: Rc<Cell<SidebarState>>,
    samples: Rc<RefCell<Vec<SidebarPresentation>>>,
}

struct SidebarView {
    state: Rc<Cell<SidebarState>>,
    samples: Rc<RefCell<Vec<SidebarPresentation>>>,
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = SideBar::default().state(self.state.get());
        let value = sidebar.sample("sidebar", window, cx);
        self.samples.borrow_mut().push(value);
        sidebar.presentation(value)
    }
}

impl SidebarFixture {
    fn open(cx: &mut TestAppContext, state: SidebarState) -> Self {
        let state = Rc::new(Cell::new(state));
        let samples = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(640.), px(480.)), {
            let state = state.clone();
            let samples = samples.clone();
            move |_, _| SidebarView { state, samples }
        });
        cx.run_until_parked();
        Self {
            window,
            state,
            samples,
        }
    }

    fn render(&self, cx: &mut TestAppContext, state: SidebarState) -> SidebarPresentation {
        self.state.set(state);
        self.window
            .update(cx, |_, window, _| window.refresh())
            .expect("refresh sidebar");
        cx.run_until_parked();
        self.latest()
    }

    fn latest(&self) -> SidebarPresentation {
        *self.samples.borrow().last().expect("sidebar rendered")
    }

    fn frame(&self, cx: &mut TestAppContext) -> usize {
        self.window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .expect("simulate sidebar frame")
    }
}

struct LayoutView {
    state: SidebarState,
}

impl Render for LayoutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = SideBar::default().state(self.state);
        let value = sidebar.sample("sidebar-layout", window, cx);
        let frame = Frame::default().sidebar_width(value.width());
        let rows = div().w_full().px(px(10.)).child(
            div().id("row-probe").test_support().w_full().child(
                NavItem::new(
                    "navigation",
                    IconName::Folder,
                    "A very long navigation title that must never wrap",
                )
                .presentation(value)
                .tooltip_enabled(false)
                .details(div().h(px(20.)).into_any_element())
                .trailing(vec![
                    div()
                        .id("row-action")
                        .test_support()
                        .size(px(32.))
                        .into_any_element(),
                ]),
            ),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                frame.toolbar(
                    div()
                        .id("menu-probe")
                        .test_support()
                        .size(px(32.))
                        .flex_shrink_0()
                        .into_any_element(),
                    div().into_any_element(),
                    cx,
                ),
            )
            .child(frame.workspace(
                sidebar.presentation(value).child(rows).into_any_element(),
                div().into_any_element(),
                cx,
            ))
            .child(frame.footer(
                div().into_any_element(),
                div().into_any_element(),
                div().into_any_element(),
                cx,
            ))
    }
}

#[gpui::test]
fn sidebar_shared_boundaries_and_navitem_slots_survive_intermediate_width(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
    });
    let window = cx.open_window(size(px(640.), px(480.)), |_, _| LayoutView {
        state: SidebarState::Expanded,
    });
    cx.run_until_parked();
    let (row, menu) = window
        .update(cx, |_, window, _| {
            assert!(window.try_find("row-action").is_some());
            (
                window.find("row-probe").bounds(),
                window.find("menu-probe").bounds(),
            )
        })
        .expect("expanded geometry");
    window
        .update(cx, |view, window, _| {
            view.state = SidebarState::Collapsed;
            window.refresh();
        })
        .expect("toggle sidebar");
    cx.run_until_parked();
    for elapsed in [60, 100] {
        cx.executor().advance_clock(Duration::from_millis(elapsed));
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .expect("advance layout");
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| {
                let toolbar = window.find("frame-toolbar-sidebar").bounds();
                let workspace = window.find("frame-workspace-sidebar").bounds();
                let footer = window.find("frame-footer-sidebar").bounds();
                assert_eq!(toolbar.right(), workspace.right());
                assert_eq!(workspace.right(), footer.right());
                let narrowed_row = window.find("row-probe").bounds();
                assert_eq!(row.origin.y, narrowed_row.origin.y);
                assert_eq!(row.size.height, narrowed_row.size.height);
                assert!(window.try_find("row-action").is_none());
                let moved_menu = window.find("menu-probe").bounds();
                assert_eq!(menu.origin.y, moved_menu.origin.y);
                if elapsed == 60 {
                    assert!(toolbar.size.width > px(58.) && toolbar.size.width < px(222.));
                    // GPUI rounds this one-pixel horizontal travel to device pixels.
                    assert!(moved_menu.origin.x >= menu.origin.x && moved_menu.origin.x <= px(13.));
                } else {
                    assert_eq!(toolbar.size.width, px(58.));
                    assert_eq!(moved_menu.center().x, toolbar.center().x);
                }
            })
            .expect("shared sidebar geometry");
    }
}

#[gpui::test]
fn sidebar_first_frame_adopts_either_endpoint_without_animation(cx: &mut TestAppContext) {
    for state in [SidebarState::Expanded, SidebarState::Collapsed] {
        let fixture = SidebarFixture::open(cx, state);
        assert_eq!(
            fixture.latest().width(),
            SideBar::default().state(state).width()
        );
        assert_eq!(fixture.frame(cx), 0);
    }
}

#[gpui::test]
fn sidebar_reversal_is_continuous_and_settles_without_frames(cx: &mut TestAppContext) {
    let fixture = SidebarFixture::open(cx, SidebarState::Expanded);
    let expanded = fixture.latest();
    assert_eq!(fixture.render(cx, SidebarState::Collapsed), expanded);
    cx.executor().advance_clock(Duration::from_millis(60));
    assert_eq!(fixture.frame(cx), 1);
    cx.run_until_parked();
    let middle = fixture.latest();
    assert!(middle.progress() > 0.0 && middle.progress() < 1.0);
    assert!(!middle.actions_visible());
    assert_eq!(fixture.render(cx, SidebarState::Expanded), middle);
    cx.executor().advance_clock(Duration::from_millis(20));
    fixture.frame(cx);
    cx.run_until_parked();
    assert!(fixture.latest().width() > middle.width());
    cx.executor().advance_clock(Duration::from_millis(160));
    fixture.frame(cx);
    cx.run_until_parked();
    assert_eq!(fixture.latest(), expanded);
    assert_eq!(fixture.frame(cx), 0);
}

#[gpui::test]
fn sidebar_reduce_motion_resolves_an_active_transition(cx: &mut TestAppContext) {
    let fixture = SidebarFixture::open(cx, SidebarState::Expanded);
    fixture.render(cx, SidebarState::Collapsed);
    cx.executor().advance_clock(Duration::from_millis(30));
    fixture.frame(cx);
    cx.run_until_parked();
    assert!(fixture.latest().progress() > 0.0);
    cx.update(|cx| cx.set_reduce_motion(true));
    let end = fixture.render(cx, SidebarState::Collapsed);
    assert_eq!(end.progress(), 0.0);
    fixture.frame(cx); // Drain the frame already scheduled before the preference changed.
    cx.run_until_parked();
    assert_eq!(fixture.frame(cx), 0);
    assert_eq!(fixture.render(cx, SidebarState::Expanded).progress(), 1.0);
    assert_eq!(fixture.frame(cx), 0);
}
