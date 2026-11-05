//! ACP connections and connection-local callback identities.
use super::*;
use bas::service::acp::{Incoming, Link, prepare, sdk};
use std::collections::HashMap;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub(crate) struct Pool {
    command: Arc<Mutex<Option<CommandConnection>>>,
    callbacks: Arc<std::sync::Mutex<HashMap<(SessionId, String), Callback>>>,
}
struct CommandConnection {
    session: SessionId,
    link: Arc<Link>,
}
struct Callback {
    request: sdk::UntypedMessage,
    responder: sdk::Responder,
    _answered: tokio::sync::oneshot::Sender<()>,
}
impl Pool {
    /// # Errors
    /// Returns preparation, initialization or session-load failures without resending commands.
    pub async fn session(&self, client: &Client, session: SessionId) -> Result<Arc<Link>> {
        let mut command = self.command.lock().await;
        if let Some(connection) = command
            .as_ref()
            .filter(|connection| connection.session == session && !connection.link.is_closed())
        {
            return Ok(connection.link.clone());
        }
        let (link, mut incoming) = open(client, session, false).await?;
        let peer = link.peer.clone();
        peer.spawn(async move {
            while let Some(event) = incoming.recv().await {
                if let Incoming::Request(_, responder) = event {
                    responder.respond_with_error(acp::Error::method_not_found())?;
                }
            }
            Ok(())
        })?;
        initialize(&link, client, session).await?;
        let link = Arc::new(link);
        *command = Some(CommandConnection {
            session,
            link: link.clone(),
        });
        Ok(link)
    }
    /// # Errors
    /// Rejects expired callbacks, invalid permission choices and closed connections.
    pub fn respond(
        &self,
        session: SessionId,
        id: String,
        response: serde_json::Value,
    ) -> Result<()> {
        let mut pending = self.callbacks.lock().expect("ACP callbacks");
        let key = (session, id);
        let callback = pending
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("ACP request is no longer pending"))?;
        if callback.responder.cancellation().is_cancelled() {
            anyhow::bail!("ACP request was cancelled");
        }
        if callback.request.method == "session/request_permission" {
            let request: acp::RequestPermissionRequest =
                serde_json::from_value(callback.request.params.clone())?;
            let response: acp::RequestPermissionResponse =
                serde_json::from_value(response.clone())?;
            if let acp::RequestPermissionOutcome::Selected(selected) = response.outcome
                && !request
                    .options
                    .iter()
                    .any(|option| option.option_id == selected.option_id)
            {
                anyhow::bail!("unknown permission option");
            }
        }
        pending
            .remove(&key)
            .expect("checked callback")
            .responder
            .respond_with_result(Ok(response))?;
        Ok(())
    }
}
async fn open(
    client: &Client,
    session: SessionId,
    interactive: bool,
) -> Result<(Link, mpsc::Receiver<Incoming>)> {
    let prepared = prepare(
        client.channel(),
        pb::PrepareAcpRequest {
            target: Some(pb::prepare_acp_request::Target::Session(
                session.to_string(),
            )),
            interactive,
        },
    )
    .await?;
    Ok(Link::open(prepared).await?)
}
async fn initialize(link: &Link, client: &Client, session: SessionId) -> Result<()> {
    link.peer
        .send_request(
            acp::InitializeRequest::new(sdk::schema::ProtocolVersion::V1).client_capabilities(
                acp::ClientCapabilities::new().elicitation(
                    acp::ElicitationCapabilities::new()
                        .form(acp::ElicitationFormCapabilities::new())
                        .url(acp::ElicitationUrlCapabilities::new()),
                ),
            ),
        )
        .block_task()
        .await?;
    let snapshot = client
        .clone()
        .get_session(pb::GetSessionRequest {
            session: session.to_string(),
        })
        .await?
        .into_inner()
        .session
        .ok_or_else(|| anyhow::anyhow!("missing session"))?;
    link.peer
        .send_request(acp::LoadSessionRequest::new(
            session.to_string(),
            snapshot
                .cwd
                .ok_or_else(|| anyhow::anyhow!("session has no working directory"))?,
        ))
        .block_task()
        .await?;
    Ok(())
}

pub(super) async fn watch(
    pool: Pool,
    client: Client,
    updates: mpsc::Sender<Update>,
    session: SessionId,
    epoch: u64,
) {
    loop {
        let result = watch_once(&pool, &client, &updates, session, epoch).await;
        let message = result
            .err()
            .map(Failure::from)
            .unwrap_or_else(|| Failure::InvalidPayload("ACP stream ended".into()));
        let terminal = message
            .code()
            .is_some_and(|code| terminal_watch_status(code) || code == Code::Unimplemented);
        if updates
            .send(Update::PermissionUnavailable {
                epoch,
                message: message.clone(),
            })
            .await
            .is_err()
        {
            return;
        }
        if updates
            .send(Update::ElicitationUnavailable { epoch, message })
            .await
            .is_err()
        {
            return;
        }
        if terminal {
            return;
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}
async fn watch_once(
    pool: &Pool,
    client: &Client,
    updates: &mpsc::Sender<Update>,
    session: SessionId,
    epoch: u64,
) -> Result<()> {
    let (link, mut incoming) = open(client, session, true).await?;
    let _callbacks = CallbackLease {
        pool: pool.clone(),
        session,
        prefix: format!("{}:", link.prepared.id),
    };
    // Drain replay while loading; the durable Bas journal supplies the UI timeline.
    let ready = tokio::time::timeout(REQUEST_TIMEOUT, initialize(&link, client, session));
    tokio::pin!(ready);
    let mut initialized = false;
    let mut cancellations = futures::stream::FuturesUnordered::new();
    let mut permissions = HashMap::<String, PendingPermission>::new();
    let mut elicitations = HashMap::<String, PendingElicitation>::new();
    use futures::StreamExt;
    loop {
        tokio::select! {
            result = &mut ready, if !initialized => { result??; initialized = true; }
            _ = updates.closed() => return Ok(()),
            id = cancellations.next(), if !cancellations.is_empty() => {
                let (id, cancelled): (String, bool) = id.expect("pending callback");
                pool.callbacks.lock().expect("ACP callbacks").remove(&(session, id.clone()));
                permissions.remove(&id);
                if cancelled { elicitations.remove(&id); }
                else if let Some(value) = elicitations.get_mut(&id) { value.answered = true; }
                publish(updates, epoch, &permissions, &elicitations, vec![]).await?;
            }
            incoming = incoming.recv() => match incoming.ok_or_else(|| anyhow::anyhow!("ACP stream ended"))? {
                Incoming::Request(request, responder) => {
                    let id = format!("{}:{}", link.prepared.id, serde_json::to_string(responder.id())?);
                    match request.method.as_str() {
                        "session/request_permission" => {
                            let value: acp::RequestPermissionRequest = serde_json::from_value(request.params.clone())?;
                            if value.session_id.to_string() != session.to_string() { anyhow::bail!("permission belongs to another session"); }
                            permissions.insert(id.clone(), PendingPermission { request_id: id.clone(), request: value });
                        }
                        "elicitation/create" => {
                            elicitations.insert(id.clone(), PendingElicitation { request_id: id.clone(), answered: false, request: bas::authentication::elicitation::parse_request(&request.params).map_err(|error| Failure::InvalidPayload(error.to_string())) });
                        }
                        _ => { responder.respond_with_error(acp::Error::method_not_found())?; continue; }
                    }
                    let cancelled = responder.cancellation();
                    let cancelled_id = id.clone();
                    let (answered, replied) = tokio::sync::oneshot::channel();
                    cancellations.push(async move {
                        let cancelled = tokio::select! { _ = cancelled.cancelled() => true, _ = replied => false };
                        (cancelled_id, cancelled)
                    });
                    pool.callbacks.lock().expect("ACP callbacks").insert((session, id), Callback { request, responder, _answered: answered });
                    publish(updates, epoch, &permissions, &elicitations, vec![]).await?;
                }
                Incoming::Notification(notification) if notification.method == "elicitation/complete" => {
                    let id = notification.params.get("elicitationId").and_then(serde_json::Value::as_str).ok_or_else(|| anyhow::anyhow!("missing elicitation ID"))?;
                    elicitations.retain(|_, entry| entry.request.as_ref().map_or(true, |request| request.raw.get("elicitationId").and_then(serde_json::Value::as_str) != Some(id)));
                    publish(updates, epoch, &permissions, &elicitations, vec![id.to_owned()]).await?;
                }
                Incoming::Notification(_) => {}
            }
        }
    }
}
async fn publish(
    updates: &mpsc::Sender<Update>,
    epoch: u64,
    permissions: &HashMap<String, PendingPermission>,
    elicitations: &HashMap<String, PendingElicitation>,
    completed_ids: Vec<String>,
) -> Result<()> {
    updates
        .send(Update::Permissions {
            epoch,
            pending: permissions.values().cloned().collect(),
        })
        .await?;
    updates
        .send(Update::Elicitations {
            epoch,
            pending: elicitations.values().cloned().collect(),
            completed_ids,
        })
        .await?;
    Ok(())
}

struct CallbackLease {
    pool: Pool,
    session: SessionId,
    prefix: String,
}
impl Drop for CallbackLease {
    fn drop(&mut self) {
        self.pool
            .callbacks
            .lock()
            .expect("ACP callbacks")
            .retain(|(session, id), _| *session != self.session || !id.starts_with(&self.prefix));
    }
}
