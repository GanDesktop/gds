//! Desktop session power-inhibit and background notification helpers via XDG portals.

use std::{future::Future, time::Duration};

use tokio::sync::oneshot;

use crate::service::ParentWindow;

/// Minimum operation duration before requesting a desktop suspend/idle inhibit lock.
pub const DEFAULT_INHIBIT_DELAY: Duration = Duration::from_secs(2);

/// Urgency level for a desktop notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationUrgency {
    Normal,
    High,
}

/// Payload dispatched through `org.freedesktop.portal.Notification`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopNotification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub urgency: NotificationUrgency,
}

/// Drop handle that cancels a pending inhibit delay or releases an active inhibit request.
#[derive(Debug)]
pub struct InhibitRelease(Option<oneshot::Sender<()>>);

impl Drop for InhibitRelease {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

/// RAII guard preventing desktop suspend/idle for long-running operations.
#[cfg(feature = "task-manager")]
#[derive(Debug)]
pub struct InhibitGuard {
    _release: InhibitRelease,
}

#[cfg(feature = "task-manager")]
impl InhibitGuard {
    /// Prevent session suspend and idle if this guard stays alive longer than `delay`.
    ///
    /// Dropping the guard before `delay` elapses cancels the timer without touching D-Bus.
    #[must_use]
    pub fn delayed(
        scope: &crate::service::task_manager::Scope,
        delay: Duration,
        reason: impl Into<String>,
    ) -> Self {
        Self::delayed_with_window(scope, delay, reason, None)
    }

    /// Prevent session suspend and idle after `delay`, optionally associating the portal
    /// request with `parent_window`.
    #[must_use]
    pub fn delayed_with_window(
        scope: &crate::service::task_manager::Scope,
        delay: Duration,
        reason: impl Into<String>,
        parent_window: Option<ParentWindow>,
    ) -> Self {
        let (tx, rx) = oneshot::channel();
        let reason = reason.into();
        drop(scope.spawn_named("Desktop suspend inhibit", async move {
            hold_inhibit_after_with_window(delay, reason, parent_window, rx).await;
        }));
        Self {
            _release: InhibitRelease(Some(tx)),
        }
    }
}

/// Wait `delay` before acquiring an `org.freedesktop.portal.Inhibit` lock, then hold it until
/// `release` resolves (or its sender drops).
pub async fn hold_inhibit_after(delay: Duration, reason: String, release: oneshot::Receiver<()>) {
    hold_inhibit_after_with_window(delay, reason, None, release).await;
}

/// Wait `delay` before acquiring an `org.freedesktop.portal.Inhibit` lock associated with
/// `parent_window`, then hold it until `release` resolves (or its sender drops).
pub async fn hold_inhibit_after_with_window(
    delay: Duration,
    reason: String,
    parent_window: Option<ParentWindow>,
    release: oneshot::Receiver<()>,
) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        hold_inhibit_after_with(
            delay,
            release,
            || acquire_portal_inhibit(reason, parent_window),
            |request| async move {
                if let Err(error) = request.close().await {
                    tracing::debug!(%error, "failed to close desktop inhibit request");
                }
            },
        )
        .await;
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = (delay, reason, parent_window);
        let _ = release.await;
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
async fn acquire_portal_inhibit(
    reason: String,
    parent_window: Option<ParentWindow>,
) -> Option<ashpd::desktop::Request<()>> {
    use ashpd::desktop::inhibit::{InhibitFlags, InhibitOptions, InhibitProxy};

    let proxy = match InhibitProxy::new().await {
        Ok(proxy) => proxy,
        Err(error) => {
            tracing::debug!(%error, "desktop inhibit portal unavailable");
            return None;
        }
    };
    let identifier = parent_window.as_ref().map(ParentWindow::to_ashpd);
    let options = InhibitOptions::default().set_reason(reason.as_str());
    match proxy
        .inhibit(
            identifier.as_ref(),
            InhibitFlags::Suspend | InhibitFlags::Idle,
            options,
        )
        .await
    {
        Ok(request) => Some(request),
        Err(error) => {
            tracing::debug!(%error, "desktop inhibit request rejected or unavailable");
            None
        }
    }
}

async fn hold_inhibit_after_with<Acq, AcqFut, Rel, RelFut, H>(
    delay: Duration,
    mut release: oneshot::Receiver<()>,
    acquire: Acq,
    close: Rel,
) where
    Acq: FnOnce() -> AcqFut,
    AcqFut: Future<Output = Option<H>>,
    Rel: FnOnce(H) -> RelFut,
    RelFut: Future<Output = ()>,
{
    tokio::select! {
        biased;
        _ = &mut release => return,
        () = tokio::time::sleep(delay) => {}
    }
    let handle = tokio::select! {
        biased;
        _ = &mut release => return,
        acquired = acquire() => acquired,
    };
    let Some(handle) = handle else {
        return;
    };
    let _ = release.await;
    close(handle).await;
}

/// Best-effort dispatch of a desktop notification through `org.freedesktop.portal.Notification`.
pub async fn send_notification(notification: DesktopNotification) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use ashpd::desktop::notification::{Notification, NotificationProxy, Priority};

        let proxy = match NotificationProxy::new().await {
            Ok(proxy) => proxy,
            Err(error) => {
                tracing::debug!(%error, "desktop notification portal unavailable");
                return;
            }
        };
        let priority = match notification.urgency {
            NotificationUrgency::Normal => Priority::Normal,
            NotificationUrgency::High => Priority::High,
        };
        let payload = Notification::new(&notification.title)
            .body(notification.body.as_str())
            .priority(priority);
        if let Err(error) = proxy.add_notification(&notification.id, payload).await {
            tracing::debug!(%error, id = %notification.id, "desktop notification failed");
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = notification;
    }
}

/// Spawn a best-effort desktop notification on `scope`.
#[cfg(feature = "task-manager")]
pub fn notify_background(
    scope: &crate::service::task_manager::Scope,
    notification: DesktopNotification,
) {
    drop(scope.spawn_named("Desktop notification", async move {
        send_notification(notification).await;
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn short_operation_cancels_before_delay_without_acquiring_inhibit() {
        let acquired = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = oneshot::channel();
        let release = InhibitRelease(Some(tx));

        let acq = Arc::clone(&acquired);
        let cls = Arc::clone(&closed);
        let handle = tokio::spawn(async move {
            hold_inhibit_after_with(
                Duration::from_secs(5),
                rx,
                || async move {
                    acq.fetch_add(1, Ordering::SeqCst);
                    Some(())
                },
                |()| async move {
                    cls.fetch_add(1, Ordering::SeqCst);
                },
            )
            .await;
        });

        drop(release);
        handle.await.expect("inhibit task finishes");
        assert_eq!(acquired.load(Ordering::SeqCst), 0);
        assert_eq!(closed.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn long_operation_acquires_after_delay_and_closes_on_release() {
        let acquired = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = oneshot::channel();
        let release = InhibitRelease(Some(tx));
        let (acquired_tx, acquired_rx) = oneshot::channel();

        let acq = Arc::clone(&acquired);
        let cls = Arc::clone(&closed);
        let handle = tokio::spawn(async move {
            hold_inhibit_after_with(
                Duration::from_millis(10),
                rx,
                || async move {
                    acq.fetch_add(1, Ordering::SeqCst);
                    let _ = acquired_tx.send(());
                    Some(42u32)
                },
                |token| async move {
                    assert_eq!(token, 42u32);
                    cls.fetch_add(1, Ordering::SeqCst);
                },
            )
            .await;
        });

        acquired_rx.await.expect("inhibit acquired after delay");
        assert_eq!(acquired.load(Ordering::SeqCst), 1);
        assert_eq!(closed.load(Ordering::SeqCst), 0);

        drop(release);
        handle.await.expect("inhibit task closes cleanly");
        assert_eq!(closed.load(Ordering::SeqCst), 1);
    }
}
