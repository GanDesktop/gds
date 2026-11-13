use super::*;

/// Run the durable conversation journal alongside its ACP controller.
#[cfg(test)]
pub(super) async fn watch_session(
    protocol: protocol::Pool,
    client: Client,
    updates: mpsc::Sender<Update>,
    session: SessionId,
    epoch: u64,
) {
    let (store, _) = history::Store::new(None, &gds::service::task_manager::Scope::default());
    watch_cached(protocol, client, updates, session, epoch, None, store).await;
}

pub(super) async fn watch_cached(
    protocol: protocol::Pool,
    client: Client,
    updates: mpsc::Sender<Update>,
    session: SessionId,
    epoch: u64,
    mut cursor: Option<u64>,
    store: history::Store,
) {
    if cursor.is_none()
        && let Some(window) = store.latest(session).await
    {
        cursor = Some(window.cursor);
        if updates
            .send(Update::HistoryWindow {
                epoch,
                window: window.into(),
                cached: true,
            })
            .await
            .is_err()
        {
            return;
        }
    }
    tokio::join!(
        super::watch_cached(
            client.clone(),
            updates.clone(),
            session,
            epoch,
            cursor,
            store
        ),
        protocol::watch(protocol, client, updates, session, epoch)
    );
}
