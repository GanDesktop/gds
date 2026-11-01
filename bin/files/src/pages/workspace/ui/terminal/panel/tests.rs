use super::Panel;
use gpui_kit::{AppContext, TestAppContext, component::Root, px, size};
use std::task::{Context as TaskContext, Poll, Waker};

#[gpui_kit::test]
async fn f2_from_terminal_input_confirms_and_cancels_instance_rename(cx: &mut TestAppContext) {
    use gpui_kit::{component::WindowExt, test::TestWindowExt};
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("rename terminal directory");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::features::file_operations::commands::register(cx);
        gds::ui::terminal::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx)
            .expect("rename translations");
    });
    let panel = cx.update(|cx| cx.new(Panel::new));
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        Root::new(panel.clone(), window, cx)
    });
    cx.update_window(window.into(), |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.preferences.shell = Some(directory.path().join("missing-shell"));
            panel.open(directory.path().to_path_buf(), window, cx);
        });
        window.render_frame(cx);
        window.press("f2", cx);
        assert!(
            window.has_active_dialog(cx),
            "F2 must reach instance rename, not the PTY"
        );
    })
    .expect("open rename dialog");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        for key in ["shift-b", "u", "i", "l", "d"] {
            window.press(key, cx);
        }
        window.press("enter", cx);
        assert!(!window.has_active_dialog(cx));
        assert_eq!(
            panel.read(cx).entries[0].custom_name.as_deref(),
            Some("Build")
        );
        panel.update(cx, |panel, cx| panel.focus(window, cx));
        window.render_frame(cx);
        window.press("f2", cx);
        assert!(window.has_active_dialog(cx));
    })
    .expect("confirm rename and reopen dialog");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.input("Discard", cx);
        window.press("escape", cx);
        assert!(!window.has_active_dialog(cx));
        assert_eq!(
            panel.read(cx).entries[0].custom_name.as_deref(),
            Some("Build")
        );
    })
    .expect("terminal rename keyboard workflow");
    let shutdown = cx.update(|cx| panel.update(cx, |panel, cx| panel.shutdown(cx)));
    shutdown.await;
}

#[gpui_kit::test]
async fn revealing_an_empty_local_panel_starts_once_and_focuses_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("terminal test directory");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx)
            .expect("terminal translations");
    });
    let panel = cx.update(|cx| cx.new(Panel::new));
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        Root::new(panel.clone(), window, cx)
    });
    let shutdown = window
        .update(cx, |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.reveal(window, cx);
                assert!(
                    panel.entries.is_empty(),
                    "unknown cwd never falls back to Home"
                );
                panel.set_cwd(Some(directory.path().to_path_buf()), cx);
                panel.set_preferences(
                    gds::ui::terminal::Preferences {
                        shell: Some(directory.path().join("missing-test-shell")),
                        ..Default::default()
                    },
                    cx,
                );
                panel.reveal(window, cx);
                let terminal = panel.entries[0].terminal.clone();
                assert!(panel.contains_focus(window, cx));
                panel.reveal(window, cx);
                assert_eq!(panel.entries.len(), 1);
                assert_eq!(panel.entries[0].terminal, terminal);
                panel.set_cwd(None, cx);
                panel.reveal(window, cx);
                assert_eq!(
                    panel.entries.len(),
                    1,
                    "unavailable target preserves existing instance"
                );
                panel.shutdown(cx)
            })
        })
        .expect("reveal local panel");
    shutdown.await;
}

#[gpui_kit::test]
fn independent_empty_collections_do_not_spawn_when_targets_or_preferences_change(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        let first = cx.new(Panel::new);
        let second = cx.new(Panel::new);
        first.update(cx, |panel, cx| {
            panel.set_cwd(
                Some(std::env::current_dir().expect("working directory")),
                cx,
            );
            let mut preferences = gds::ui::terminal::Preferences::default();
            preferences.login_shell = true;
            panel.set_preferences(preferences, cx);
            assert!(panel.entries.is_empty());
            assert!(panel.active.is_none());
        });
        assert!(second.read(cx).cwd.is_none());
        assert!(!second.read(cx).preferences.login_shell);
        first.update(cx, |panel, cx| panel.set_cwd(None, cx));
        assert!(first.read(cx).entries.is_empty());
        assert!(first.read(cx).cwd.is_none());
    });
}

#[gpui_kit::test]
fn empty_collection_shutdown_is_repeatable_and_does_not_seal_other_owners(cx: &mut TestAppContext) {
    let (mut first, mut repeated) = cx.update(|cx| {
        let panel = cx.new(Panel::new);
        let other = cx.new(Panel::new);
        let futures = panel.update(cx, |panel, cx| {
            let first = panel.shutdown(cx);
            assert!(panel.sealed);
            assert!(panel.entries.is_empty());
            (first, panel.shutdown(cx))
        });
        assert!(!other.read(cx).sealed);
        futures
    });
    let mut task = TaskContext::from_waker(Waker::noop());
    assert_eq!(first.as_mut().poll(&mut task), Poll::Ready(()));
    assert_eq!(repeated.as_mut().poll(&mut task), Poll::Ready(()));
}

#[gpui_kit::test]
async fn close_starting_terminal_skips_dialog_and_preserves_shutdown_barrier(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{component::Root, px, size};
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("terminal directory");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("test messages");
    });
    let mut panel = None;
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        let view = cx.new(Panel::new);
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.expect("terminal panel");
    let shutdown = window
        .update(cx, |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.preferences.shell = Some(directory.path().join("missing-test-shell"));
                panel.open(directory.path().to_path_buf(), window, cx);
                let id = panel.active.expect("new terminal");
                let terminal = panel.entries[0].terminal.clone();
                panel.request_close(id, window, cx);
                panel.request_close(id, window, cx);
                assert!(panel.entries[0].closing, "startup is cancelled immediately");
                // Collection closure must include the already closing entry.
                (panel.shutdown(cx), terminal)
            })
        })
        .expect("close starting terminal");
    let (shutdown, terminal) = shutdown;
    shutdown.await;
    cx.run_until_parked();
    cx.update(|cx| assert!(!terminal.read(cx).is_running(cx)));
}

#[cfg(unix)]
#[gpui_kit::test]
async fn folder_switch_opens_new_instance_and_preserves_existing_shells(cx: &mut TestAppContext) {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };
    cx.executor().allow_parking();
    let parent = tempfile::tempdir().expect("panel cwd");
    let target = parent.path().join("选中 folder ' $HOME");
    std::fs::create_dir(&target).expect("target directory");
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|path| path.join("sh"))
        .chain([PathBuf::from("/bin/sh")])
        .find(|path| path.is_absolute() && path.is_file())
        .expect("POSIX shell");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("test messages");
    });
    let panel = cx.update(|cx| cx.new(Panel::new));
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        Root::new(panel.clone(), window, cx)
    });
    cx.update_window(window.into(), |_, window, cx| panel.update(cx, |panel, cx| {
        panel.change_directory(target.clone(), window, cx);
        assert!(panel.entries.is_empty(), "no selection must not create a terminal");
        panel.preferences.shell = Some(shell);
        panel.preferences.shell_args = vec!["-c".into(),
            r#"stty -echo; printf '\033]2;%s\007' "$PWD"; while IFS= read -r line; do eval "$line"; printf '\033]2;%s\007' "$PWD"; done"#.into()];
        panel.open(parent.path().to_owned(), window, cx);
        let first = panel.active.expect("first terminal");
        panel.rename_entry(first, " Build output ", cx);
        panel.open(parent.path().to_owned(), window, cx);
    })).expect("open two terminals");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        cx.run_until_parked();
        if cx.update(|cx| {
            panel
                .read(cx)
                .entries
                .iter()
                .all(|entry| entry.terminal.read(cx).title(cx).is_some())
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "shell startup");
        std::thread::sleep(Duration::from_millis(10));
    }
    let originals = cx.update(|cx| {
        panel
            .read(cx)
            .entries
            .iter()
            .map(|entry| entry.terminal.clone())
            .collect::<Vec<_>>()
    });
    cx.update_window(window.into(), |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.change_directory(target.clone(), window, cx);
        })
    })
    .expect("switch selected terminal cwd");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        cx.run_until_parked();
        if cx.update(|cx| {
            panel.read(cx).entries.get(2).is_some_and(|entry| {
                entry
                    .terminal
                    .read(cx)
                    .title(cx)
                    .is_some_and(|title| title.as_ref() == target.to_str().expect("UTF-8 path"))
            })
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "selected shell cd");
        std::thread::sleep(Duration::from_millis(10));
    }
    let shutdown = cx.update(|cx| {
        panel.update(cx, |panel, cx| {
            assert_eq!(panel.entries.len(), 3);
            assert_eq!(panel.entries[0].terminal, originals[0]);
            assert_eq!(panel.entries[1].terminal, originals[1]);
            assert_eq!(
                Panel::title(&panel.entries[0], cx).as_ref(),
                "Build output",
                "custom instance name survives shell title updates and selection changes"
            );
            assert_eq!(
                Panel::title(&panel.entries[1], cx).as_ref(),
                parent.path().to_str().expect("UTF-8 path")
            );
            panel.rename_entry(panel.entries[0].id, "", cx);
            assert_eq!(
                Panel::title(&panel.entries[0], cx).as_ref(),
                parent.path().to_str().expect("UTF-8 path"),
                "clearing a custom name restores the shell title"
            );
            assert_eq!(panel.active, Some(panel.entries[2].id));
            for original in &originals {
                assert_eq!(
                    original.read(cx).title(cx).as_deref(),
                    parent.path().to_str()
                );
            }
            panel.shutdown(cx)
        })
    });
    shutdown.await;
}

#[gpui_kit::test]
async fn row_close_is_hover_only_and_targets_its_own_terminal(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, Modifiers, ModifiersChangedEvent, test::TestWindowExt};

    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("terminal row directory");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx)
            .expect("terminal row translations");
        crate::features::file_operations::commands::register(cx);
    });
    let panel = cx.update(|cx| cx.new(Panel::new));
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        Root::new(panel.clone(), window, cx)
    });
    let shutdown = cx
        .update_window(window.into(), |_, window, cx| {
            let (first, second) = panel.update(cx, |panel, cx| {
                panel.preferences.shell = Some(directory.path().join("missing-test-shell"));
                panel.set_cwd(Some(directory.path().to_path_buf()), cx);
                panel.open(directory.path().to_path_buf(), window, cx);
                panel.open(directory.path().to_path_buf(), window, cx);
                (panel.entries[0].id, panel.entries[1].id)
            });
            window.render_frame(cx);
            window.hover("files-terminal-tabs", cx);
            assert!(!window.find(("files-terminal-close", first)).visible());
            assert!(!window.find(("files-terminal-close", second)).visible());
            window.hover(("files-terminal-tab", first), cx);
            assert!(window.find(("files-terminal-close", first)).visible());
            window.click(("files-terminal-close", first), cx);
            assert_eq!(panel.read(cx).active, Some(second));
            assert!(panel.read(cx).entries[0].closing);
            window.click(("files-terminal-tab", second), cx);
            window.hover("files-terminal-tabs", cx);
            window.dispatch_event(
                ModifiersChangedEvent {
                    modifiers: Modifiers::control(),
                    capslock: window.capslock(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            assert!(window.find(("files-terminal-close", second)).visible());
            panel.update(cx, |panel, cx| {
                panel.focus(window, cx);
                cx.notify();
            });
            window.render_frame(cx);
            assert!(
                !window.find(("files-terminal-close", second)).visible(),
                "PTY focus must not advertise the navigation Ctrl+W target"
            );
            panel.update(cx, |panel, cx| panel.shutdown(cx))
        })
        .expect("terminal row close targeting");
    shutdown.await;
}
