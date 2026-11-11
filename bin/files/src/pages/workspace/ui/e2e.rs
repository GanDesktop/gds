//! Native-window scenarios, compiled only for the isolated E2E runner.
use super::*;
use futures::{StreamExt, channel::mpsc, future};
use gds::app::AppExt as _;
use gpui_kit::test::{ClickOptions, TestWindowExt};
use std::fs;

pub(crate) fn start(window: AnyWindowHandle, browser: Entity<Browser>, cx: &mut App) {
    let artifacts =
        PathBuf::from(std::env::var_os("GDS_E2E_ARTIFACTS").expect("E2E runner directory"));
    cx.spawn(async move |cx| {
        let mut session = Session {
            window,
            browser,
            artifacts,
            cx: cx.clone(),
        };
        session.workflow().await;
        session.cx.update(|cx| cx.gds().tasks().request_quit());
    })
    .detach();
}

struct Session {
    window: AnyWindowHandle,
    browser: Entity<Browser>,
    artifacts: PathBuf,
    cx: AsyncApp,
}

impl Session {
    async fn workflow(&mut self) {
        self.wait_for(|browser, _, _| {
            browser
                .active()
                .is_some_and(|tab| tab.table_rows.len() == 1)
        })
        .await;
        self.capture("01-open").await;
        self.step(|window, cx| {
            window.click(("file-name", 0usize), cx);
            window.press("f2", cx);
        })
        .await;
        self.wait_for(|_, window, cx| window.has_active_dialog(cx))
            .await;
        self.step(|window, cx| {
            window.press("ctrl-a", cx);
            window.input("renamed.txt", cx);
            window.press("enter", cx);
        })
        .await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "renamed.txt"))
            .await;
        assert_eq!(
            fs::read(self.artifacts.join("home/work/renamed.txt")).expect("renamed payload"),
            b"original payload\n"
        );
        assert!(!self.artifacts.join("home/work/original.txt").exists());
        self.capture("02-renamed").await;

        eprintln!("E2E action: undo rename");
        self.step(|window, cx| window.press("ctrl-z", cx)).await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "original.txt"))
            .await;
        eprintln!("E2E action: redo rename");
        self.step(|window, cx| window.press("ctrl-shift-z", cx))
            .await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "renamed.txt"))
            .await;

        eprintln!("E2E action: create folder");
        self.step(|window, cx| window.press("ctrl-shift-n", cx))
            .await;
        self.wait_for(|_, window, cx| window.has_active_dialog(cx))
            .await;
        self.step(|window, cx| {
            window.press("ctrl-a", cx);
            window.input("created", cx);
            eprintln!("E2E action: submit folder name");
            window.press("enter", cx);
        })
        .await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "created"))
            .await;
        assert!(self.artifacts.join("home/work/created").is_dir());

        self.step(|window, cx| {
            window.click(("file-name", 1usize), cx);
            window.press("delete", cx);
        })
        .await;
        self.wait_for(|browser, _, _| {
            !browser.operations.in_progress && !Self::has_file(browser, "renamed.txt")
        })
        .await;
        assert!(!self.artifacts.join("home/work/renamed.txt").exists());
        self.step(|window, cx| window.press("ctrl-z", cx)).await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "renamed.txt"))
            .await;
        assert_eq!(
            fs::read(self.artifacts.join("home/work/renamed.txt")).expect("restored payload"),
            b"original payload\n"
        );
        self.capture("03-restored").await;

        fs::write(
            self.artifacts.join("home/work/download.txt"),
            b"external download\n",
        )
        .expect("external writer");
        self.wait_for(|browser, _, _| Self::has_file(browser, "download.txt"))
            .await;
        self.capture("04-watched-create").await;
        self.step(|window, cx| window.press("ctrl-g", cx)).await;
        self.wait_for(|browser, _, _| {
            browser
                .active()
                .is_some_and(|tab| tab.view_mode == ViewMode::Grid)
        })
        .await;
        self.capture("05-grid").await;

        self.step(|window, cx| window.press("ctrl-n", cx)).await;
        self.wait_for(|browser, _, _| browser.tabs.items.len() == 2)
            .await;
        let ids = self.cx.update(|cx| {
            self.browser
                .read(cx)
                .tabs
                .items
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<_>>()
        });
        self.step(|window, cx| {
            let source = window.find(("tab", ids[0])).bounds().center();
            let target = window.find(("tab", ids[1])).bounds();
            window.drag(
                source,
                point(target.center().x, target.bottom() - px(3.)),
                cx,
            );
        })
        .await;
        self.wait_for(|browser, _, _| browser.tabs.items[0].id == ids[1])
            .await;
        self.capture("06-reordered-tabs").await;
        self.search_on_typing().await;
        self.search_result_navigation().await;
        self.location_history().await;
        fs::write(self.artifacts.join("passed.txt"), "PASS: rename, undo, redo, create folder, trash, restore, filesystem watch, grid, tab reorder, search on typing, IME protocol, search result navigation, mouse navigation across locations\n").expect("result artifact");
    }

    async fn search_on_typing(&mut self) {
        let browser = self.browser.clone();
        self.step(move |window, cx| {
            browser.update(cx, |browser, cx| {
                browser.window.focus.focus(window, cx);
            });
            window.render_frame(cx);
            window.input("x", cx);
            assert_eq!(browser.read(cx).toolbar.filter.input.read(cx).value(), "");
            browser.update(cx, |browser, cx| {
                let mut preferences = browser.settings.preferences.clone();
                preferences.files.auto_focus_search = true;
                browser.apply_preferences(preferences, window, cx);
            });
        })
        .await;

        let browser = self.browser.clone();
        self.step(move |window, cx| {
            window.press("ctrl-n", cx);
            assert!(browser.read(cx).window.focus.is_focused(window));
            window.press("down", cx);
            assert!(
                !browser
                    .read(cx)
                    .toolbar
                    .filter
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
            window.input("ab", cx);
            assert_eq!(browser.read(cx).toolbar.filter.input.read(cx).value(), "ab");
            assert!(
                browser
                    .read(cx)
                    .toolbar
                    .filter
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        })
        .await;
        self.capture("07-type-to-search").await;

        let browser = self.browser.clone();
        self.step(move |window, cx| {
            browser.update(cx, |browser, cx| {
                browser
                    .toolbar
                    .filter
                    .input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                browser.window.focus.focus(window, cx);
            });
            window.render_frame(cx);
            let bounds = window.find("directory-filter").bounds();
            // Start composition at the list, then continue through the focused field.
            gpui_kit::test::TestInput::new(
                ElementInputHandler::new(bounds, browser.clone()),
                window,
            )
            .compose_text(None, "zhong", Some(5..5), cx);
            let input = browser.read(cx).toolbar.filter.input.clone();
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
            let mut ime = gpui_kit::test::TestInput::new(
                ElementInputHandler::new(bounds, input.clone()),
                window,
            );
            assert_eq!(ime.marked_text_range(cx), Some(0..5));
            ime.commit_text("中", cx);
            assert_eq!(input.read(cx).value(), "中");
            assert_eq!(ime.marked_text_range(cx), None);
        })
        .await;
        self.capture("08-search-ime").await;
    }

    async fn search_result_navigation(&mut self) {
        let browser = self.browser.clone();
        let path = self.artifacts.join("home/work");
        self.step(move |window, cx| {
            browser.update(cx, |browser, cx| browser.open_tab(path, false, window, cx));
        })
        .await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "download.txt"))
            .await;
        let browser = self.browser.clone();
        self.step(|window, cx| window.input(".txt", cx)).await;
        self.wait_for(|browser, _, _| {
            browser
                .active()
                .is_some_and(|tab| tab.table_rows.len() == 2)
        })
        .await;
        self.step(move |window, cx| {
            window.press("down", cx);
            let state = browser.read(cx);
            let tab = state.active().expect("search tab");
            assert!(state.window.focus.is_focused(window));
            assert_eq!(
                tab.navigation.view().selected,
                vec![tab.table_rows[0].path.clone()]
            );
            window.press("down", cx);
            let tab = browser.read(cx).active().expect("search tab");
            assert_eq!(
                tab.navigation.view().selected,
                vec![tab.table_rows[1].path.clone()]
            );
            window.press("ctrl-f", cx);
            window.press("up", cx);
            let state = browser.read(cx);
            let tab = state.active().expect("search tab");
            assert!(state.window.focus.is_focused(window));
            assert_eq!(
                tab.navigation.view().selected,
                vec![tab.table_rows.last().expect("last result").path.clone()]
            );
            window.input("missing-result", cx);
        })
        .await;
        self.wait_for(|browser, _, _| {
            browser
                .active()
                .is_some_and(|tab| tab.table_rows.is_empty())
        })
        .await;
        let browser = self.browser.clone();
        self.step(move |window, cx| {
            window.press("down", cx);
            assert!(
                browser
                    .read(cx)
                    .toolbar
                    .filter
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        })
        .await;

        let browser = self.browser.clone();
        self.step(move |window, cx| {
            window.press("ctrl-a", cx);
            window.press("backspace", cx);
        })
        .await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "download.txt"))
            .await;
        self.step(move |window, cx| {
            let bounds = window.find("directory-filter").bounds();
            let input = browser.read(cx).toolbar.filter.input.clone();
            gpui_kit::test::TestInput::new(ElementInputHandler::new(bounds, input.clone()), window)
                .compose_text(None, "down", Some(4..4), cx);
            window.press("down", cx);
            window.press("up", cx);
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
            gpui_kit::test::TestInput::new(ElementInputHandler::new(bounds, input.clone()), window)
                .commit_text("download", cx);
        })
        .await;
        self.wait_for(|browser, _, _| {
            browser
                .active()
                .is_some_and(|tab| tab.table_rows.len() == 1)
        })
        .await;
        let browser = self.browser.clone();
        self.step(move |window, cx| {
            window.press("down", cx);
            assert!(browser.read(cx).window.focus.is_focused(window));
            assert_eq!(
                browser
                    .read(cx)
                    .active()
                    .expect("search tab")
                    .navigation
                    .view()
                    .selected,
                vec![
                    browser.read(cx).active().expect("search tab").table_rows[0]
                        .path
                        .clone()
                ]
            );
            window.press("ctrl-f", cx);
            window.input("created", cx);
        })
        .await;
        self.wait_for(|browser, _, _| Self::has_file(browser, "created"))
            .await;
        self.step(|window, cx| {
            window.press("down", cx);
            window.press("enter", cx);
        })
        .await;
        self.wait_for(|browser, _, _| {
            browser
                .current_path()
                .is_some_and(|path| path.ends_with("work/created"))
        })
        .await;
        self.capture("09-search-open-folder").await;
    }

    async fn location_history(&mut self) {
        // Disabled tags still expose navigation, without needing an index service.
        self.open_address("tags:/").await;
        self.open_address("tags:/工作").await;
        self.open_address("tags:/工作/设计").await;
        self.mouse_history(NavigationDirection::Back, "tags:/工作")
            .await;
        self.mouse_history(NavigationDirection::Back, "tags:/")
            .await;
        self.mouse_history(NavigationDirection::Forward, "tags:/工作")
            .await;
        self.mouse_history(NavigationDirection::Forward, "tags:/工作/设计")
            .await;
        self.open_address("location:/recent").await;
        self.mouse_history(NavigationDirection::Back, "tags:/工作/设计")
            .await;
        self.mouse_history(NavigationDirection::Forward, "location:/recent")
            .await;
        self.capture("10-location-mouse-history").await;
    }

    async fn open_address(&mut self, address: &str) {
        self.step(|window, cx| {
            window.press("ctrl-l", cx);
            window.input(address, cx);
            window.press("enter", cx);
        })
        .await;
        self.wait_for_address(address).await;
    }

    async fn mouse_history(&mut self, direction: NavigationDirection, address: &str) {
        self.step(|window, cx| {
            window.click_with_options(
                "page-title",
                ClickOptions::new().with_button(MouseButton::Navigate(direction)),
                cx,
            );
        })
        .await;
        self.wait_for_address(address).await;
    }

    async fn wait_for_address(&mut self, address: &str) {
        let expected = Location::from_uri(address).expect("scenario address");
        self.wait_for(|browser, _, _| {
            browser.current_location() == expected
                && browser
                    .current_tab()
                    .is_some_and(|tab| !tab.navigation.is_loading())
        })
        .await;
    }

    fn has_file(browser: &Browser, name: &str) -> bool {
        !browser.operations.in_progress
            && browser
                .active()
                .is_some_and(|tab| tab.table_rows.iter().any(|entry| entry.name == name))
    }

    /// Wait for application notifications, including one initial observation.
    /// The outer runner supplies a deadline; there is no polling timer.
    async fn wait_for(&mut self, predicate: impl Fn(&Browser, &mut Window, &mut App) -> bool) {
        let (events, receiver) = mpsc::unbounded();
        events.unbounded_send(()).expect("initial observation");
        let root_events = events.clone();
        let root_subscription = self
            .cx
            .update_window(self.window, |_, window, cx| {
                let root = window
                    .root::<gpui_kit::component::Root>()
                    .expect("window root")
                    .expect("kit root");
                cx.observe(&root, move |_, _| {
                    root_events
                        .unbounded_send(())
                        .expect("active root observation");
                })
            })
            .expect("native window");
        let subscription = self.cx.update(|cx| {
            cx.observe(&self.browser, move |_, _| {
                events.unbounded_send(()).expect("active observation");
            })
        });
        receiver
            .filter_map(|()| {
                let ready = self
                    .cx
                    .update_window(self.window, |_, window, cx| {
                        window.render_frame(cx);
                        self.browser
                            .update(cx, |browser, cx| predicate(browser, window, cx))
                    })
                    .expect("native window");
                future::ready(ready.then_some(()))
            })
            .next()
            .await
            .expect("application notification");
        drop(subscription);
        drop(root_subscription);
        self.step(|_, _| {}).await;
    }

    async fn step(&mut self, action: impl FnOnce(&mut Window, &mut App)) {
        let (done, frame) = futures::channel::oneshot::channel();
        self.cx
            .update_window(self.window, |_, window, cx| {
                window.render_frame(cx);
                action(window, cx);
                window.on_next_frame(move |_, _| done.send(()).expect("awaited frame"));
                window.refresh();
            })
            .expect("native input dispatch");
        frame.await.expect("native frame");
    }

    async fn capture(&mut self, name: &str) {
        eprintln!("E2E checkpoint: {name}");
        let directory = self.artifacts.join(name);
        fs::create_dir(&directory).expect("checkpoint directory");
        self.cx
            .update_window(self.window, |_, window, _| {
                fs::write(
                    directory.join("elements.txt"),
                    format!("{:#?}", gpui_kit::base::test_support::snapshots(window)),
                )
                .expect("semantic artifact");
            })
            .expect("native window");
        let tasks = self
            .cx
            .update(|cx| self.browser.read(cx).window.tasks.clone());
        let capture =
            std::env::var_os("GDS_E2E_SCREENSHOT").expect("compositor screenshot command");
        let output = tasks
            .spawn_named("Capture test screenshot", async move {
                tokio::process::Command::new(capture)
                    .current_dir(directory)
                    .output()
                    .await
                    .expect("compositor capture")
            })
            .await
            .expect("screenshot task");
        assert!(
            output.status.success(),
            "screenshot failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
