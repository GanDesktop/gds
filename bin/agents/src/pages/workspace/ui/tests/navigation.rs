use super::*;
use gpui_kit::{AssetSource, InputEvent};

#[gpui_kit::test]
fn sidebar_search_filters_then_restores_the_list_without_losing_drafts(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let mut first = session();
    first.title = Some("Find this conversation".into());
    let mut second = session();
    second.title = Some("Another conversation".into());
    attach(&fixture, first.clone(), cx);
    attach(&fixture, second.clone(), cx);
    cx.update_window(fixture.window.into(), |_, window, _| {
        window.activate_window()
    })
    .expect("activate the search window for focus events");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.composer.input.update(cx, |input, cx| {
                input.set_value("Keep this composition", window, cx);
            });
        });
        assert!(window.try_find("thread-search").is_none());
        window.click("sidebar-search", cx);
        window.input("Find this", cx);
        assert!(window.try_find(format!("session-{}", second.id)).is_none());
    })
    .expect("filter the sidebar");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.click(format!("session-{}", first.id), cx);
    })
    .expect("select a search result");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = fixture.view.read(cx);
        assert_eq!(view.selected.id, Some(first.id));
        assert_eq!(
            view.sessions.drafts[&second.id].text,
            "Keep this composition"
        );
        assert!(!view.navigation.search_expanded);
        assert!(view.navigation.search.read(cx).value().is_empty());
        assert!(window.find(format!("session-{}", second.id)).visible());

        window.press("ctrl-f", cx);
        window.input("no matching conversation", cx);
    })
    .expect("search with no results");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.press("ctrl-l", cx);
    })
    .expect("leave search for the composer");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert!(!fixture.view.read(cx).navigation.search_expanded);
        assert!(window.try_find("thread-search").is_none());

        window.press("ctrl-f", cx);
        window.input("Find this", cx);
        window.press("escape", cx);
        let view = fixture.view.read(cx);
        assert!(!view.navigation.search_expanded);
        assert!(!view.window.compact);
        assert!(view.composer.input.focus_handle(cx).is_focused(window));
    })
    .expect("temporary sidebar search preserves session selection and drafts");
}

#[gpui_kit::test]
fn sidebar_view_menu_selects_each_view_and_keeps_the_current_one(cx: &mut TestAppContext) {
    assert!(
        crate::shared::ui::assets::Assets
            .load(IconName::ListFilter.path().as_ref())
            .expect("load the view menu icon")
            .is_some()
    );
    let fixture = fixture(cx);
    let active = session();
    let mut archived = session();
    archived.archived = true;
    attach(&fixture, archived.clone(), cx);
    attach(&fixture, active.clone(), cx);
    cx.update(|cx| {
        fixture.view.update(cx, |view, cx| {
            view.navigation.bookmarks.insert(active.id);
            cx.notify();
        });
    });
    for (collapsed, index, expected) in [false, true].into_iter().flat_map(|collapsed| {
        workspace_navigation::View::ALL
            .into_iter()
            .enumerate()
            .chain([
                (1, workspace_navigation::View::Bookmarks),
                (1, workspace_navigation::View::Bookmarks),
            ])
            .map(move |(index, view)| (collapsed, index, view))
    }) {
        cx.update_window(fixture.window.into(), |_, window, cx| {
            if fixture.view.read(cx).window.sidebar_visible == collapsed {
                window.press("ctrl-b", cx);
            }
            window.click("sidebar-view-menu", cx);
            for _ in 0..=index {
                window.press("down", cx);
            }
            window.press("enter", cx);
        })
        .expect("choose a sidebar view");
        cx.run_until_parked();
        cx.update_window(fixture.window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(fixture.view.read(cx).navigation.view, expected);
            let is_archive = expected == workspace_navigation::View::Archived;
            let prefix = "session";
            assert_eq!(
                window
                    .try_find(format!("{prefix}-{}", archived.id))
                    .is_some(),
                is_archive
            );
            assert_eq!(
                window.try_find(format!("{prefix}-{}", active.id)).is_some(),
                !is_archive
            );
        })
        .expect(
            "sidebar menu switches list organization without toggling an already selected view",
        );
    }
}

#[gpui_kit::test]
fn collapsed_sidebar_keeps_drafts_and_switches_sessions_without_expanding(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.press("ctrl-b", cx);
        window.click("draft-thread-0", cx);
        window.input("Keep the new draft", cx);
        window.press("ctrl-b", cx);
        assert_eq!(
            fixture.view.read(cx).composer.input.read(cx).value(),
            "Keep the new draft"
        );
        assert!(window.find("draft-thread-0").visible());
    })
    .expect("folding keeps the current draft accessible");
    let first = session();
    let second = session();
    attach(&fixture, first.clone(), cx);
    attach(&fixture, second.clone(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.composer.input.update(cx, |input, cx| {
                input.set_value("Keep the session draft", window, cx)
            });
        });
        window.press("ctrl-b", cx);
        window.click(format!("session-{}", first.id), cx);
        let view = fixture.view.read(cx);
        assert_eq!(view.selected.id, Some(first.id));
        assert!(!view.window.sidebar_visible);
        assert_eq!(
            view.sessions.drafts[&second.id].text,
            "Keep the session draft"
        );
        window.right_click(format!("session-{}", first.id), cx);
        assert!(
            fixture
                .view
                .read(cx)
                .window
                .menu
                .as_ref()
                .is_some_and(|menu| menu.window.is_none())
        );
        assert_eq!(cx.windows().len(), 1);
        window.render_frame(cx);
        assert!(window.find("popup-menu").visible());
        window.press("escape", cx);
    })
    .expect("collapsed session navigation preserves drafts and context actions");
    cx.run_until_parked();
    fixture.view.read_with(cx, |view, _| {
        assert!(view.window.menu.is_none());
        assert!(!view.window.sidebar_visible);
        assert!(!view.window.compact);
    });
}

#[gpui_kit::test]
fn sidebar_draft_title_double_click_renames_offline_and_preserves_discard(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.selected.creating = false;
            view.backend.connected = false;
            cx.notify();
        });
        window.click("draft-thread-title-text-0", cx);
        assert!(fixture.view.read(cx).renaming.edit.is_none());
        window.double_click("draft-thread-title-text-0", cx);
        let view = fixture.view.read(cx);
        assert!(view.renaming_draft());
        assert!(view.renaming.input.focus_handle(cx).is_focused(window));
        window.press("ctrl-a", cx);
        window.input("Local draft", cx);
        window.press("enter", cx);
    })
    .expect("edit local draft offline");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(fixture.view.read(cx).draft_title(), "Local draft");
        for target in ["draft-thread-agent-0"] {
            window.double_click(target, cx);
            assert!(
                fixture.view.read(cx).renaming.edit.is_none(),
                "target {target}"
            );
        }
        let row = window.find("draft-thread-title-0").bounds();
        let text = window.find("draft-thread-title-text-0").bounds();
        window.click_with_options(
            "draft-thread-title-0",
            gpui_kit::test::ClickOptions::new()
                .with_count(2)
                .with_offset(gpui_kit::point(
                    text.right() - row.left() + px(2.),
                    row.size.height / 2.,
                )),
            cx,
        );
        assert!(fixture.view.read(cx).renaming.edit.is_none());
        window.hover("draft-thread-0", cx);
        assert!(window.try_find("rename-draft-0").is_none());
        assert_eq!(
            window.find("draft-thread-actions-0").bounds().size.width,
            px(layout::ICON_BUTTON_SIZE)
        );
        window.click("discard-draft-0", cx);
        assert!(fixture.view.read(cx).navigation.drafts.metadata.is_empty());
        assert!(fixture.view.read(cx).renaming.edit.is_none());
    })
    .expect("local draft discard");
}

#[gpui_kit::test]
fn sidebar_draft_title_rejects_busy_and_stale_clicks(cx: &mut TestAppContext) {
    for blocked in ["creating", "pasting", "stale"] {
        let fixture = fixture(cx);
        cx.update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.selected.creating = false;
                cx.notify();
            });
            window.click("draft-thread-title-text-0", cx);
            let position = window.find("draft-thread-title-text-0").bounds().center();
            window.dispatch_event(
                gpui_kit::MouseDownEvent {
                    position,
                    button: gpui_kit::MouseButton::Left,
                    click_count: 2,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            fixture.view.update(cx, |view, _| match blocked {
                "creating" => view.selected.creating = true,
                "pasting" => view.composer.paste_pending = true,
                "stale" => view.composer.generation += 1,
                _ => unreachable!(),
            });
            window.dispatch_event(
                gpui_kit::MouseUpEvent {
                    position,
                    button: gpui_kit::MouseButton::Left,
                    click_count: 2,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            assert!(fixture.view.read(cx).renaming.edit.is_none(), "{blocked}");
        })
        .expect("blocked local draft rename");
    }
}

#[gpui_kit::test]
fn sidebar_title_clicks_select_then_rename_without_reselecting(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let mut first = session();
    first.status = SessionStatus::Working;
    let second = session();
    attach(&fixture, first.clone(), cx);
    attach(&fixture, second.clone(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.composer.input.update(cx, |input, cx| {
                input.set_value("keep this draft", window, cx);
            });
        });
        let title = format!("thread-title-text-{}", first.id);
        window.click(title.clone(), cx);
        let view = fixture.view.read(cx);
        assert_eq!(view.selected.id, Some(first.id));
        assert!(view.renaming.edit.is_none());
        assert_eq!(view.sessions.drafts[&second.id].text, "keep this draft");
        let epoch = view.selected.epoch;
        window.double_click(title, cx);
        let view = fixture.view.read(cx);
        assert!(view.renaming_thread(first.id));
        assert_eq!(view.selected.epoch, epoch);
        assert!(view.renaming.input.focus_handle(cx).is_focused(window));
        assert!(window.try_find(format!("rename-{}", first.id)).is_none());
        let input = view.renaming.input.entity_id();
        window.double_click("rename-title-input", cx);
        assert_eq!(fixture.view.read(cx).renaming.input.entity_id(), input);
        window.press("escape", cx);
        assert!(fixture.view.read(cx).renaming.edit.is_none());
        for target in [
            format!("thread-agent-{}", first.id),
            format!("thread-title-{}", first.id),
        ] {
            window.double_click(target.clone(), cx);
            assert!(
                fixture.view.read(cx).renaming.edit.is_none(),
                "target {target}"
            );
        }
        window.hover("sidebar-header", cx);
        let status = window.find(format!("thread-status-{}", first.id)).bounds();
        window.hover(format!("session-{}", first.id), cx);
        assert!(!window.find(format!("thread-status-{}", first.id)).visible());
        assert_eq!(
            window.find(format!("thread-actions-{}", first.id)).bounds(),
            status
        );
        assert_eq!(
            window
                .find(format!("thread-actions-{}", first.id))
                .bounds()
                .size
                .width,
            px(layout::ICON_BUTTON_SIZE)
        );
        window.click(format!("archive-{}", first.id), cx);
        assert!(
            fixture
                .view
                .read(cx)
                .sessions
                .pending
                .archiving
                .contains(&first.id)
        );
        assert!(fixture.view.read(cx).renaming.edit.is_none());
    })
    .expect("sidebar pointer interactions");
}

#[gpui_kit::test]
fn sidebar_title_double_click_rejects_disabled_targets(cx: &mut TestAppContext) {
    for disabled in ["offline", "archiving", "deleting"] {
        let fixture = fixture(cx);
        let target = session();
        attach(&fixture, target.clone(), cx);
        cx.update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                match disabled {
                    "offline" => view.backend.connected = false,
                    "archiving" => {
                        view.sessions.pending.archiving.insert(target.id);
                    }
                    "deleting" => {
                        view.sessions.pending.deleting.insert(target.id);
                    }
                    _ => unreachable!(),
                }
                cx.notify();
            });
            window.double_click(format!("thread-title-text-{}", target.id), cx);
            assert!(fixture.view.read(cx).renaming.edit.is_none(), "{disabled}");
        })
        .expect("disabled title");
    }
}

#[gpui_kit::test]
fn sidebar_title_editor_commits_and_f2_keeps_header_behavior(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx);
    let target = session();
    attach(&fixture, target.clone(), cx);
    while fixture.commands.try_recv().is_ok() {}
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.press("f2", cx);
        assert!(matches!(
            fixture.view.read(cx).renaming.edit.as_ref(),
            Some(rename::Edit {
                location: rename::Location::Header,
                ..
            })
        ));
        window.press("escape", cx);
        window.double_click(format!("thread-title-text-{}", target.id), cx);
        window.press("ctrl-a", cx);
        window.input("New sidebar title", cx);
        window.press("enter", cx);
    })
    .expect("title commit and F2");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(fixture.view.read(cx).renaming.edit.is_none());
        assert!(matches!(
            fixture.commands.try_recv(),
            Ok(Command::Rename { session, title })
                if session == target.id && title == "New sidebar title"
        ));
        assert!(fixture.commands.try_recv().is_err());
    })
    .expect("title commit receipt");
}

#[gpui_kit::test]
fn sidebar_title_context_menu_still_renames(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let target = session();
    attach(&fixture, target.clone(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        let size = window.bounds().size;
        window.right_click(format!("thread-title-text-{}", target.id), cx);
        assert!(
            fixture
                .view
                .read(cx)
                .window
                .menu
                .as_ref()
                .expect("context menu")
                .window
                .is_none()
        );
        assert_eq!(cx.windows().len(), 1);
        assert_eq!(window.bounds().size, size);
    })
    .expect("open context menu");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        for _ in 0..3 {
            window.press("down", cx);
        }
        window.press("enter", cx);
    })
    .expect("choose rename");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = fixture.view.read(cx);
        assert!(view.renaming_thread(target.id));
        assert!(view.renaming.input.focus_handle(cx).is_focused(window));
    })
    .expect("menu rename focuses sidebar editor");
}

#[gpui_kit::test]
fn sidebar_title_stale_double_click_cannot_rename_removed_session(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let target = session();
    attach(&fixture, target.clone(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.click(format!("thread-title-text-{}", target.id), cx);
        let position = window
            .find(format!("thread-title-text-{}", target.id))
            .bounds()
            .center();
        window.dispatch_event(
            gpui_kit::MouseDownEvent {
                position,
                button: gpui_kit::MouseButton::Left,
                click_count: 2,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        // Keep the painted listener alive while its catalog target becomes stale.
        fixture.view.update(cx, |view, _| {
            view.sessions.entries.retain(|entry| entry.id != target.id);
        });
        window.dispatch_event(
            gpui_kit::MouseUpEvent {
                position,
                button: gpui_kit::MouseButton::Left,
                click_count: 2,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        assert!(fixture.view.read(cx).renaming.edit.is_none());
    })
    .expect("stale title click");
}

#[gpui_kit::test]
fn switching_preserves_cached_history_but_detach_and_switch_clear_transient_controls(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let first = session();
    let second = session();
    attach(&fixture, first.clone(), cx);
    attach(&fixture, second.clone(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.select(first.id, window, cx);
            view.composer.input.update(cx, |input, cx| {
                input.set_value("first composition", window, cx);
            });
            view.sessions
                .outboxes
                .entry(first.id)
                .or_default()
                .enqueue(Draft {
                    text: "background queued prompt".into(),
                    attachments: Vec::new(),
                });
            view.sessions.pending.configuring.insert(first.id);
            view.sessions.pending.loading_attachments.insert(first.id);
            view.sessions.pending.cancelling.insert(first.id);
            view.sessions.pending.archiving.insert(first.id);

            for detach in [false, true] {
                let current = view.selected.id.expect("selected thread");
                view.update(
                    history_window(
                        view.selected.epoch,
                        if current == first.id {
                            first.clone()
                        } else {
                            second.clone()
                        }
                        .into(),
                        Vec::new(),
                    ),
                    window,
                    cx,
                );
                view.update(
                    Update::Permissions {
                        epoch: view.selected.epoch,
                        pending: vec![approval::permission(current)],
                    },
                    window,
                    cx,
                );
                assert!(!view.selected.approval.panel.is_empty());
                view.selected.approval.error = Some("current approval failure".into());
                view.selected.authentication.required = Some("embedded".into());
                view.selected.error = Some("current failure".into());
                view.selected
                    .timeline
                    .expanded
                    .insert("current item".into());
                let old_epoch = view.selected.epoch;
                if detach {
                    view.new_thread(None, window, cx);
                    assert_eq!(view.selected.id, None);
                } else {
                    view.select(second.id, window, cx);
                    assert_eq!(view.selected.id, Some(second.id));
                }
                assert_eq!(view.selected.epoch, old_epoch + 1);
                if detach {
                    assert!(view.selected.timeline.conversation.is_none());
                } else {
                    assert_eq!(
                        view.selected
                            .timeline
                            .conversation
                            .as_ref()
                            .expect("cached second thread")
                            .session
                            .id,
                        second.id,
                    );
                }
                assert!(view.selected.approval.panel.is_empty());
                assert!(view.selected.approval.error.is_none());
                assert!(view.selected.authentication.required.is_none());
                assert!(view.selected.error.is_none());
                assert!(view.selected.timeline.expanded.is_empty());
                assert_eq!(view.sessions.drafts[&first.id].text, "first composition");
                assert_eq!(view.sessions.outboxes[&first.id].len(), 1);
                assert!(view.sessions.pending.configuring.contains(&first.id));
                assert!(
                    view.sessions
                        .pending
                        .loading_attachments
                        .contains(&first.id)
                );
                assert!(view.sessions.pending.cancelling.contains(&first.id));
                assert!(view.sessions.pending.archiving.contains(&first.id));
            }
            view.select(first.id, window, cx);
            assert_eq!(view.composer.input.read(cx).value(), "first composition");
        });
    })
    .expect("selection lifecycle preserves work owned by other sessions");
}

#[gpui_kit::test]
fn sidebar_motion_keeps_shared_boundaries_rows_and_scroll(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let mut sessions: Vec<_> = (0..30).map(|_| session()).collect();
    for session in &mut sessions {
        session.cwd = Some(PathBuf::from("/tmp"));
        session.workspace = project(session.cwd.as_deref()).id;
    }
    sessions.sort_by_key(|session| (std::cmp::Reverse(session.updated_at), session.id));
    let target = sessions[8].id;
    let mut hidden = session();
    hidden.cwd = Some(PathBuf::from("/var"));
    hidden.workspace = project(hidden.cwd.as_deref()).id;
    let hidden_id = hidden.id;
    sessions.push(hidden);
    cx.update(|cx| {
        fixture.view.update(cx, |view, cx| {
            view.sessions.entries = sessions;
            view.navigation
                .collapsed
                .insert(Some(PathBuf::from("/var")));
            cx.notify();
        });
    });
    cx.run_until_parked();
    let initial = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture
                .view
                .read(cx)
                .navigation
                .scroll
                .set_offset(gpui_kit::point(px(0.), px(-96.)));
            window.refresh();
            window.render_frame(cx);
            let row = window.find(format!("session-{target}")).bounds();
            let icon = window.find(format!("thread-icon-{target}")).bounds();
            let offset = fixture.view.read(cx).navigation.scroll.offset();
            assert_eq!(offset.y, px(-96.));
            window.press("ctrl-b", cx);
            (row, icon, offset)
        })
        .expect("capture expanded sidebar geometry");
    for elapsed in [60, 160] {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(elapsed));
        cx.update_window(fixture.window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .expect("advance sidebar animation without sleeping");
        cx.run_until_parked();
        cx.update_window(fixture.window.into(), |_, window, cx| {
            window.render_frame(cx);
            let top = window.find("frame-toolbar-sidebar").bounds();
            let body = window.find("frame-workspace-sidebar").bounds();
            let bottom = window.find("frame-footer-sidebar").bounds();
            assert_eq!(top.right(), body.right());
            assert_eq!(bottom.right(), body.right());
            if elapsed == 60 {
                assert!(body.size.width > px(gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH));
                assert!(body.size.width < px(gds::appearance::tokens::SIDEBAR_WIDTH));
            } else {
                assert_eq!(
                    body.size.width,
                    px(gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH)
                );
            }
            let row = window.find(format!("session-{target}")).bounds();
            let icon = window.find(format!("thread-icon-{target}")).bounds();
            assert_eq!(row.top(), initial.0.top());
            assert_eq!(row.size.height, initial.0.size.height);
            assert_eq!(icon.center().y, initial.1.center().y);
            assert_eq!(fixture.view.read(cx).navigation.scroll.offset(), initial.2);
            assert!(
                fixture
                    .view
                    .read(cx)
                    .navigation
                    .collapsed
                    .contains(&Some(PathBuf::from("/var")))
            );
            assert!(window.try_find(format!("session-{hidden_id}")).is_none());
            if elapsed == 160 {
                assert_eq!(icon.center().x, body.center().x);
                window.hover(format!("session-{target}"), cx);
                assert!(
                    window
                        .try_find(format!("thread-actions-{target}"))
                        .is_none()
                );
            }
        })
        .expect("one continuous sidebar layout at every width");
    }
}

#[gpui_kit::test]
fn sidebar_collapsed_menu_keeps_commands_and_focus_available(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    attach(&fixture, session(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.activate_window();
        gds::appearance::motion::set_animations_enabled(false, cx);
        window.press("ctrl-b", cx);
        window.click("sidebar-view-menu", cx);
        for _ in 0..6 {
            window.press("down", cx);
        }
        window.press("enter", cx);
    })
    .expect("create a new session from the narrow sidebar menu");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert!(fixture.view.read(cx).selected.id.is_none());
        assert!(!fixture.view.read(cx).window.sidebar_visible);
        assert!(
            fixture
                .view
                .read(cx)
                .composer
                .input
                .focus_handle(cx)
                .is_focused(window)
        );
        window.click("sidebar-view-menu", cx);
        for _ in 0..5 {
            window.press("down", cx);
        }
        window.press("enter", cx);
    })
    .expect("search from the same menu");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        let view = fixture.view.read(cx);
        assert!(view.window.sidebar_visible);
        assert!(view.navigation.search_expanded);
        assert!(view.navigation.search.focus_handle(cx).is_focused(window));
        window.press("escape", cx);
        window.press("ctrl-b", cx);
        window.click("sidebar-view-menu", cx);
        for _ in 0..7 {
            window.press("down", cx);
        }
        window.press("enter", cx);
    })
    .expect("open add-workspace without moving the narrow menu control");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert!(!fixture.view.read(cx).window.sidebar_visible);
        assert!(window.find("popup-menu").visible());
        assert!(fixture.view.read(cx).window.menu.is_some());
        window.press("escape", cx);
    })
    .expect("workspace add menu remains accessible");
}

#[gpui_kit::test]
fn workspace_selector_scopes_sessions_and_keeps_pins_above_it(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let first = session();
    let mut second = session();
    second.cwd = Some("/tmp/second-workspace".into());
    second.workspace = project(second.cwd.as_deref()).id;
    let mut pinned = first.clone();
    pinned.id = SessionId::new();
    attach(&fixture, pinned.clone(), cx);
    attach(&fixture, first.clone(), cx);
    attach(&fixture, second.clone(), cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.navigation.pinned.insert(pinned.id);
            view.composer
                .input
                .update(cx, |input, cx| input.set_value("second draft", window, cx));
            cx.notify();
        });
        window.render_frame(cx);
        let picker = window.find("workspace-selector").bounds();
        let pins = window.find("pinned-sessions").bounds();
        assert!(pins.bottom() <= picker.top());
        assert!(
            window
                .find(format!("session-{}", pinned.id))
                .bounds()
                .bottom()
                <= picker.top()
        );
        assert!(window.find(format!("session-{}", second.id)).bounds().top() >= picker.bottom());
        assert!(window.try_find(format!("session-{}", first.id)).is_none());
        window.click("workspace-picker", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .expect("select first workspace from the sidebar menu");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(fixture.view.read(cx).selected.id, Some(first.id));
        assert!(window.find(format!("session-{}", first.id)).visible());
        assert!(window.try_find(format!("session-{}", second.id)).is_none());
        fixture.view.update(cx, |view, cx| {
            view.switch_workspace(second.cwd.clone(), window, cx)
        });
        assert_eq!(fixture.view.read(cx).selected.id, Some(second.id));
        assert_eq!(
            fixture.view.read(cx).composer.input.read(cx).value(),
            "second draft"
        );
        gds::appearance::motion::set_animations_enabled(false, cx);
        fixture
            .view
            .update(cx, |view, cx| view.toggle_sidebar(window, cx));
        window.render_frame(cx);
        let row = window.find(format!("session-{}", pinned.id)).bounds();
        let close = window
            .find(format!("thread-close-corner-{}", pinned.id))
            .bounds();
        assert_eq!(close.size, size(px(16.), px(16.)));
        assert!(close.left() >= row.left() && close.right() <= row.right());
        assert!(close.top() >= row.top() && close.bottom() <= row.bottom());
        window.click(format!("thread-close-corner-{}", pinned.id), cx);
        assert!(!fixture.view.read(cx).navigation.pinned.contains(&pinned.id));
        assert_eq!(
            fixture.view.read(cx).selected.id,
            Some(second.id),
            "unpin never selects the row"
        );
    })
    .expect("restore workspace draft and unpin from collapsed row");
}
