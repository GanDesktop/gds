use super::*;

pub(super) fn permission(session: SessionId) -> bas::model::PendingPermission {
    bas::model::PendingPermission {
        request_id: "live-request".into(),
        request: bas::acp::RequestPermissionRequest::new(
            session.to_string(),
            bas::acp::ToolCallUpdate::new(
                "write-file",
                bas::acp::ToolCallUpdateFields::new().title("写入配置"),
            ),
            vec![bas::acp::PermissionOption::new(
                "allow-once",
                "允许一次",
                bas::acp::PermissionOptionKind::AllowOnce,
            )],
        ),
    }
}

#[gpui_kit::test]
fn live_approval_can_be_answered_without_resizing_the_conversation(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx);
    let meta = session();
    attach(&fixture, meta.clone(), cx);
    while fixture.commands.try_recv().is_ok() {}
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.update(
                history_window(
                    view.selected.epoch,
                    meta.clone().into(),
                    vec![Event::new(
                        meta.id,
                        Cursor::from_raw(1),
                        EventKind::Acp(bas::acp::SessionUpdate::UserMessageChunk(
                            bas::acp::ContentChunk::new("Run tool".into()),
                        )),
                    )],
                ),
                window,
                cx,
            );
            cx.notify();
        });
        window.render_frame(cx);
    })
    .expect("conversation before approval");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.update(Update::Permissions {
                epoch: view.selected.epoch,
                pending: vec![permission(meta.id)],
            }, window, cx);
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.find("approval-panel").visible());
        window.click("permission-live-request-allow-once", cx);
        assert!(matches!(fixture.commands.try_recv(), Ok(Command::RespondPermission { session, .. }) if session == meta.id));
    }).expect("answer live approval");
}

#[gpui_kit::test]
fn late_approval_snapshots_cannot_appear_in_another_thread(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let first = session();
    let second = session();
    attach(&fixture, first.clone(), cx);
    let old_epoch = cx.update(|cx| fixture.view.read(cx).selected.epoch);
    attach(&fixture, second, cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.update(
                Update::Permissions {
                    epoch: old_epoch,
                    pending: vec![permission(first.id)],
                },
                window,
                cx,
            );
            assert!(view.selected.approval.panel.is_empty());
        });
        window.render_frame(cx);
        assert!(window.try_find("approval-panel").is_none());
    })
    .expect("approval epoch isolation");
}
