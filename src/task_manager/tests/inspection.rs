use super::*;

#[test]
fn diagnostics_are_opt_in_but_shutdown_still_waits_for_cleanup() {
    struct UnusedLabel;
    impl std::fmt::Display for UnusedLabel {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            panic!("diagnostic labels must not be formatted when disabled");
        }
    }
    let root = Scope::default();
    let owner = root.named_child(UnusedLabel);
    let completion = owner
        .register_named(UnusedLabel, Execution::Thread)
        .unwrap();
    assert!(!owner.inspection_enabled());
    assert!(root.admission.lock().unwrap().is_none());
    assert!(completion.inspection.is_none());
    assert!(root.snapshot().scopes.is_empty());
    assert!(root.snapshot().tasks.is_empty());
    let mut shutdown = Box::pin(root.shutdown());
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(shutdown.as_mut().poll(&mut cx), Poll::Pending);
    assert!(owner.token().is_cancelled());
    assert!(owner.register().is_err());
    drop(completion);
    assert_eq!(shutdown.as_mut().poll(&mut cx), Poll::Ready(()));
}

#[test]
fn snapshots_preserve_hierarchy_and_cleanup_without_owning_resources() {
    let root = Scope::new(true);
    let service = root.named_child("Connection");
    let worker = service.named_child("History");
    let completion = worker.register_named("Flush", Execution::Thread).unwrap();
    let before = root.snapshot();
    assert_eq!(before.scopes.len(), 3);
    assert_eq!(before.scopes[1].parent, Some(before.scopes[0].id));
    assert_eq!(before.scopes[2].parent, Some(before.scopes[1].id));
    assert_eq!(before.tasks.len(), 1);
    assert_eq!(before.tasks[0].scope, before.scopes[2].id);
    assert_eq!(before.tasks[0].name, "Flush");
    assert_eq!(before.tasks[0].execution, Execution::Thread);
    assert!(!before.tasks[0].cancelling);

    drop(worker);
    drop(service);
    let closing = root.snapshot();
    assert_eq!(
        closing.scopes.len(),
        3,
        "pending cleanup retains its ancestry"
    );
    assert_eq!(closing.scopes[2].name, "History");
    assert!(closing.scopes[1..].iter().all(|scope| scope.cancelling));
    assert!(closing.tasks[0].cancelling);
    assert_eq!(closing.tasks[0].id, before.tasks[0].id);
    assert!(closing.tasks[0].age >= before.tasks[0].age);

    drop(completion);
    let after = root.snapshot();
    assert!(after.tasks.is_empty());
    assert_eq!(
        after.scopes.len(),
        1,
        "saved snapshots do not retain scopes"
    );
    assert!(!root.token().is_cancelled());
}

#[test]
fn registration_site_and_unpolled_future_drop_are_visible() {
    let root = Scope::new(true);
    let line = line!() + 1;
    let work = root.track(std::future::pending::<()>()).unwrap();
    let snapshot = root.snapshot();
    assert_eq!(snapshot.tasks[0].source.file(), file!());
    assert_eq!(snapshot.tasks[0].source.line(), line);
    assert_eq!(snapshot.tasks[0].execution, Execution::Supervisor);
    drop(work);
    assert!(root.snapshot().tasks.is_empty());
    root.cancel();
    assert!(root.register().is_err());
    assert!(root.snapshot().tasks.is_empty());
}

#[tokio::test]
async fn executor_adapters_report_kind_until_completion() {
    let root = Scope::new(true);
    let runtime = tokio::runtime::Handle::current();
    let cancel = root.token();
    let asynchronous = root
        .spawn(&runtime, async move { cancel.cancelled().await })
        .unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let blocking = root
        .spawn_blocking(&runtime, move || receive.recv().unwrap())
        .unwrap();
    let (stop, stopped) = std::sync::mpsc::channel();
    let thread = root
        .spawn_thread(std::thread::Builder::new(), move || stopped.recv().unwrap())
        .unwrap();
    let snapshot = root.snapshot();
    assert_eq!(
        snapshot
            .tasks
            .iter()
            .map(|task| task.execution)
            .collect::<Vec<_>>(),
        [Execution::Tokio, Execution::Blocking, Execution::Thread]
    );
    assert!(
        snapshot
            .tasks
            .iter()
            .all(|task| task.source.file() == file!())
    );
    root.cancel();
    assert!(root.snapshot().tasks.iter().all(|task| task.cancelling));
    send.send(()).unwrap();
    stop.send(()).unwrap();
    asynchronous.await.unwrap();
    blocking.await.unwrap();
    thread.join().unwrap();
    root.shutdown().await;
    assert!(root.snapshot().tasks.is_empty());
}
