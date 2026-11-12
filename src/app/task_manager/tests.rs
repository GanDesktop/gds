use gds::app::AppExt as _;
use gpui_kit::{AppContext, Empty, TestAppContext, px, size};
use std::future::Future as _;

#[gpui_kit::test]
fn windows_disappear_before_services_finish(cx: &mut TestAppContext) {
    let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
    cx.update(|cx| cx.gds().tasks().init_test(false));
    let root = cx.update(|cx| cx.gds().tasks().scope());
    assert!(!root.inspection_enabled());
    cx.update(|cx| assert!(cx.gds().tasks().inspector().is_none()));
    let owner = root.child();
    let (release, receiver) = tokio::sync::oneshot::channel();
    let cancel = owner.token();
    let work = owner
        .track(async move {
            cancel.cancelled().await;
            receiver.await.unwrap();
        })
        .unwrap();
    cx.update(|cx| cx.background_executor().spawn(work).detach());
    cx.update(|cx| cx.gds().tasks().request_quit());
    assert!(cx.update_window(window.into(), |_, _, _| ()).is_err());
    assert!(root.token().is_cancelled());
    let mut drain = Box::pin(root.shutdown());
    use std::task::{Context, Poll, Waker};
    assert_eq!(
        drain.as_mut().poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(
        drain.as_mut().poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    );
}

#[gpui_kit::test]
fn another_window_keeps_the_application_scope_open(cx: &mut TestAppContext) {
    let first = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
    let _second = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
    cx.update(|cx| cx.gds().tasks().init_test(false));
    let root = cx.update(|cx| cx.gds().tasks().scope());
    cx.update_window(first.into(), |_, window, _| window.remove_window())
        .unwrap();
    cx.update(|cx| cx.gds().tasks().quit_when_idle());
    assert!(!root.token().is_cancelled());
    assert!(root.register().is_ok());
}

#[gpui_kit::test]
fn debug_collects_from_startup_without_opening_the_panel(cx: &mut TestAppContext) {
    cx.update(|cx| cx.gds().tasks().init_test(true));
    let root = cx.update(|cx| cx.gds().tasks().scope());
    assert!(root.inspection_enabled());
    let _completion = root.register().unwrap();
    cx.update(|cx| {
        assert_eq!(
            cx.gds().tasks().inspector().unwrap().snapshot().tasks.len(),
            1
        );
        assert!(!cx.gds_ro().ui().logs().is_enabled());
    });
    cx.update(|cx| assert!(cx.windows().is_empty()));
}

#[gpui_kit::test]
fn startup_and_headless_service_do_not_quit_without_windows(cx: &mut TestAppContext) {
    let root = gds::service::task_manager::Scope::default();
    cx.update(|cx| {
        cx.gds().tasks().init(root.clone());
        cx.gds().tasks().quit_when_idle();
        assert!(!root.token().is_cancelled());
        cx.gds().tasks().ready(true);
        cx.gds().tasks().quit_when_idle();
        assert!(!root.token().is_cancelled());
        cx.gds().tasks().request_quit();
        assert!(root.token().is_cancelled());
    });
}

#[gpui_kit::test]
fn closing_during_startup_cannot_reenter_running(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.gds()
            .tasks()
            .init(gds::service::task_manager::Scope::default());
        cx.gds().tasks().request_quit();
        cx.gds().tasks().ready(true);
        assert!(cx.gds().tasks().is_shutting_down());
        assert!(cx.gds().tasks().scope().register().is_err());
    });
}
