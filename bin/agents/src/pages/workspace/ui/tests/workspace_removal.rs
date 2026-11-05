use super::*;
use connection::WorkspaceMutation;

#[gpui_kit::test]
fn workspace_delete_requires_confirmation_and_cascades_only_when_confirmed(
    cx: &mut TestAppContext,
) {
    let mut fixture = fixture(cx);
    let first = session();
    let mut archived = session();
    archived.archived = true;
    attach(&fixture, archived.clone(), cx);
    attach(&fixture, first.clone(), cx);
    while fixture.commands.try_recv().is_ok() {}
    cx.update_window(fixture.window.into(), |_, _, cx| {
        fixture.view.update(cx, |view, cx| {
            view.request_remove_workspace(first.cwd.clone(), cx)
        });
    })
    .expect("request delete");
    assert!(fixture.commands.try_recv().is_err());
    let confirmation = cx
        .windows()
        .into_iter()
        .find(|window| window.window_id() != fixture.window.window_id())
        .expect("confirmation");
    cx.update_window(confirmation, |_, window, cx| {
        window.render_frame(cx);
        window.click("message-close", cx);
    })
    .expect("cancel");
    assert!(fixture.commands.try_recv().is_err());
    cx.update_window(fixture.window.into(), |_, _, cx| {
        fixture.view.update(cx, |view, cx| {
            view.request_remove_workspace(first.cwd.clone(), cx)
        });
    })
    .expect("request again");
    let confirmation = cx
        .windows()
        .into_iter()
        .find(|window| window.window_id() != fixture.window.window_id())
        .expect("confirmation");
    cx.update_window(confirmation, |_, window, cx| {
        window.render_frame(cx);
        window.click("message-confirm", cx);
    })
    .expect("confirm cascade");
    assert!(
        matches!(fixture.commands.try_recv(), Ok(Command::Workspace(WorkspaceMutation::Delete { id, force: true })) if id == first.workspace)
    );
    assert!(
        fixture.commands.try_recv().is_err(),
        "one daemon operation, no client cascade"
    );
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.update(
                Update::Workspace {
                    mutation: WorkspaceMutation::Delete {
                        id: first.workspace,
                        force: true,
                    },
                    result: Err("offline".into()),
                },
                window,
                cx,
            );
            assert_eq!(view.sessions.entries.len(), 2);
            assert_eq!(view.backend.workspaces.len(), 1);
            assert!(view.navigation.workspace_pending.is_empty());
            view.update(
                Update::Workspace {
                    mutation: WorkspaceMutation::Delete {
                        id: first.workspace,
                        force: true,
                    },
                    result: Ok(None),
                },
                window,
                cx,
            );
            assert!(view.sessions.entries.is_empty());
            assert!(view.backend.workspaces.is_empty());
            assert!(view.sessions.removed.contains(&archived.id));
        });
    })
    .expect("failure preserves data; receipt removes all known members");
    assert!(
        !std::iter::from_fn(|| fixture.commands.try_recv().ok())
            .any(|command| matches!(command, Command::Create(_))),
        "deleting the last workspace must not recreate it through session preparation"
    );
}

#[gpui_kit::test]
fn workspace_archive_is_one_server_operation_and_stale_catalog_cannot_restore_it(
    cx: &mut TestAppContext,
) {
    let mut fixture = fixture(cx);
    let meta = session();
    attach(&fixture, meta.clone(), cx);
    while fixture.commands.try_recv().is_ok() {}
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            let mutation = WorkspaceMutation::Archive {
                id: meta.workspace,
                archived: true,
            };
            view.mutate_workspace(mutation.clone(), cx);
            let mut workspace = view.backend.workspaces[0].clone();
            workspace.archived = true;
            workspace.revision += 1;
            view.update(
                Update::Workspace {
                    mutation,
                    result: Ok(Some(workspace)),
                },
                window,
                cx,
            );
            assert!(view.selected.id.is_none());
            view.merge_workspaces(vec![project(meta.cwd.as_deref())]);
            assert!(view.backend.workspaces[0].archived);
            assert!(view.grouped_threads(&view.sessions.entries, "").is_empty());
            view.navigation.view = workspace_navigation::View::Archived;
            assert_eq!(
                view.grouped_threads(&[], "").len(),
                1,
                "archived empty workspace remains accessible"
            );
        });
    })
    .expect("archive workspace");
    let commands: Vec<_> = std::iter::from_fn(|| fixture.commands.try_recv().ok()).collect();
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(
                command,
                Command::Workspace(WorkspaceMutation::Archive { archived: true, .. })
            ))
            .count(),
        1
    );
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::Archive { .. } | Command::Delete(_)))
    );
}

#[gpui_kit::test]
fn empty_workspace_is_displayed_without_creating_a_session(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx);
    while fixture.commands.try_recv().is_ok() {}
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            let workspace = project(Some(std::path::Path::new("/empty")));
            view.update(
                Update::Workspace {
                    mutation: WorkspaceMutation::Create {
                        cwd: workspace.cwd.clone(),
                    },
                    result: Ok(Some(workspace.clone())),
                },
                window,
                cx,
            );
            assert!(view.sessions.entries.is_empty());
            assert!(view.backend.workspaces.contains(&workspace));
            view.update(
                Update::Catalog(Ok(connection::Catalog {
                    workspaces: view.backend.workspaces.clone(),
                    agents: view.backend.agents.clone(),
                    sessions: Vec::new(),
                })),
                window,
                cx,
            );
            assert!(view.sessions.entries.is_empty());
            assert_eq!(view.grouped_threads(&[], "").len(), 2);
        });
    })
    .expect("empty project and periodic catalog refresh");
    assert!(
        !std::iter::from_fn(|| fixture.commands.try_recv().ok())
            .any(|command| matches!(command, Command::Create(_)))
    );
}
