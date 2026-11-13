use super::{NewTerminal, Panel, Preferences};
use gds::app::AppExt as _;
use gpui_kit::{
    AppContext, Entity, TestAppContext, component::Root, px, size, test::TestWindowExt,
};

#[cfg(unix)]
#[gpui_kit::test]
async fn shell_title_changes_notify_the_host_caption(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        crate::shared::lib::appearance::init_default(cx);
    });
    let directory = tempfile::tempdir().expect("terminal title directory");
    let mut panel = None;
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        let entity = cx.new(Panel::new);
        panel = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let panel = panel.expect("terminal panel");
    let (title_seen, changed) = futures::channel::oneshot::channel();
    let mut title_seen = Some(title_seen);
    let _subscription = cx.update(|cx| {
        cx.observe(&panel, move |panel, cx| {
            if panel.read(cx).active_title(cx).as_ref() == "Live shell caption"
                && let Some(title_seen) = title_seen.take()
            {
                title_seen.send(()).expect("caption observer is waiting");
            }
        })
    });
    window
        .update(cx, |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_preferences(
                    Preferences {
                        shell: Some(which::which("sh").expect("POSIX test shell")),
                        shell_args: vec![
                            "-c".into(),
                            "printf '\\033]0;Live shell caption\\007'; read wait".into(),
                        ],
                        ..Default::default()
                    },
                    cx,
                );
                panel.open(directory.path().to_path_buf(), window, cx);
            });
        })
        .expect("start caption shell");
    changed.await.expect("OSC title reaches panel observers");
    let shutdown = cx.update(|cx| panel.update(cx, |panel, cx| panel.shutdown(cx)));
    shutdown.await;
}

#[gpui_kit::test]
async fn empty_dock_keeps_management_focus_without_workspace_sidebar(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        crate::shared::lib::appearance::init_default(cx);
        super::super::bind_keys(cx);
    });
    let directory = tempfile::tempdir().expect("empty workspace terminal directory");
    let mut panel = None;
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        let view = cx.new(Panel::new);
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.expect("workspace panel");
    let shutdown = cx
        .update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_preferences(
                    Preferences {
                        shell: Some(directory.path().join("missing-test-shell")),
                        ..Default::default()
                    },
                    cx,
                );
                panel.set_cwd(Some(directory.path().to_path_buf()), cx);
                panel.focus(window, cx);
            });
            window.render_frame(cx);
            assert!(panel.read(cx).contains_focus(window, cx));
            window.press("ctrl-n", cx);
            panel.update(cx, |panel, cx| {
                assert_eq!(
                    panel.entries.len(),
                    1,
                    "empty dock management works without navigation tabs"
                );
                panel.shutdown(cx)
            })
        })
        .expect("empty dock focus routing");
    shutdown.await;
}

#[gpui_kit::test]
async fn agents_selection_and_new_target_do_not_retarget_existing_terminals(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("terminal test directory");
    let first_cwd = directory.path().join("first");
    let second_cwd = directory.path().join("second");
    std::fs::create_dir(&first_cwd).expect("first directory");
    std::fs::create_dir(&second_cwd).expect("second directory");
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        crate::shared::lib::appearance::init_default(cx);
        super::super::bind_keys(cx);
    });
    let mut panel: Option<Entity<Panel>> = None;
    let window = cx.open_window(size(px(900.), px(500.)), |window, cx| {
        let view = cx.new(Panel::new);
        view.update(cx, |panel, cx| {
            // A missing explicit shell exercises managed startup without opening
            // a user's shell or depending on its interactive configuration.
            panel.set_preferences(
                Preferences {
                    shell: Some(directory.path().join("missing-test-shell")),
                    ..Default::default()
                },
                cx,
            );
        });
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.expect("collection");
    let shutdown = window
        .update(cx, |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.reveal(window, cx);
                assert!(
                    panel.entries.is_empty(),
                    "unknown cwd cannot launch a shell"
                );
                panel.set_cwd(Some(first_cwd.clone()), cx);
                panel.reveal(window, cx);
                panel.reveal(window, cx);
                assert_eq!(
                    panel.entries.len(),
                    1,
                    "reopening does not duplicate startup"
                );
                assert!(panel.contains_focus(window, cx));
                let first = panel.entries[0].terminal.clone();
                assert_eq!(first.read(cx).cwd(), first_cwd.as_path());
                assert_eq!(panel.active_title(cx).as_ref(), "first");
                panel.set_cwd(Some(second_cwd.clone()), cx);
                panel.new_terminal(&NewTerminal, window, cx);
                assert_eq!(panel.entries.len(), 2);
                assert_eq!(first.read(cx).cwd(), first_cwd.as_path());
                assert_eq!(
                    panel.entries[1].terminal.read(cx).cwd(),
                    second_cwd.as_path()
                );
                let second_id = panel.entries[1].id;
                assert_eq!(panel.active, Some(second_id));
                assert_eq!(panel.active_title(cx).as_ref(), "second");
                panel.cycle(true, window, cx);
                assert_eq!(panel.active_title(cx).as_ref(), "first");
                assert_eq!(
                    panel.active_terminal().expect("first selected").entity_id(),
                    first.entity_id()
                );
                panel.set_cwd(None, cx);
                panel.new_terminal(&NewTerminal, window, cx);
                assert_eq!(
                    panel.entries.len(),
                    2,
                    "unknown cwd disables only future creation"
                );
                let shutdown = panel.shutdown(cx);
                panel.open(first_cwd.clone(), window, cx);
                assert!(!panel.has_sessions(), "shutdown seals the collection");
                shutdown
            })
        })
        .expect("manage local terminal selection");
    shutdown.await;
}
