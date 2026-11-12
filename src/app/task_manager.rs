//! Application-owned task service. Close windows, drain work, then quit.
use std::ops::Deref;

use crate::service::task_manager::Scope;
use gpui_kit::{App, Global};

struct Application {
    scope: Scope,
    phase: Phase,
    keep_alive: bool,
    retained: Vec<gpui_kit::AnyView>,
}
impl Global for Application {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Starting,
    Running,
    Closing,
}

/// Task service access and application shutdown coordination.
#[derive(Clone, Copy)]
pub struct Tasks<C> {
    pub(super) cx: C,
}

impl<C: Deref<Target = App>> Tasks<C> {
    /// Obtain the service installed by application startup before deriving tasks.
    pub fn scope(&self) -> Scope {
        self.cx.global::<Application>().scope.clone()
    }

    /// Read-only diagnostics, available only when enabled at startup.
    pub fn inspector(&self) -> Option<crate::service::task_manager::Inspector> {
        self.cx.try_global::<Application>()?.scope.inspector()
    }

    pub fn is_shutting_down(&self) -> bool {
        self.cx
            .try_global::<Application>()
            .is_some_and(|app| app.phase == Phase::Closing)
    }
}

impl Tasks<&mut App> {
    /// Install once before starting tasks; diagnostics are fixed for this app lifetime.
    pub fn init(&mut self, scope: Scope) {
        let cx = &mut *self.cx;
        if cx.has_global::<Application>() {
            return;
        }
        cx.set_global(Application {
            scope,
            phase: Phase::Starting,
            keep_alive: false,
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

    /// Start accepting business requests once required services are ready.
    pub fn ready(&mut self, keep_alive: bool) {
        let app = self.cx.global_mut::<Application>();
        if app.phase == Phase::Starting {
            app.phase = Phase::Running;
            app.keep_alive = keep_alive;
        }
    }

    pub fn quit_when_idle(&mut self) {
        let app = self.cx.global::<Application>();
        if app.phase != Phase::Running || app.keep_alive || !self.cx.windows().is_empty() {
            return;
        }
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let cx = &mut *self.cx;
        let root = cx.global::<Application>().scope.clone();
        let app = cx.global_mut::<Application>();
        if app.phase == Phase::Closing {
            return;
        }
        app.phase = Phase::Closing;
        root.cancel();
        // The coordinator itself must not belong to the scope it is waiting for.
        cx.spawn(async move |cx| {
            root.shutdown().await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    }

    /// Retain entity-owned quit observers until GPUI has collected their futures.
    pub fn retain_until_quit(&mut self, views: Vec<gpui_kit::AnyView>) {
        let cx = &mut *self.cx;
        cx.global_mut::<Application>().retained.extend(views);
    }

    pub fn request_quit(&mut self) {
        let cx = &mut *self.cx;
        let mut views = Vec::new();
        for handle in cx.windows() {
            let _ = handle.update(cx, |view, window, _| {
                views.push(view);
                window.remove_window();
            });
        }
        self.retain_until_quit(views);
        self.shutdown();
    }

    /// Translate SIGINT/SIGTERM into the same application-owned quit request as UI.
    #[cfg(unix)]
    pub fn signals(&mut self, quit: fn(&mut App)) {
        let cx = &mut *self.cx;
        let root = cx.global::<Application>().scope.clone();
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
        let task = root.spawn_named("Watch shutdown signals", work);
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
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

#[cfg(feature = "gui-tests")]
mod testing;
