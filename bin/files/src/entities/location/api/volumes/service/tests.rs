use super::{Service, Snapshot};
use gpui_kit::{AppContext, TestAppContext};

#[gpui_kit::test]
fn stopped_watcher_reports_its_join_error_instead_of_leaving_discovery_pending(
    cx: &mut TestAppContext,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("fixture runtime");
    let handle = runtime.handle().clone();
    drop(runtime);

    let service = cx.update(|cx| cx.new(|cx| Service::new(handle, cx)));
    cx.run_until_parked();
    cx.update(|cx| {
        let service = service.read(cx);
        assert!(!service.discovering);
        assert!(service.error.is_some());
        assert_eq!(service.snapshot, Snapshot::default());
    });
}

#[gpui_kit::test]
fn releasing_the_service_cancels_its_owned_listener(cx: &mut TestAppContext) {
    // A current-thread runtime keeps the listener unpolled, so this lifecycle
    // check does not connect to the machine's system bus.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("fixture runtime");
    let service = cx.update(|cx| cx.new(|cx| Service::new(runtime.handle().clone(), cx)));
    cx.run_until_parked();
    assert_eq!(runtime.metrics().num_alive_tasks(), 1);

    cx.update(|_| drop(service));
    cx.run_until_parked();
    runtime.block_on(async { tokio::task::yield_now().await });
    assert_eq!(runtime.metrics().num_alive_tasks(), 0);
}
