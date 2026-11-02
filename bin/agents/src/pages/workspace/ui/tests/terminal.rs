use super::*;

#[gpui_kit::test]
fn remote_terminal_toggle_expands_without_spawning_or_falling_back(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.window.compact = true;
            view.toggle_terminal(window, cx);
            assert!(!view.window.compact);
            assert!(view.terminals.visible);
            assert!(
                view.terminal_panel().is_none(),
                "fixture uses loopback TCP, not local IPC"
            );
            assert!(!view.has_running_terminals(cx));
        });
    })
    .expect("remote terminal is unavailable");
}

#[cfg(unix)]
#[gpui_kit::test]
async fn render_sync_defers_startup_and_hiding_cancels_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = fixture(cx);
    let panel = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.backend.server =
                    bas::service::Target::Unix("/tmp/terminal-test.sock".into()).into();
                view.navigation.selected_directory = Some(std::env::temp_dir());
                view.terminals.visible = true;
                view.sync_terminal_panel(window, cx);
                let panel = view.terminal_panel().expect("local owner discovered");
                assert!(
                    !panel.read(cx).has_sessions(),
                    "render sync never starts a process"
                );
                view.terminals.visible = false;
                panel
            })
        })
        .expect("sync local panel");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(
            !panel.read(cx).has_sessions(),
            "hidden before deferred startup"
        );
        assert!(panel.read(cx).needs_initial_reveal());
    });
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.set_presented(true, window, cx);
            view.terminals.visible = true;
            view.sync_terminal_panel(window, cx);
            assert!(
                !panel.read(cx).has_sessions(),
                "retry still defers process startup"
            );
        });
    })
    .expect("return to canceled owner");
    cx.run_until_parked();
    let shutdown = cx.update(|cx| {
        panel.update(cx, |panel, cx| {
            assert!(
                panel.has_sessions(),
                "returning retries canceled initial reveal"
            );
            assert!(!panel.needs_initial_reveal());
            panel.shutdown(cx)
        })
    });
    shutdown.await;
}

#[cfg(unix)]
#[gpui_kit::test]
async fn panels_start_on_reveal_and_survive_hiding_and_directory_switches(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("terminal owner directories");
    let first_cwd = directory.path().join("first");
    let second_cwd = directory.path().join("second");
    std::fs::create_dir(&first_cwd).expect("first cwd");
    std::fs::create_dir(&second_cwd).expect("second cwd");
    let fixture = fixture(cx);
    let shutdowns = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.backend.server =
                    bas::service::Target::Unix("/tmp/terminal-test.sock".into()).into();
                view.navigation.selected_directory = Some(first_cwd.clone());
                view.toggle_terminal(window, cx);
                let first = view.terminal_panel().expect("explicit local cwd panel");
                assert!(first.read(cx).has_sessions());
                assert!(view.terminal_focused(window, cx));
                view.toggle_terminal(window, cx);
                view.prepare_to_close(window, cx);
                view.set_presented(false, window, cx);
                assert_eq!(
                    view.terminal_panel().expect("retained").entity_id(),
                    first.entity_id()
                );
                view.set_presented(true, window, cx);
                view.navigation.selected_directory = Some(second_cwd.clone());
                view.toggle_terminal(window, cx);
                let second = view.terminal_panel().expect("second cwd panel");
                assert_ne!(first.entity_id(), second.entity_id());
                view.navigation.selected_directory = Some(first_cwd.clone());
                assert_eq!(
                    view.terminal_panel().expect("original").entity_id(),
                    first.entity_id()
                );
                let no_cwd = session();
                view.selected.id = Some(no_cwd.id);
                view.sessions.entries.push(no_cwd);
                assert!(
                    view.terminal_panel().is_none(),
                    "session without cwd must not reuse selected directory"
                );
                view.selected.id = None;
                view.navigation.selected_directory = None;
                assert!(view.terminal_panel().is_none());
                view.shutdown_terminals(cx)
            })
        })
        .expect("panels are workspace-owned");
    for shutdown in shutdowns {
        shutdown.await;
    }
}

#[cfg(unix)]
#[gpui_kit::test]
async fn terminal_navigation_is_nested_in_its_workspace_and_dock_does_not_split_sidebar(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace terminal directories");
    let first_cwd = directory.path().join("first");
    let second_cwd = directory.path().join("second");
    std::fs::create_dir(&first_cwd).expect("first workspace");
    std::fs::create_dir(&second_cwd).expect("second workspace");
    let fixture = fixture(cx);
    let first = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            let first = fixture.view.update(cx, |view, cx| {
                view.backend.server =
                    bas::service::Target::Unix("/tmp/terminal-test.sock".into()).into();
                view.navigation.directories = vec![first_cwd.clone(), second_cwd.clone()];
                view.navigation.selected_directory = Some(first_cwd.clone());
                view.toggle_terminal(window, cx);
                view.terminal_panel()
                    .expect("first workspace provides its panel")
            });
            window.render_frame(cx);
            let group = window.find("workspace-header-0").bounds();
            let terminals = window.find("workspace-terminals").bounds();
            let dock = window.find("local-terminal-dock").bounds();
            assert!(
                terminals.origin.y > group.origin.y,
                "terminal controls belong below workspace heading"
            );
            assert!(
                terminals.right() <= px(SIDEBAR_WIDTH),
                "controls remain inside workspace navigation"
            );
            assert!(
                dock.origin.x >= px(SIDEBAR_WIDTH),
                "terminal content never takes over application sidebar"
            );
            first
        })
        .expect("workspace-owned terminal layout");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        let before = window.find("workspace-terminals").bounds();
        gds::appearance::motion::set_animations_enabled(false, cx);
        fixture
            .view
            .update(cx, |view, cx| view.toggle_sidebar(window, cx));
        window.render_frame(cx);
        let after = window.find("workspace-terminals").bounds();
        assert_eq!(before.top(), after.top());
        assert_eq!(before.size.height, after.size.height);
        assert!(window.find("agents-terminal-tabs").visible());
        assert!(
            after.right() <= px(gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH),
            "terminal bounds: {before:?} -> {after:?}; sidebar header: {:?}; expanded: {}",
            window.find("sidebar-header").bounds(),
            fixture.view.read(cx).window.sidebar_visible
        );
        fixture.view.update(cx, |view, cx| {
            let presentation = gds::ui::sidebar::SidebarPresentation::default();
            view.navigation.selected_directory = Some(second_cwd.clone());
            view.sync_terminal_panel(window, cx);
            let second = view.terminal_panel().expect("second workspace panel");
            assert_ne!(first.entity_id(), second.entity_id());
            assert!(
                view.render_workspace_terminals(Some(&first_cwd), presentation, cx)
                    .is_none()
            );
            assert!(
                view.render_workspace_terminals(Some(&second_cwd), presentation, cx)
                    .is_some()
            );
            view.navigation.selected_directory = Some(first_cwd.clone());
            assert_eq!(
                view.terminal_panel()
                    .expect("retained first panel")
                    .entity_id(),
                first.entity_id()
            );
            assert!(
                view.render_workspace_terminals(Some(&first_cwd), presentation, cx)
                    .is_some()
            );
            assert!(
                view.render_workspace_terminals(Some(&second_cwd), presentation, cx)
                    .is_none()
            );
        });
    })
    .expect("directory switch retains terminal owner");
    let shutdowns = cx.update(|cx| {
        fixture
            .view
            .update(cx, |view, cx| view.shutdown_terminals(cx))
    });
    for shutdown in shutdowns {
        shutdown.await;
    }
}

#[cfg(unix)]
#[gpui_kit::test]
async fn dock_switch_and_close_work_without_sidebar_terminal_controls(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace terminal control directory");
    let cwd = directory.path().to_path_buf();
    let fixture = fixture(cx);
    let panel = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.backend.server =
                    bas::service::Target::Unix("/tmp/terminal-test.sock".into()).into();
                view.navigation.directories = vec![cwd.clone()];
                view.navigation.selected_directory = Some(cwd.clone());
                view.terminals.visible = true;
                view.sync_terminal_panel(window, cx);
                let panel = view.terminal_panel().expect("workspace panel");
                panel.update(cx, |panel, cx| {
                    panel.set_preferences(
                        gds::ui::terminal::Preferences {
                            shell: Some(cwd.join("missing-test-shell")),
                            ..Default::default()
                        },
                        cx,
                    );
                    panel.open(cwd.clone(), window, cx);
                });
                panel
            })
        })
        .expect("one workspace terminal");
    let first_id = cx.update(|cx| panel.read(cx).active_id().expect("initial terminal"));
    for (view_mode, sidebar_visible, collapsed, filtered) in [
        (workspace_navigation::View::History, true, false, false),
        (workspace_navigation::View::Bookmarks, true, false, false),
        (workspace_navigation::View::Workspaces, false, false, false),
        (workspace_navigation::View::Workspaces, true, true, false),
        (workspace_navigation::View::Workspaces, true, false, true),
    ] {
        cx.update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.navigation.view = view_mode;
                view.window.sidebar_visible = sidebar_visible;
                view.navigation.collapsed.clear();
                if collapsed {
                    view.navigation.collapsed.insert(Some(cwd.clone()));
                }
                view.navigation.search.update(cx, |input, cx| {
                    input.set_value(
                        if filtered {
                            "unmatched-workspace-filter"
                        } else {
                            ""
                        },
                        window,
                        cx,
                    )
                });
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(
                window.try_find("agents-terminal-tabs").is_some(),
                view_mode == workspace_navigation::View::Workspaces && !collapsed && !filtered,
            );
            assert!(window.try_find("choose-local-terminal").is_none());
            assert!(window.find("close-local-terminal").visible());
            window.click("new-local-terminal", cx);
            assert_eq!(panel.read(cx).choices(cx).len(), 2);
            assert_ne!(panel.read(cx).active_id(), Some(first_id));
            window.render_frame(cx);
            assert!(window.find("choose-local-terminal").visible());
            window.click("choose-local-terminal", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("select existing terminal from dock");
        cx.run_until_parked();
        cx.update_window(fixture.window.into(), |_, window, cx| {
            assert_eq!(panel.read(cx).active_id(), Some(first_id));
            assert_eq!(
                panel.read(cx).choices(cx).len(),
                2,
                "selection never creates a terminal"
            );
            window.click("choose-local-terminal", cx);
            window.press("down", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("select the other existing terminal");
        cx.run_until_parked();
        cx.update_window(fixture.window.into(), |_, window, cx| {
            assert_ne!(panel.read(cx).active_id(), Some(first_id));
            window.click("close-local-terminal", cx);
            assert_eq!(panel.read(cx).choices(cx).len(), 1);
            assert_eq!(
                panel.read(cx).active_id(),
                Some(first_id),
                "close retains original instance"
            );
        })
        .expect("close only selected terminal from dock");
    }
    let shutdowns = cx.update(|cx| {
        fixture
            .view
            .update(cx, |view, cx| view.shutdown_terminals(cx))
    });
    for shutdown in shutdowns {
        shutdown.await;
    }
}

#[cfg(unix)]
#[gpui_kit::test]
async fn terminal_tabs_keep_workspace_shortcuts_out_and_f6_restores_composer(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let fixture = fixture(cx);
    let shutdown = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            let shutdown = fixture.view.update(cx, |view, cx| {
                view.backend.server =
                    bas::service::Target::Unix("/tmp/terminal-test.sock".into()).into();
                view.navigation.selected_directory = Some("/tmp".into());
                view.toggle_terminal(window, cx);
                view.terminal_panel()
                    .expect("panel")
                    .update(cx, |panel, cx| {
                        panel.set_cwd(None, cx);
                        let shutdown = panel.shutdown(cx);
                        panel.focus(window, cx);
                        shutdown
                    })
            });
            window.render_frame(cx);
            let epoch = fixture.view.read(cx).selected.epoch;
            for key in [
                "ctrl-n",
                "ctrl-w",
                "ctrl-d",
                "ctrl-l",
                "ctrl-tab",
                "ctrl-q",
                "ctrl-shift-l",
                "pageup",
                "escape",
            ] {
                window.press(key, cx);
            }
            assert_eq!(fixture.view.read(cx).selected.epoch, epoch);
            assert!(!fixture.view.read(cx).window.compact);
            assert!(fixture.view.read(cx).terminal_focused(window, cx));
            window.press("f6", cx);
            shutdown
        })
        .expect("terminal keyboard boundary");
    shutdown.await;
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert!(
            fixture
                .view
                .read(cx)
                .composer
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
        window.press("ctrl-`", cx);
        assert!(!fixture.view.read(cx).terminals.visible);
    })
    .expect("focus exit and window toggle");
}

#[cfg(unix)]
#[gpui_kit::test]
async fn close_without_confirmation_reaps_hidden_starting_terminal(cx: &mut TestAppContext) {
    // The PTY supervisor is an OS thread, outside the deterministic UI scheduler.
    cx.executor().allow_parking();
    let fixture = fixture(cx);
    let panel = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.backend.server =
                    bas::service::Target::Unix("/tmp/terminal-test.sock".into()).into();
                view.navigation.selected_directory = Some(std::env::temp_dir());
                view.toggle_terminal(window, cx);
                let panel = view.terminal_panel().expect("local panel");
                view.toggle_terminal(window, cx);
                assert!(view.has_running_terminals(cx), "starting counts as running");
                panel
            })
        })
        .expect("request close with hidden terminal");
    let (closed, finished) = futures::channel::oneshot::channel();
    let mut closed = Some(closed);
    let expected = fixture.window.window_id();
    let _closed = cx.update(|cx| {
        cx.on_window_closed(move |_, id| {
            if id == expected
                && let Some(closed) = closed.take()
            {
                let _ = closed.send(());
            }
        })
    });
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture
            .view
            .update(cx, |view, cx| view.close_window(window, cx));
        assert!(
            cx.windows()
                .iter()
                .all(|handle| handle.window_id() == expected),
            "close must not open a confirmation window"
        );
    })
    .expect("request immediate close");
    finished
        .await
        .expect("Host closes only after the real PTY shutdown completes");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!panel.read(cx).has_sessions());
        assert!(
            !cx.windows()
                .iter()
                .any(|window| window.window_id() == fixture.window.window_id())
        );
    });
}

#[cfg(unix)]
#[gpui_kit::test]
async fn sidebar_terminal_close_follows_hover_and_keyboard_target(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, Modifiers, ModifiersChangedEvent};

    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("sidebar terminal directory");
    let fixture = fixture(cx);
    let panel = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.backend.server =
                    bas::service::Target::Unix("/tmp/terminal-test.sock".into()).into();
                view.navigation.directories = vec![directory.path().to_path_buf()];
                view.navigation.selected_directory = Some(directory.path().to_path_buf());
                view.terminals.visible = true;
                view.sync_terminal_panel(window, cx);
                let panel = view.terminal_panel().expect("sidebar terminal panel");
                panel.update(cx, |panel, cx| {
                    panel.set_preferences(
                        gds::ui::terminal::Preferences {
                            shell: Some(directory.path().join("missing-test-shell")),
                            ..Default::default()
                        },
                        cx,
                    );
                    panel.open(directory.path().to_path_buf(), window, cx);
                    panel.open(directory.path().to_path_buf(), window, cx);
                });
                panel
            })
        })
        .expect("two sidebar terminals");
    let (first, second) = cx.update(|cx| {
        let choices = panel.read(cx).choices(cx);
        (choices[0].0, choices[1].0)
    });
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.hover("sidebar-header", cx);
        assert!(!window.find(("agents-terminal-close", first)).visible());
        assert!(!window.find(("agents-terminal-close", second)).visible());
        window.hover(("agents-terminal-tab", first), cx);
        assert!(window.find(("agents-terminal-close", first)).visible());
        window.click(("agents-terminal-close", first), cx);
        assert_eq!(panel.read(cx).choices(cx).len(), 1);
        assert_eq!(
            panel.read(cx).active_id(),
            Some(second),
            "closing an inactive row never selects its retired instance"
        );
        window.click(("agents-terminal-tab", second), cx);
        window.hover("sidebar-header", cx);
        window.dispatch_event(
            ModifiersChangedEvent {
                modifiers: Modifiers::control(),
                capslock: window.capslock(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert!(window.find(("agents-terminal-close", second)).visible());
        window.dispatch_event(
            ModifiersChangedEvent {
                modifiers: Modifiers::default(),
                capslock: window.capslock(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert!(!window.find(("agents-terminal-close", second)).visible());
        window.press("ctrl-w", cx);
        assert!(panel.read(cx).choices(cx).is_empty());
    })
    .expect("sidebar close interaction");
    let shutdowns = cx.update(|cx| {
        fixture
            .view
            .update(cx, |view, cx| view.shutdown_terminals(cx))
    });
    for shutdown in shutdowns {
        shutdown.await;
    }
}
