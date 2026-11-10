//! GPUI adapter: close native windows first, drain application work, then quit.
use crate::task_manager::Scope;
use gpui_kit::{App, Global};

struct Application {
    scope: Scope,
    closing: bool,
    retained: Vec<gpui_kit::AnyView>,
}
impl Global for Application {}

pub fn scope(cx: &mut App) -> Scope {
    init(cx);
    cx.global::<Application>().scope.clone()
}

pub fn is_shutting_down(cx: &App) -> bool {
    cx.try_global::<Application>()
        .is_some_and(|app| app.closing)
}

pub fn init(cx: &mut App) {
    if cx.has_global::<Application>() {
        return;
    }
    cx.set_global(Application {
        scope: Scope::new(crate::ui::logs::is_enabled(cx)),
        closing: false,
        retained: Vec::new(),
    });
    let root = cx.global::<Application>().scope.clone();
    // Native/OS quit bypassing the normal request still gets best-effort cleanup.
    cx.on_app_quit(move |_| {
        let root = root.clone();
        async move { root.shutdown().await }
    })
    .detach();
}

pub fn quit_when_idle(cx: &mut App) {
    if !cx.windows().is_empty() {
        return;
    }
    let root = scope(cx);
    let app = cx.global_mut::<Application>();
    if app.closing {
        return;
    }
    app.closing = true;
    root.cancel();
    // The coordinator itself must not belong to the scope it is waiting for.
    cx.spawn(async move |cx| {
        root.shutdown().await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Retain entity-owned quit observers until GPUI has collected their futures.
pub fn retain_until_quit(views: Vec<gpui_kit::AnyView>, cx: &mut App) {
    init(cx);
    cx.global_mut::<Application>().retained.extend(views);
}

pub fn request_quit(cx: &mut App) {
    let mut views = Vec::new();
    for handle in cx.windows() {
        let _ = handle.update(cx, |view, window, _| {
            views.push(view);
            window.remove_window();
        });
    }
    retain_until_quit(views, cx);
    quit_when_idle(cx);
}

/// Translate SIGINT/SIGTERM into the same application-owned quit request as UI.
#[cfg(unix)]
pub fn signals(runtime: &tokio::runtime::Handle, quit: fn(&mut App), cx: &mut App) {
    let root = scope(cx);
    let cancel = root.token();
    let (send, receive) = tokio::sync::oneshot::channel();
    let work = async move {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = cancel.cancelled() => {},
            _ = interrupt.recv() => { let _ = send.send(()); },
            _ = terminate.recv() => { let _ = send.send(()); },
        }
        std::io::Result::Ok(())
    };
    let task = root.spawn(runtime, work).expect("application is running");
    cx.spawn(async move |cx| {
        if receive.await.is_ok() {
            cx.update(quit);
        }
        if let Ok(Err(error)) = task.await {
            tracing::warn!(%error, "cannot install shutdown signals");
        }
    })
    .detach();
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests {
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
}
