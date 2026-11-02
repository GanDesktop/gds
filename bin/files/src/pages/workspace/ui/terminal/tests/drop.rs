use crate::pages::workspace::ui::{Browser, LaunchOptions, Places, configuration};
use gpui_kit::{
    AppContext, ExternalPaths, FileDropEvent, PlatformInput, TestAppContext, component::Root, px,
    size, test::TestWindowExt,
};
use std::time::{Duration, Instant};

#[gpui_kit::test]
async fn internal_and_external_drops_reach_the_live_terminal(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("drop fixture");
    let folder = directory.path().join("folder ' with spaces");
    let file = directory.path().join("file $literal.txt");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(&file, "fixture").unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|path| path.join("sh"))
        .find(|path| path.is_absolute() && path.is_file())
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        gds::ui::terminal::init(cx);
        cx.set_reduce_motion(true);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).unwrap();
    });
    let mut browser = None;
    let handle = cx.open_window(size(px(1100.), px(800.)), |window, cx| {
        let mut preferences = configuration::Preferences::default();
        preferences.terminal.shell = Some(shell);
        preferences.terminal.shell_args = vec!["-c".into(),
            r#"stty -echo; printf '\033]2;drop-ready\007'; while IFS= read -r line; do printf '%s\n' "$line" >> received; done"#.into()];
        let view = cx.new(|cx| Browser::new(LaunchOptions {
            runtime: runtime.handle().clone(), places: Ok(Places::default()),
            start: directory.path().to_owned(), home: directory.path().to_owned(),
            preferences, startup_error: None, picker: None,
        }, window, cx));
        view.update(cx, |view, cx| view.toggle_terminal(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    let panel = cx.update(|cx| {
        browser
            .read(cx)
            .terminal
            .dock
            .as_ref()
            .unwrap()
            .panel
            .clone()
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        cx.run_until_parked();
        if cx.update(|cx| panel.read(cx).active_title(cx).as_ref() == "drop-ready") {
            break;
        }
        assert!(Instant::now() < deadline, "terminal startup");
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut expected = String::new();
    for (external, path) in [(false, folder), (true, file)] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let position = window.find("files-terminal-input").bounds().center();
            if external {
                window.dispatch_event(
                    PlatformInput::FileDrop(FileDropEvent::Entered {
                        position,
                        paths: ExternalPaths([path.clone()].into_iter().collect()),
                    }),
                    cx,
                );
                window.render_frame(cx);
                window.dispatch_event(
                    PlatformInput::FileDrop(FileDropEvent::Submit { position }),
                    cx,
                );
            } else {
                let index = browser
                    .read(cx)
                    .active()
                    .unwrap()
                    .table_rows
                    .iter()
                    .position(|entry| entry.path == path)
                    .unwrap();
                window.drag_to(("file-name", index), "files-terminal-input", cx);
            }
            assert!(!cx.has_active_drag(), "terminal consumes dropped paths");
            window.render_frame(cx);
            // Drop focuses the PTY and inserts only text; Enter submits it separately.
            window.press("enter", cx);
        })
        .expect("drop over terminal cells");
        expected.push_str(&gds::service::terminal::input::path_text(&[path]).unwrap());
        expected.push('\n');
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.run_until_parked();
            let received =
                std::fs::read_to_string(directory.path().join("received")).unwrap_or_default();
            if received == expected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "PTY received {received:?}, expected {expected:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let shutdown = cx.update(|cx| panel.update(cx, |panel, cx| panel.shutdown(cx)));
    shutdown.await;
}
