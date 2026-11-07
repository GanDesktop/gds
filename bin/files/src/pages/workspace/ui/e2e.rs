//! Native-window scenarios, compiled only for the isolated E2E runner.
use super::*;
use futures::{StreamExt, channel::mpsc, future};
use gpui_kit::test::TestWindowExt;
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
        session.cx.update(|cx| cx.quit());
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
        fs::write(self.artifacts.join("passed.txt"), "PASS: rename, undo, redo, create folder, trash, restore, filesystem watch, grid, tab reorder\n").expect("result artifact");
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
        let runtime = self
            .cx
            .update(|cx| self.browser.read(cx).window.runtime.clone());
        let capture =
            std::env::var_os("GDS_E2E_SCREENSHOT").expect("compositor screenshot command");
        let output = runtime
            .spawn(async move {
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
