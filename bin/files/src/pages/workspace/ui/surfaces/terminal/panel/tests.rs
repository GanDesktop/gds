use super::Panel;
use gds::{app::AppExt as _, ui::terminal};
use gpui_kit::{
    AppContext, Entity, Focusable, TestAppContext, WindowHandle, component::Root, px, size,
    test::TestWindowExt,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn open_test_panel(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Panel>) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds().tasks().init_test(false);
        cx.gds().ui().init_terminal();
        let catalog = crate::shared::i18n::prepare(gds::i18n::Language::English)
            .expect("prepare English translations");
        cx.gds().i18n().install_catalog(catalog);
        cx.gds()
            .appearance()
            .theme()
            .apply(&Default::default())
            .expect("apply default theme");
    });
    let mut panel = None;
    let window = cx.open_window(size(px(800.), px(500.)), |window, cx| {
        let view = cx.new(Panel::new);
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    (window, panel.expect("panel entity created in window"))
}

fn wait_for_panel(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    panel: &Entity<Panel>,
    predicate: impl Fn(&Panel, &gpui_kit::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        cx.executor().advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        let ready = cx
            .update_window(window.into(), |_, _, cx| predicate(panel.read(cx), cx))
            .expect("poll panel condition");
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for panel condition"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn interactive_bash() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|path| path.join("bash"))
        .chain([PathBuf::from("/bin/bash"), PathBuf::from("/usr/bin/bash")])
        .find(|path| path.is_absolute() && path.is_file())
        .expect("terminal auto-close test requires bash")
}

fn posix_shell() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|path| path.join("sh"))
        .chain([PathBuf::from("/bin/sh"), PathBuf::from("/usr/bin/sh")])
        .find(|path| path.is_absolute() && path.is_file())
        .expect("terminal test requires a POSIX sh")
}

fn open_interactive_tab(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    panel: &Entity<Panel>,
    cwd: PathBuf,
) -> (u64, Entity<terminal::Terminal>) {
    let (id, terminal, initial_focus) = cx
        .update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(cwd, window, cx);
                let entry = panel.entries.last().expect("opened terminal entry");
                let initial_focus = entry.terminal.read(cx).focus_handle(cx);
                (entry.id, entry.terminal.clone(), initial_focus)
            })
        })
        .expect("open interactive terminal tab");
    wait_for_panel(cx, window, panel, |_, cx| {
        let terminal = terminal.read(cx);
        terminal.status(cx) == terminal::Status::Running
            && terminal.focus_handle(cx) != initial_focus
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            terminal.read(cx).focus_handle(cx).is_focused(window),
            "opened terminal view must hold focus"
        );
        window.input(r"unset IGNOREEOF; PS1='\[\e]0;rc=$?\a\]'", cx);
        window.press("enter", cx);
    })
    .expect("initialize interactive prompt title");
    wait_for_panel(cx, window, panel, |_, cx| {
        terminal.read(cx).title(cx).as_deref() == Some("rc=0")
    });
    (id, terminal)
}

#[gpui_kit::test]
async fn closing_a_tab_removes_it_before_its_terminal_finishes(cx: &mut TestAppContext) {
    let (window, panel) = open_test_panel(cx);
    let directory = tempfile::tempdir().expect("create temporary working directory");
    let shutdown = cx
        .update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(directory.path().to_owned(), window, cx);
                let entry = &panel.entries[0];
                let id = entry.id;
                let terminal = entry.terminal.clone();
                panel.request_close(id, window, cx);
                assert!(
                    panel.entries.is_empty(),
                    "remove the tab without polling cleanup"
                );
                assert!(panel.active.is_none());
                terminal.update(cx, |terminal, cx| terminal.shutdown(cx))
            })
        })
        .expect("update window to close terminal tab");
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    shutdown.await;
}

#[gpui_kit::test]
async fn ctrl_d_exits_and_auto_closes_tabs_including_after_nonzero_command(
    cx: &mut TestAppContext,
) {
    let (window, panel) = open_test_panel(cx);
    let directory = tempfile::tempdir().expect("create temporary working directory");
    let bash = interactive_bash();
    cx.update_window(window.into(), |_, _, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_preferences(
                terminal::Preferences {
                    shell: Some(bash),
                    shell_args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
                    ..Default::default()
                },
                cx,
            );
        });
    })
    .expect("configure interactive bash preferences");

    let (id0, term0) = open_interactive_tab(cx, window, &panel, directory.path().to_owned());
    let (_id1, term1) = open_interactive_tab(cx, window, &panel, directory.path().to_owned());

    cx.update_window(window.into(), |_, window, cx| {
        window.input("false", cx);
        window.press("enter", cx);
    })
    .expect("run non-zero command in second tab");
    wait_for_panel(cx, window, &panel, |_, cx| {
        term1.read(cx).title(cx).as_deref() == Some("rc=1")
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.press("ctrl-d", cx);
    })
    .expect("press Ctrl+D in second tab");
    wait_for_panel(cx, window, &panel, |panel, _| panel.entries.len() == 1);

    cx.update_window(window.into(), |_, window, cx| {
        {
            let view = panel.read(cx);
            assert_eq!(view.entries.len(), 1);
            assert_eq!(view.entries[0].id, id0);
            assert_eq!(view.active, Some(id0));
            assert!(
                term0.read(cx).focus_handle(cx).is_focused(window),
                "remaining active terminal must receive focus when another tab auto-closes"
            );
            assert!(!view.tabs_focus.is_focused(window));
        }
        window.render_frame(cx);
        assert!(
            term0.read(cx).contains_focus(window, cx),
            "remaining active terminal view must contain focus after frame"
        );
        window.press("ctrl-d", cx);
    })
    .expect("verify focus transfer and press Ctrl+D in remaining tab");

    wait_for_panel(cx, window, &panel, |panel, _| panel.entries.is_empty());
    cx.update_window(window.into(), |_, window, cx| {
        let view = panel.read(cx);
        assert!(view.entries.is_empty());
        assert!(view.active.is_none());
        assert!(
            view.tabs_focus.is_focused(window),
            "focus must fall back to terminal tabs when the last tab auto-closes"
        );
        assert!(view.contains_focus(window, cx));
    })
    .expect("verify last terminal tab cleanup and focus fallback");

    let scope = cx.update(|cx| cx.gds_ro().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
}

#[gpui_kit::test]
async fn failed_terminal_launch_keeps_tab_open(cx: &mut TestAppContext) {
    let (window, panel) = open_test_panel(cx);
    let directory = tempfile::tempdir().expect("create temporary working directory");
    let missing_shell = directory.path().join("missing-shell");
    let (id, terminal) = cx
        .update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_preferences(
                    terminal::Preferences {
                        shell: Some(missing_shell),
                        ..Default::default()
                    },
                    cx,
                );
                panel.open(directory.path().to_owned(), window, cx);
                let entry = &panel.entries[0];
                (entry.id, entry.terminal.clone())
            })
        })
        .expect("open terminal with missing shell");

    wait_for_panel(cx, window, &panel, |_, cx| {
        matches!(terminal.read(cx).status(cx), terminal::Status::Failed(_))
    });
    cx.executor().advance_clock(Duration::from_millis(50));
    cx.run_until_parked();

    let shutdown = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let view = panel.read(cx);
            assert_eq!(view.entries.len(), 1);
            assert_eq!(view.active, Some(id));
            assert!(matches!(
                terminal.read(cx).status(cx),
                terminal::Status::Failed(_)
            ));
            panel.update(cx, |panel, cx| panel.shutdown(cx))
        })
        .expect("verify failed terminal tab remains open");
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    shutdown.await;
}

#[gpui_kit::test]
async fn signal_terminated_terminal_keeps_tab_open(cx: &mut TestAppContext) {
    let (window, panel) = open_test_panel(cx);
    let directory = tempfile::tempdir().expect("create temporary working directory");
    let (id, terminal) = cx
        .update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_preferences(
                    terminal::Preferences {
                        shell: Some(posix_shell()),
                        shell_args: vec!["-c".into(), "kill -s KILL $$".into()],
                        ..Default::default()
                    },
                    cx,
                );
                panel.open(directory.path().to_owned(), window, cx);
                let entry = &panel.entries[0];
                (entry.id, entry.terminal.clone())
            })
        })
        .expect("open terminal that terminates by signal");

    wait_for_panel(cx, window, &panel, |_, cx| {
        matches!(
            terminal.read(cx).status(cx),
            terminal::Status::Exited {
                signal: Some(_),
                ..
            }
        )
    });
    cx.executor().advance_clock(Duration::from_millis(50));
    cx.run_until_parked();

    let shutdown = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let view = panel.read(cx);
            assert_eq!(view.entries.len(), 1);
            assert_eq!(view.active, Some(id));
            assert!(matches!(
                terminal.read(cx).status(cx),
                terminal::Status::Exited {
                    signal: Some(_),
                    ..
                }
            ));
            panel.update(cx, |panel, cx| panel.shutdown(cx))
        })
        .expect("verify signal-terminated terminal tab remains open");
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    shutdown.await;
}

#[gpui_kit::test]
async fn background_and_unfocused_auto_close_preserve_active_tab_and_external_focus(
    cx: &mut TestAppContext,
) {
    let (window, panel) = open_test_panel(cx);
    let directory = tempfile::tempdir().expect("create temporary working directory");
    let shell = posix_shell();
    let (id0, term0) = cx
        .update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_preferences(
                    terminal::Preferences {
                        shell: Some(shell.clone()),
                        shell_args: vec!["-c".into(), "sleep 60".into()],
                        ..Default::default()
                    },
                    cx,
                );
                panel.open(directory.path().to_owned(), window, cx);
                let id0 = panel.entries[0].id;
                let term0 = panel.entries[0].terminal.clone();
                panel.set_preferences(
                    terminal::Preferences {
                        shell: Some(shell.clone()),
                        shell_args: vec!["-c".into(), "exit 5".into()],
                        ..Default::default()
                    },
                    cx,
                );
                panel.open(directory.path().to_owned(), window, cx);
                panel.select(id0, window, cx);
                panel.focus(window, cx);
                (id0, term0)
            })
        })
        .expect("open long-lived and background exiting tabs");

    wait_for_panel(cx, window, &panel, |panel, _| panel.entries.len() == 1);
    let outside = cx
        .update_window(window.into(), |_, window, cx| {
            let view = panel.read(cx);
            assert_eq!(view.active, Some(id0));
            assert!(term0.read(cx).focus_handle(cx).is_focused(window));

            let outside = cx.focus_handle();
            panel.update(cx, |panel, cx| {
                panel.set_preferences(
                    terminal::Preferences {
                        shell: Some(shell),
                        shell_args: vec!["-c".into(), "exit 0".into()],
                        ..Default::default()
                    },
                    cx,
                );
                panel.open(directory.path().to_owned(), window, cx);
            });
            outside.focus(window, cx);
            outside
        })
        .expect("open active exiting tab while external handle holds focus");

    wait_for_panel(cx, window, &panel, |panel, _| panel.entries.len() == 1);
    let shutdown = cx
        .update_window(window.into(), |_, window, cx| {
            let view = panel.read(cx);
            assert_eq!(view.active, Some(id0));
            assert!(
                outside.is_focused(window),
                "auto-closing a terminal must not steal focus from outside the panel"
            );
            panel.update(cx, |panel, cx| panel.shutdown(cx))
        })
        .expect("verify external focus preserved");
    let scope = cx.update(|cx| cx.gds_ro().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    shutdown.await;
    scope.shutdown().await;
}
