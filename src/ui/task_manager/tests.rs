use super::*;
use gpui_kit::{AppContext, Empty, TestAppContext, px, size};

#[gpui_kit::test]
fn windows_disappear_before_services_finish(cx: &mut TestAppContext) {
    let window = cx.open_window(size(px(100.), px(100.)), |_, _| Empty);
    let root = cx.update(scope);
    assert!(!root.inspection_enabled());
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
    cx.update(request_quit);
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
    let root = cx.update(scope);
    cx.update_window(first.into(), |_, window, _| window.remove_window())
        .unwrap();
    cx.update(quit_when_idle);
    assert!(!root.token().is_cancelled());
    assert!(root.register().is_ok());
}

#[gpui_kit::test]
fn debug_collects_from_startup_without_opening_the_panel(cx: &mut TestAppContext) {
    cx.update(|cx| crate::ui::logs::install(crate::service::logs::Store::new(), cx));
    let root = cx.update(scope);
    assert!(root.inspection_enabled());
    let _completion = root.register().unwrap();
    assert_eq!(root.snapshot().tasks.len(), 1);
    cx.update(|cx| assert!(cx.windows().is_empty()));
}
