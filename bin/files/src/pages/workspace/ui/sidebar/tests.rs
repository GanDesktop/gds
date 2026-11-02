use super::{Browser, LaunchOptions, Location, OpenTab, Places, configuration, storage, tokens};
use gpui_kit::{
    App, AppContext, Bounds, Pixels, Point, TestAppContext, Window, component::Root, point, px,
    size, test::TestWindowExt,
};
use std::time::Duration;

struct Geometry {
    rows: Vec<Bounds<Pixels>>,
    icon: Bounds<Pixels>,
    scroll: [Point<Pixels>; 3],
    split: f32,
}

impl Geometry {
    fn capture(browser: &Browser, window: &Window, cx: &App) -> Self {
        let width = window.find("frame-toolbar-sidebar").bounds().right();
        assert!((width - px(browser.width())).abs() <= px(1.));
        for id in [
            "frame-toolbar-sidebar",
            "frame-workspace-sidebar",
            "frame-footer-sidebar",
        ] {
            let columns = window.find_all(id);
            assert!(!columns.is_empty(), "missing column {id}");
            for column in columns {
                assert!((column.bounds().right() - width).abs() < px(0.1), "{id}");
            }
        }
        assert_eq!(window.find("files-terminal-tabs").bounds().right(), width);
        assert_eq!(
            window.find("files-terminal-divider").bounds().right(),
            width
        );
        Self {
            rows: vec![
                window.find(("tab", 4_u64)).bounds(),
                window.find(("tab", 5_u64)).bounds(),
                window.find("sidebar-volume-fixture-1").bounds(),
                window.find("sidebar-volume-fixture-2").bounds(),
                window.find("home").bounds(),
                window.find("recent").bounds(),
                window.find(("files-terminal-row", 0_u64)).bounds(),
            ],
            icon: window.find(("tab-icon", 4_u64)).bounds(),
            scroll: [
                browser.sidebar.regions.tabs_scroll.offset(),
                browser.sidebar.regions.volumes_scroll.offset(),
                browser.sidebar.regions.locations_scroll.offset(),
            ],
            split: browser.sidebar.split.read(cx).height(),
        }
    }

    fn assert_vertical_layout(&self, other: &Self) {
        for (before, after) in self.rows.iter().zip(&other.rows) {
            assert_eq!(before.origin.y, after.origin.y, "row Y moved");
            assert_eq!(before.size.height, after.size.height, "row height changed");
        }
        assert_eq!(self.icon.origin.y, other.icon.origin.y);
        assert_eq!(self.scroll, other.scroll);
        assert_eq!(self.split, other.split);
    }
}

#[gpui_kit::test]
async fn sidebar_motion_keeps_files_geometry_scroll_and_terminal_boundary(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = tempfile::tempdir().expect("sidebar fixture");
    // Device discovery stays unpolled: no system bus or real devices are used.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unpolled device runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1100.), px(1000.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut preferences = configuration::Preferences::default();
            preferences.files.sidebar.tabs_height = Some(160.);
            preferences.files.sidebar.show_volumes = true;
            preferences.terminal.shell = Some(root.path().join("missing-test-shell"));
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(Places::default()),
                    start: root.path().to_owned(),
                    home: root.path().to_owned(),
                    preferences,
                    startup_error: None,
                    picker: None,
                },
                window,
                cx,
            )
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.expect("Files workspace");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |this, cx| {
            for id in 2..25 {
                this.tabs
                    .items
                    .push(OpenTab::new(id, Location::Recent, None, false));
            }
            this.tabs.next_id = 25;
            this.devices.service.update(cx, |service, cx| {
                service.snapshot.volumes = (0..8)
                    .map(|index| storage::Volume {
                        id: storage::VolumeId {
                            stable: Some(format!("fixture-{index}")),
                            attachment: format!("fixture-{index}"),
                        },
                        object: format!("fixture-{index}"),
                        drive: None,
                        name: format!("Long mounted volume name {index}"),
                        size: 0,
                        label: String::new(),
                        filesystem: "test".into(),
                        read_only: false,
                        locked: false,
                        mountable: true,
                        system_mount: false,
                        mount_points: if index % 2 == 0 {
                            vec![root.path().to_owned()]
                        } else {
                            Vec::new()
                        },
                        visible: true,
                    })
                    .collect();
                cx.notify();
            });
            this.toggle_terminal(window, cx);
            cx.notify();
        });
        window.render_frame(cx);
    })
    .expect("populate sidebar");
    cx.run_until_parked();
    // Measure the sidebar only after the terminal's height transition settles.
    cx.executor().advance_clock(super::super::motion::DURATION);
    cx.update_window(window.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
    })
    .expect("finish terminal reveal");
    cx.run_until_parked();
    let initial = cx
        .update_window(window.into(), |_, window, cx| {
            browser.update(cx, |this, _| {
                this.sidebar
                    .regions
                    .tabs_scroll
                    .set_offset(point(px(0.), px(-100.)));
                this.sidebar
                    .regions
                    .volumes_scroll
                    .set_offset(point(px(0.), px(-50.)));
            });
            window.render_frame(cx);
            window.hover(("tab", 4_u64), cx);
            assert!(window.try_find(("close-tab", 4_u64)).is_some());
            Geometry::capture(browser.read(cx), window, cx)
        })
        .expect("measure expanded layout");
    assert!(initial.scroll[0].y < px(0.));
    assert!(initial.scroll[1].y < px(0.));
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |this, cx| {
            this.sidebar.collapsed = true;
            cx.notify();
        });
        window.render_frame(cx);
    })
    .expect("collapse sidebar");
    cx.executor().advance_clock(Duration::from_millis(60));
    cx.update_window(window.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
    })
    .expect("advance sidebar");
    cx.run_until_parked();
    let middle = cx
        .update_window(window.into(), |_, window, cx| {
            let this = browser.read(cx);
            assert!(
                this.sidebar.presentation.progress() > 0.
                    && this.sidebar.presentation.progress() < 1.
            );
            assert!(window.try_find(("close-tab", 4_u64)).is_none());
            assert!(window.try_find(("close-tab-corner", 4_u64)).is_none());
            Geometry::capture(this, window, cx)
        })
        .expect("measure intermediate layout");
    initial.assert_vertical_layout(&middle);
    assert!(middle.icon.origin.x > initial.icon.origin.x);
    let middle_width = cx.update(|cx| browser.read(cx).width());
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |this, cx| {
            this.sidebar.collapsed = false;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(browser.read(cx).width(), middle_width, "reversal jumps");
        initial.assert_vertical_layout(&Geometry::capture(browser.read(cx), window, cx));
    })
    .expect("reverse in flight");
    // Reduced motion must resolve every host column in this same frame.
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |this, cx| {
            this.sidebar.collapsed = true;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(browser.read(cx).width(), tokens::COLLAPSED_SIDEBAR_WIDTH);
        let collapsed = Geometry::capture(browser.read(cx), window, cx);
        let meter = (0..8)
            .step_by(2)
            .filter_map(|index| window.try_find(format!("sidebar-volume-fixture-{index}-capacity")))
            .find(|meter| meter.visible())
            .expect("collapsed volumes keep a visible capacity meter");
        let bounds = meter.bounds().scale(window.scale_factor());
        let track: gpui_kit::Background = {
            use gpui_kit::component::ActiveTheme;
            let color: gpui_kit::Background = cx.theme().tokens.progress_bar.into();
            color.opacity(0.2)
        };
        assert!(
            window
                .painted_quads()
                .iter()
                .any(|quad| quad.bounds == bounds && quad.background == track),
            "capacity track must not fade with sidebar labels"
        );
        initial.assert_vertical_layout(&collapsed);
        assert!(collapsed.icon.origin.x >= middle.icon.origin.x);
        assert!(
            (collapsed.icon.center().x - px(tokens::COLLAPSED_SIDEBAR_WIDTH / 2.)).abs() <= px(1.)
        );
        // Group collapse preferences and tab identities are independent of the column target.
        assert!(
            !browser
                .read(cx)
                .settings
                .preferences
                .files
                .sidebar
                .locations_collapsed
        );
        assert!(
            !browser
                .read(cx)
                .settings
                .preferences
                .files
                .sidebar
                .volumes_collapsed
        );
        assert_eq!(browser.read(cx).tabs.items.len(), 24);
        window.click("recent", cx);
        assert_eq!(browser.read(cx).current_location(), Location::Recent);
        assert_eq!(browser.read(cx).tabs.items.len(), 24);
    })
    .expect("collapsed layout and navigation");
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |this, cx| {
            this.settings.preferences.files.sidebar.locations_collapsed = true;
            this.settings.preferences.files.sidebar.volumes_collapsed = true;
            this.sidebar.collapsed = false;
            this.window.focus.focus(window, cx);
            cx.notify();
        });
        window.render_frame(cx);
        assert!(
            window.try_find("home").is_none(),
            "width must not expand locations"
        );
        assert!(
            window.try_find("sidebar-volume-fixture-1").is_none(),
            "width must not expand volumes"
        );
        browser.update(cx, |this, cx| {
            this.show_page(super::super::pages::Page::Settings, window, cx);
            cx.notify();
        });
        for collapsed in [true, false] {
            browser.update(cx, |this, cx| {
                this.sidebar.collapsed = collapsed;
                cx.notify();
            });
            window.render_frame(cx);
            let width = px(browser.read(cx).width());
            for id in [
                "frame-toolbar-sidebar",
                "frame-workspace-sidebar",
                "frame-footer-sidebar",
            ] {
                assert_eq!(
                    window.find(id).bounds().right(),
                    width,
                    "settings column {id}"
                );
            }
            window.click(("settings-page", 1_usize), cx);
            assert_eq!(
                browser.read(cx).tabs.items.len(),
                24,
                "settings must not mutate file tabs"
            );
        }
    })
    .expect("collapsed groups and settings share host state");
    let shutdown = cx.update(|cx| {
        let panel = browser
            .read(cx)
            .terminal
            .dock
            .as_ref()
            .expect("terminal dock")
            .panel
            .clone();
        panel.update(cx, |panel, cx| panel.shutdown(cx))
    });
    shutdown.await;
}
