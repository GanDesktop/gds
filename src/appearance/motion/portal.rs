use crate::service::task_manager::{Scope, Task as IoTask};
use futures_lite::StreamExt as _;
use gpui_kit::{App, Global, Task};
use std::collections::HashMap;
use tokio::sync::watch;
use zbus::zvariant::OwnedValue;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "reduced-motion";

type Settings = HashMap<String, HashMap<String, OwnedValue>>;

struct Watch {
    io: IoTask<()>,
    _updates: Task<()>,
}
impl Global for Watch {}
impl Drop for Watch {
    fn drop(&mut self) {
        self.io.abort();
    }
}

pub(super) fn follow(tasks: &Scope, cx: &mut App) {
    let (sender, mut receiver) = watch::channel(None);
    let cancel = tasks.token();
    let io = tasks.spawn_named("Watch reduced-motion preference", async move {
        let result = tokio::select! {
            _ = cancel.cancelled() => return,
            result = listen(sender) => result,
        };
        if let Err(error) = result {
            tracing::warn!(%error, "motion portal unavailable; keeping the last system preference");
        }
    });
    let updates = cx.spawn(async move |cx| {
        while receiver.changed().await.is_ok() {
            let reduced = *receiver.borrow_and_update();
            if let Some(reduced) = reduced {
                cx.update(|cx| super::system_changed(reduced, cx));
            }
        }
    });
    cx.set_global(Watch {
        io,
        _updates: updates,
    });
}

async fn listen(sender: watch::Sender<Option<bool>>) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    listen_on(connection, sender).await
}

async fn listen_on(
    connection: zbus::Connection,
    sender: watch::Sender<Option<bool>>,
) -> zbus::Result<()> {
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .await?;
    // Subscribe first so a change during the initial read is not lost. ReadAll
    // is available in v1 and avoids the legacy Read method's nested variants.
    let mut changes = proxy.receive_signal("SettingChanged").await?;
    read(&proxy, &sender).await?;
    while let Some(message) = changes.next().await {
        let (namespace, key, _value): (String, String, OwnedValue) =
            message.body().deserialize()?;
        if namespace == NAMESPACE && key == KEY {
            // Read the current value rather than replay a possibly queued signal
            // from before the initial snapshot. This is event-driven, not polling.
            read(&proxy, &sender).await?;
        }
    }
    Ok(())
}

async fn read(proxy: &zbus::Proxy<'_>, sender: &watch::Sender<Option<bool>>) -> zbus::Result<()> {
    let settings: Settings = proxy.call("ReadAll", &(vec![NAMESPACE],)).await?;
    let reduced = settings
        .get(NAMESPACE)
        .and_then(|settings| settings.get(KEY))
        .and_then(|value| u32::try_from(value).ok())
        == Some(1);
    sender.send_replace(Some(reduced));
    Ok(())
}
