use super::*;

async fn pending_update(events: &mut mpsc::Receiver<Update>) -> Vec<PendingPermission> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Update::Permissions { epoch, pending } =
                events.recv().await.expect("approval watcher stays alive")
            {
                assert_eq!(epoch, 9);
                return pending;
            }
        }
    })
    .await
    .expect("approval snapshot arrives over gRPC")
}

async fn unavailable_update(events: &mut mpsc::Receiver<Update>) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.expect("watcher stays alive") {
                Update::PermissionUnavailable { epoch, message } => {
                    assert_eq!(epoch, 9);
                    return message.presentation();
                }
                Update::Permissions { pending, .. } => {
                    assert!(
                        !pending.is_empty(),
                        "approval failure must not publish a successful empty snapshot"
                    );
                }
                _ => {}
            }
        }
    })
    .await
    .expect("approval failure is visible")
}

#[tokio::test]
async fn disconnected_approval_stream_reports_failure_without_a_successful_empty_snapshot() {
    let fixture = fixture(Scenario::ApprovalDisconnected).await;
    let (updates, mut events) = mpsc::channel(EVENT_CAPACITY);
    let task = tokio::spawn(
        crate::entities::session::connection::permissions::watch_session(
            fixture.protocol.clone(),
            fixture.client.clone(),
            updates,
            fixture.session,
            9,
        ),
    );
    let message = unavailable_update(&mut events).await;
    assert!(!message.is_empty());
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn subscription_and_response_preserve_native_choices_over_grpc() {
    let fixture = fixture(Scenario::Approval).await;
    let (updates, mut events) = mpsc::channel(EVENT_CAPACITY);
    let task = tokio::spawn(
        crate::entities::session::connection::permissions::watch_session(
            fixture.protocol.clone(),
            fixture.client.clone(),
            updates,
            fixture.session,
            9,
        ),
    );
    let pending = pending_update(&mut events).await;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].request.tool_call.fields.raw_input,
        Some(serde_json::json!({"command": "pwd"}))
    );
    let request_id = pending[0].request_id.clone();
    let invalid = execute(
        fixture.protocol.clone(),
        fixture.client.clone(),
        Command::RespondPermission {
            session: fixture.session,
            request_id: request_id.clone(),
            outcome: acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                "invented",
            )),
        },
    )
    .await;
    assert!(matches!(
        invalid,
        Update::PermissionResponded { result: Err(_), .. }
    ));
    assert!(
        fixture
            .permission_answers
            .lock()
            .expect("answers")
            .is_empty()
    );
    let answered = execute(
        fixture.protocol.clone(),
        fixture.client.clone(),
        Command::RespondPermission {
            session: fixture.session,
            request_id: request_id.clone(),
            outcome: acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                "allow-once",
            )),
        },
    )
    .await;
    assert!(
        matches!(answered, Update::PermissionResponded { session, request_id: id, result: Ok(()) }
        if session == fixture.session && id == request_id)
    );
    assert!(pending_update(&mut events).await.is_empty());
    let duplicate = execute(
        fixture.protocol.clone(),
        fixture.client.clone(),
        Command::RespondPermission {
            session: fixture.session,
            request_id,
            outcome: acp::RequestPermissionOutcome::Cancelled,
        },
    )
    .await;
    assert!(matches!(
        duplicate,
        Update::PermissionResponded { result: Err(_), .. }
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        while fixture
            .permission_answers
            .lock()
            .expect("answers")
            .is_empty()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("ACP response reaches backend");
    assert_eq!(fixture.permission_answers.lock().expect("answers").len(), 1);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn detached_controller_rejects_answers_from_its_connection() {
    let fixture = fixture(Scenario::Approval).await;
    let (updates, mut events) = mpsc::channel(EVENT_CAPACITY);
    let task = tokio::spawn(super::super::permissions::watch_session(
        fixture.protocol.clone(),
        fixture.client.clone(),
        updates,
        fixture.session,
        9,
    ));
    let pending = pending_update(&mut events).await;
    let id = pending[0].request_id.clone();
    assert_ne!(id, "pending-1");
    task.abort();
    let _ = task.await;
    let result = execute(
        fixture.protocol.clone(),
        fixture.client.clone(),
        Command::RespondPermission {
            session: fixture.session,
            request_id: id,
            outcome: acp::RequestPermissionOutcome::Cancelled,
        },
    )
    .await;
    assert!(matches!(
        result,
        Update::PermissionResponded { result: Err(_), .. }
    ));
    assert!(
        fixture
            .permission_answers
            .lock()
            .expect("answers")
            .is_empty()
    );
}
