use super::*;
use bas::acp;

fn listing() -> connection::AgentHistory {
    connection::AgentHistory {
        sessions: acp::ListSessionsResponse::new(vec![
            acp::SessionInfo::new("external/id", "/tmp").title("Native CLI history"),
        ]),
        initialization: acp::InitializeResponse::new(
            agent_client_protocol::schema::ProtocolVersion::V1,
        )
        .agent_capabilities(acp::AgentCapabilities::new().load_session(true)),
        imported: HashMap::new(),
    }
}

#[gpui_kit::test]
fn embedded_history_opens_the_session_in_its_original_workspace(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx);
    let mut meta = session();
    meta.status = SessionStatus::Stopped;
    attach(&fixture, meta.clone(), cx);
    let (history_commands, _history_receiver) = mpsc::unbounded_channel();
    let (_history_updates, history_events) = mpsc::channel(64);
    let callback = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            let request = fixture.view.update(cx, |view, cx| {
                view.composer
                    .input
                    .update(cx, |input, cx| input.set_value("history draft", window, cx));
                view.history_request(meta.agent.to_string(), cx)
            });
            let callback = request.open_session.clone();
            let return_focus = window.focused(cx);
            let browser = crate::widgets::history::window::connected_content(
                request,
                Connection::for_test(history_commands),
                history_events,
                window,
                cx,
            );
            let root = window.root::<Root>().flatten().expect("workspace root");
            let host = crate::shared::lib::diagnostics::content(root.read(cx), cx)
                .downcast::<crate::pages::workspace::windows::WorkspaceHost>()
                .expect("workspace host");
            host.update(cx, |host, cx| {
                host.push_history(browser, return_focus, window, cx)
            });
            callback
        })
        .expect("embedded history");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("agent-history-window").visible());
        assert!(!fixture.view.read(cx).window.presented);
    })
    .expect("history covers the workspace");
    while fixture.commands.try_recv().is_ok() {}
    meta.status = SessionStatus::Idle;
    meta.revision = Cursor::from_raw(10);
    cx.update(|cx| assert!(callback(meta.clone().into(), cx)));
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-history-window").is_none());
        let view = fixture.view.read(cx);
        assert!(view.window.presented);
        assert_eq!(view.selected.id, Some(meta.id));
        assert_eq!(view.selected.stream, StreamState::Loading);
        assert_eq!(view.composer.input.read(cx).value(), "history draft");
    })
    .expect("original session and draft restored");
    cx.update(|cx| assert_eq!(cx.windows().len(), 1));
    let mut watched = false;
    while let Ok(command) = fixture.commands.try_recv() {
        if matches!(command, Command::Watch { session, .. } if session == meta.id) {
            watched = true;
        }
    }
    assert!(watched);
}

#[gpui_kit::test]
fn reopening_restarted_current_session_reattaches_without_clearing_the_draft(
    cx: &mut TestAppContext,
) {
    let mut fixture = fixture(cx);
    let mut meta = session();
    meta.status = SessionStatus::Stopped;
    attach(&fixture, meta.clone(), cx);
    let callback = cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture.view.update(cx, |view, cx| {
                view.selected.stream = StreamState::Closed;
                view.composer.input.update(cx, |input, cx| {
                    input.set_value("keep unsent text", window, cx)
                });
                view.history_request(meta.agent.to_string(), cx)
                    .open_session
            })
        })
        .expect("closed stream");
    while fixture.commands.try_recv().is_ok() {}
    meta.status = SessionStatus::Idle;
    meta.revision = Cursor::from_raw(10);
    cx.update(|cx| assert!(callback(meta.clone().into(), cx)));
    cx.run_until_parked();
    assert!(
        matches!(fixture.commands.try_recv(), Ok(Command::Watch { session, .. }) if session == meta.id)
    );
    cx.update(|cx| {
        let view = fixture.view.read(cx);
        assert_eq!(view.selected.id, Some(meta.id));
        assert_eq!(view.selected.stream, StreamState::Loading);
        assert_eq!(view.composer.input.read(cx).value(), "keep unsent text");
    });
    assert!(fixture.commands.try_recv().is_err());
}

#[gpui_kit::test]
fn workspace_restart_retries_failure_and_preserves_the_current_draft(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx);
    let mut meta = session();
    meta.agent = "unsupported-acp".into();
    meta.status = SessionStatus::Stopped;
    attach(&fixture, meta.clone(), cx);
    while fixture.commands.try_recv().is_ok() {}
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.request_restart_agent_session(meta.id, cx);
            assert!(fixture.commands.try_recv().is_err(), "unsupported Agent cannot restart");
            meta.initialization = Some(listing().initialization);
            *view.sessions.entries.iter_mut().find(|entry| entry.id == meta.id).expect("session") = meta.clone();
            view.composer.input.update(cx, |input, cx| input.set_value("unsent draft", window, cx));
            view.request_restart_agent_session(meta.id, cx);
            assert!(matches!(fixture.commands.try_recv(), Ok(Command::RestartSession(id)) if id == meta.id));
            view.update(Update::RestartedSession { session: meta.id, result: Err("restart rejected".into()) }, window, cx);
            assert!(!view.sessions.pending.restarting.contains(&meta.id));
            assert_eq!(view.composer.input.read(cx).value(), "unsent draft");
            view.request_restart_agent_session(meta.id, cx);
            assert!(matches!(fixture.commands.try_recv(), Ok(Command::RestartSession(id)) if id == meta.id));
            meta.status = SessionStatus::Idle;
            meta.revision = Cursor::from_raw(1);
            view.update(Update::RestartedSession { session: meta.id, result: Ok(meta.clone().into()) }, window, cx);
            assert!(matches!(fixture.commands.try_recv(), Ok(Command::Watch { session, .. }) if session == meta.id));
            assert_eq!(view.composer.input.read(cx).value(), "unsent draft");
            assert_eq!(view.selected.stream, StreamState::Loading);
        });
    }).expect("retry and reattach");
    assert!(fixture.commands.try_recv().is_err());
}
