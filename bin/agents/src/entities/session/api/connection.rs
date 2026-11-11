//! The Tokio side of the desktop client. GPUI only exchanges owned messages.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use bas::{
    acp,
    model::*,
    service::{Client, HistoryOverview, HistoryWindow, pb, tls::ConnectionOptions as Target},
};
use gds::service::task_manager::{Group, Scope, Task as IoTask};
use tokio::{
    sync::{Notify, mpsc},
    task::AbortHandle,
};
use tonic::Code;

mod cache;
mod history;
mod prepared;
pub(crate) use prepared::{HistoryWeight, Prepared, PreparedHistory, PreparedLocation};
mod model;
mod permissions;
mod protocol;
mod tracking;

pub(crate) use crate::shared::api::failure::{Condition, Failure, FailureContext};
pub(crate) use model::{AgentSummary, PendingElicitation};

const EVENT_CAPACITY: usize = 256;
const HISTORY_TURNS: u32 = 1;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub(crate) struct CreateSession {
    pub workspace: Option<WorkspaceId>,
    pub epoch: u64,
    pub agent: String,
    pub title: String,
    pub cwd: String,
}

#[derive(Clone, Debug)]
pub(crate) enum WorkspaceMutation {
    Create { cwd: std::path::PathBuf },
    Archive { id: WorkspaceId, archived: bool },
    Delete { id: WorkspaceId, force: bool },
}

pub(crate) enum Command {
    Workspace(WorkspaceMutation),
    Refresh,
    AgentHistory {
        epoch: u64,
        agent: String,
        cwd: Option<std::path::PathBuf>,
    },
    ImportHistory {
        epoch: u64,
        agent: String,
        info: acp::SessionInfo,
    },
    DeleteAgentHistory {
        epoch: u64,
        agent: String,
        id: String,
    },
    CloseSession(SessionId),
    RestartSession(SessionId),
    SetDirectories {
        session: SessionId,
        directories: Vec<std::path::PathBuf>,
    },
    Create(CreateSession),
    Rename {
        session: SessionId,
        title: String,
    },
    Archive {
        session: SessionId,
        archived: bool,
    },
    Delete(SessionId),
    Prompt {
        session: SessionId,
        submission: Arc<PromptSubmission>,
        title: Option<String>,
    },
    ResolvePrompt {
        session: SessionId,
        submission: Arc<PromptSubmission>,
        retry: bool,
    },
    Cancel(SessionId),
    SetConfig(acp::SetSessionConfigOptionRequest),
    RespondPermission {
        session: SessionId,
        request_id: String,
        outcome: acp::RequestPermissionOutcome,
    },
    RespondElicitation {
        session: SessionId,
        request_id: String,
        response: serde_json::Value,
    },
    Watch {
        session: SessionId,
        epoch: u64,
    },
    Resume {
        session: SessionId,
        epoch: u64,
        cursor: u64,
    },
    OlderHistory {
        session: SessionId,
        epoch: u64,
        before: u64,
    },
    HistoryOverview {
        session: SessionId,
        epoch: u64,
        request: u64,
    },
    HistoryLocation {
        session: SessionId,
        epoch: u64,
        request: u64,
        cursor: u64,
    },
    ForgetHistory(SessionId),
    Detach,
}

pub(crate) struct Catalog {
    pub workspaces: Vec<bas::model::Workspace>,
    pub agents: Vec<AgentSummary>,
    pub sessions: Vec<SessionSnapshot>,
}

pub(crate) struct AgentHistory {
    pub sessions: acp::ListSessionsResponse,
    pub initialization: acp::InitializeResponse,
    pub imported: std::collections::HashMap<String, String>,
}

pub(crate) enum Update {
    Workspace {
        mutation: WorkspaceMutation,
        result: Result<Option<bas::model::Workspace>, Failure>,
    },
    AgentHistory {
        epoch: u64,
        result: Result<AgentHistory, Failure>,
    },
    ImportedHistory {
        epoch: u64,
        result: Result<SessionSnapshot, Failure>,
    },
    DeletedAgentHistory {
        epoch: u64,
        id: String,
        result: Result<(), Failure>,
    },
    ClosedSession {
        session: SessionId,
        result: Result<SessionSnapshot, Failure>,
    },
    RestartedSession {
        session: SessionId,
        result: Result<SessionSnapshot, Failure>,
    },
    DirectoriesUpdated {
        session: SessionId,
        result: Result<SessionSnapshot, Failure>,
    },
    Elicitations {
        epoch: u64,
        pending: Vec<PendingElicitation>,
        completed_ids: Vec<String>,
    },
    ElicitationUnavailable {
        epoch: u64,
        message: Failure,
    },
    ElicitationResponded {
        session: SessionId,
        request_id: String,
        result: Result<(), Failure>,
    },
    Permissions {
        epoch: u64,
        pending: Vec<PendingPermission>,
    },
    PermissionUnavailable {
        epoch: u64,
        message: Failure,
    },
    PermissionResponded {
        session: SessionId,
        request_id: String,
        result: Result<(), Failure>,
    },
    Configured {
        session: SessionId,
        result: Result<acp::SetSessionConfigOptionResponse, Failure>,
    },
    Catalog(Result<Catalog, Failure>),
    Created {
        epoch: u64,
        result: Result<SessionSnapshot, Failure>,
    },
    AuthenticationRequired {
        epoch: u64,
        agent: String,
    },
    Renamed {
        session: SessionId,
        result: Result<(), Failure>,
    },
    Archived {
        session: SessionId,
        result: Result<SessionSnapshot, Failure>,
    },
    Deleted {
        session: SessionId,
        result: Result<(), Failure>,
    },
    Submission {
        session: SessionId,
        submission: Arc<PromptSubmission>,
        result: Result<PromptSubmissionOutcome, Failure>,
    },
    TrackedTurn {
        event: Event,
    },
    TrackingFailed {
        session: SessionId,
        receipt: TurnReceipt,
        message: Failure,
    },
    Cancelled {
        session: SessionId,
        result: Result<(), Failure>,
    },
    HistoryWindow {
        epoch: u64,
        window: PreparedHistory,
        cached: bool,
    },
    OlderHistory {
        epoch: u64,
        before: u64,
        result: Result<PreparedHistory, Failure>,
    },
    HistoryOverview {
        session: SessionId,
        epoch: u64,
        request: u64,
        result: Result<Prepared<HistoryOverview>, Failure>,
    },
    HistoryLocation {
        session: SessionId,
        epoch: u64,
        request: u64,
        result: Result<PreparedLocation, Failure>,
    },
    HistoryMeta {
        epoch: u64,
        session: Prepared<SessionSnapshot>,
    },
    Management {
        epoch: u64,
        event: Prepared<Event>,
    },
    Event {
        epoch: u64,
        event: Prepared<Event>,
    },
    Stream {
        epoch: u64,
        state: StreamState,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StreamState {
    Loading,
    Live,
    Reconnecting(Failure),
    Closed,
    Unavailable(Failure),
}

pub(crate) struct Connection {
    scope: Option<gds::service::task_manager::Owner>,
    commands: mpsc::UnboundedSender<Command>,
    task: Option<IoTask<()>>,
}

impl Connection {
    pub fn new(
        server: Target,
        parent: &gds::service::task_manager::Scope,
    ) -> (Self, mpsc::Receiver<Update>) {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (updates, events) = mpsc::channel(EVENT_CAPACITY);
        let scope = parent.named_child("Agent connection / history");
        let worker = (*scope).clone();
        let task = scope.spawn_named("Maintain agent connection", async move {
            let channel = match server.channel_lazy(REQUEST_TIMEOUT) {
                Ok(channel) => channel,
                Err(error) => {
                    let _ = updates
                        .send(Update::Catalog(Err(Failure::from(anyhow::Error::from(
                            error,
                        )))))
                        .await;
                    return;
                }
            };
            let cache = match cache::Cache::open(&worker, &server).await {
                Ok(cache) => Some(cache),
                Err(error) => {
                    tracing::warn!(%error, "History cache unavailable");
                    None
                }
            };
            run_cached_scoped(
                Client::new(channel).with_payload_limit(),
                receiver,
                updates,
                cache,
                worker,
            )
            .await;
        });
        (
            Self {
                scope: Some(scope),
                commands,
                task: Some(task),
            },
            events,
        )
    }

    /// Returns the unqueued command so its owner can finish local operation state.
    ///
    /// # Errors
    /// Returns the original command if the connection task has stopped receiving.
    pub fn send(&self, command: Command) -> Result<(), Box<Command>> {
        self.commands
            .send(command)
            .map_err(|error| Box::new(error.0))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(scope) = &self.scope {
            scope.cancel();
        }
        // Closing the command sender lets the worker drain its disk writes.
        self.task.take();
    }
}

#[cfg(test)]
async fn run_cached(
    client: Client,
    commands: mpsc::UnboundedReceiver<Command>,
    updates: mpsc::Sender<Update>,
    cache: Option<cache::Cache>,
) {
    run_cached_scoped(
        client,
        commands,
        updates,
        cache,
        Scope::with_handle(tokio::runtime::Handle::current(), false),
    )
    .await;
}

async fn run_cached_scoped(
    client: Client,
    mut commands: mpsc::UnboundedReceiver<Command>,
    updates: mpsc::Sender<Update>,
    cache: Option<cache::Cache>,
    scope: Scope,
) {
    let cancel = scope.token();
    let protocol = protocol::Pool::default();
    let (history, writer) = history::Store::new(cache, &scope);
    let refresh = Arc::new(Notify::new());
    let mut tasks = Group::new(scope);
    tasks.spawn_named(
        "Watch session catalog",
        catalog_loop(client.clone(), updates.clone(), refresh.clone()),
    );
    let mut attachment: Option<AbortHandle> = None;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    Command::Refresh => refresh.notify_one(),
                    Command::Watch { session, epoch } => {
                        if let Some(task) = attachment.take() { task.abort(); }
                        history.flush().await;
                        attachment = tasks.spawn_named("Watch session history and permissions", permissions::watch_cached(protocol.clone(), client.clone(), updates.clone(), session, epoch, None, history.clone()));
                    }
                    Command::Resume { session, epoch, cursor } => {
                        if let Some(task) = attachment.take() { task.abort(); }
                        history.flush().await;
                        attachment = tasks.spawn_named("Resume session history and permissions", permissions::watch_cached(protocol.clone(), client.clone(), updates.clone(), session, epoch, Some(cursor), history.clone()));
                    }
                    Command::OlderHistory { session, epoch, before } => {
                        let history = history.clone();
                        let client = client.clone();
                        let updates = updates.clone();
                        tasks.spawn_named("Load older session history", async move {
                            let result = history.older(client, session, before).await.map(Into::into);
                            let _ = updates.send(Update::OlderHistory { epoch, before, result }).await;
                        });
                    }
                    Command::HistoryOverview { session, epoch, request } => {
                        let mut client = client.clone();
                        let updates = updates.clone();
                        tasks.spawn_named("Load session history overview", async move {
                            let result = client.history_overview(session, REQUEST_TIMEOUT)
                                .await.map(Into::into).map_err(Failure::from);
                            let _ = updates.send(Update::HistoryOverview { session, epoch, request, result }).await;
                        });
                    }
                    Command::HistoryLocation { session, epoch, request, cursor } => {
                        let mut client = client.clone();
                        let updates = updates.clone();
                        tasks.spawn_named("Locate session history", async move {
                            // A located slice is not the latest suffix: never let it
                            // replace the live history cache or its attach frontier.
                            let result = client.history_window_at(session, cursor, REQUEST_TIMEOUT)
                                .await.map(Into::into).map_err(Failure::from);
                            let _ = updates.send(Update::HistoryLocation { session, epoch, request, result }).await;
                        });
                    }
                    Command::ForgetHistory(session) => {
                        history.forget(session).await;
                    }
                    Command::Detach => {
                        if let Some(task) = attachment.take() { task.abort(); }
                        history.flush().await;
                    }
                    command => {
                        let client = client.clone();
                        let protocol = protocol.clone();
                        let updates = updates.clone();
                        let refresh = refresh.clone();
                        tasks.spawn_named("Execute session command", async move {
                            operation(protocol, client, command, updates, refresh).await;
                        });
                    }
                }
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(error)) = result
                    && !error.is_cancelled()
                {
                    tracing::error!(%error, "GUI background task failed");
                }
            }
        }
    }
    tasks.abort_all();
    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result
            && !error.is_cancelled()
        {
            tracing::error!(%error, "GUI background task failed during shutdown");
        }
    }
    drop(history);
    if let Some(writer) = writer
        && let Err(error) = writer.await
    {
        tracing::error!(%error, "History writer task failed");
    }
}

async fn operation(
    protocol: protocol::Pool,
    client: Client,
    command: Command,
    updates: mpsc::Sender<Update>,
    refresh: Arc<Notify>,
) {
    let update = execute(protocol, client.clone(), command).await;
    let accepted = match &update {
        Update::Submission {
            session,
            result: Ok(PromptSubmissionOutcome::Accepted(receipt)),
            ..
        } => Some((*session, *receipt)),
        _ => None,
    };
    if updates.send(update).await.is_err() {
        return;
    }
    refresh.notify_one();
    if let Some((session, receipt)) = accepted {
        let result = tokio::select! {
            result = tracking::track(client, session, receipt) => result,
            _ = updates.closed() => return,
        };
        let update = match result {
            Ok(event) => Update::TrackedTurn { event },
            Err(message) => Update::TrackingFailed {
                session,
                receipt,
                message,
            },
        };
        let _ = updates.send(update).await;
        refresh.notify_one();
    }
}

async fn catalog_loop(mut client: Client, updates: mpsc::Sender<Update>, refresh: Arc<Notify>) {
    loop {
        let result = request(async {
            let agents = client
                .list_agents(pb::ListAgentsRequest {})
                .await?
                .into_inner()
                .agents;
            let sessions = client
                .list_sessions(pb::ListSessionsRequest {
                    include_archived: true,
                })
                .await?
                .into_inner()
                .sessions;
            let workspaces = client
                .list_workspaces(pb::ListWorkspacesRequest {
                    include_archived: true,
                })
                .await?
                .into_inner()
                .workspaces;
            Ok(Catalog {
                workspaces: workspaces
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?,
                agents: agents.into_iter().map(Into::into).collect(),
                sessions: sessions
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?,
            })
        })
        .await;
        if updates.send(Update::Catalog(result)).await.is_err() {
            return;
        }
        tokio::select! {
            _ = refresh.notified() => {}
            _ = tokio::time::sleep(REFRESH_INTERVAL) => {}
        }
    }
}

/// # Errors
/// Preserves known RPC and payload failures, or returns the request deadline.
pub(crate) async fn request<T>(future: impl Future<Output = Result<T>>) -> Result<T, Failure> {
    match tokio::time::timeout(REQUEST_TIMEOUT, future).await {
        Ok(result) => result.map_err(Failure::from),
        Err(_) => Err(Failure::Timeout),
    }
}

async fn create_session(client: &mut Client, form: CreateSession) -> Update {
    let owner = Owner::current();
    let request = pb::CreateSessionRequest {
        workspace_id: form.workspace.map(|id| id.to_string()),
        agent: form.agent.clone(),
        title: (!form.title.trim().is_empty()).then(|| form.title.trim().to_owned()),
        cwd: (!form.cwd.trim().is_empty()).then(|| form.cwd.trim().to_owned()),
        owner_user: Some(owner.user),
        owner_host: Some(owner.host),
    };
    let response = tokio::time::timeout(REQUEST_TIMEOUT, client.create_session(request))
        .await
        .map_err(|_| Failure::Timeout.context(FailureContext::RpcDeadline))
        .and_then(|result| result.map_err(Failure::from))
        .map(tonic::Response::into_inner);
    creation_result(form.epoch, form.agent, response)
}

fn creation_result(
    epoch: u64,
    agent: String,
    response: Result<pb::CreateSessionResponse, Failure>,
) -> Update {
    if let Err(Failure::Rpc {
        code: Code::Unauthenticated,
        details,
        ..
    }) = &response
        && serde_json::from_slice::<acp::Error>(details)
            .is_ok_and(|error| error.code == acp::ErrorCode::AuthRequired)
    {
        return Update::AuthenticationRequired { epoch, agent };
    }
    let result = response
        .and_then(|response| {
            response
                .session
                .ok_or(Failure::Condition(Condition::MissingCreatedSession))
        })
        .and_then(|session| {
            session
                .try_into()
                .map_err(|error: bas::service::Error| Failure::InvalidPayload(error.to_string()))
        });
    Update::Created { epoch, result }
}

async fn execute(protocol: protocol::Pool, mut client: Client, command: Command) -> Update {
    match command {
        Command::AgentHistory { epoch, agent, cwd } => {
            let result = request(async {
                let response = client
                    .list_agent_sessions(pb::ListAgentSessionsRequest {
                        agent,
                        cwd: cwd
                            .map(|path| bas::service::utf8_cwd(&path).map(str::to_owned))
                            .transpose()?,
                    })
                    .await?
                    .into_inner();
                Ok(AgentHistory {
                    sessions: response
                        .sessions
                        .ok_or_else(|| anyhow::anyhow!("Missing Agent history"))?
                        .decode()?,
                    initialization: response
                        .initialization
                        .ok_or_else(|| anyhow::anyhow!("Missing Agent initialization"))?
                        .decode()?,
                    imported: response.imported_sessions,
                })
            })
            .await;
            Update::AgentHistory { epoch, result }
        }
        Command::ImportHistory { epoch, agent, info } => {
            let result = request(async {
                let response = client
                    .import_agent_session(pb::ImportAgentSessionRequest {
                        agent,
                        session_info: Some((&info).into()),
                    })
                    .await?
                    .into_inner();
                response
                    .session
                    .ok_or_else(|| anyhow::anyhow!("Missing imported session"))?
                    .try_into()
                    .map_err(Into::into)
            })
            .await;
            Update::ImportedHistory { epoch, result }
        }
        Command::DeleteAgentHistory { epoch, agent, id } => {
            let result = request(async {
                client
                    .delete_agent_session(pb::DeleteAgentSessionRequest {
                        agent,
                        session_id: id.clone(),
                    })
                    .await?;
                Ok(())
            })
            .await;
            Update::DeletedAgentHistory { epoch, id, result }
        }
        Command::SetDirectories {
            session,
            directories,
        } => {
            let result = request(async {
                let response = client
                    .set_session_directories(pb::SetSessionDirectoriesRequest {
                        session: session.to_string(),
                        additional_directories: directories
                            .iter()
                            .map(|path| bas::service::utf8_cwd(path).map(str::to_owned))
                            .collect::<Result<_, _>>()?,
                    })
                    .await?
                    .into_inner();
                response
                    .session
                    .ok_or_else(|| anyhow::anyhow!("Missing workspace update"))?
                    .try_into()
                    .map_err(Into::into)
            })
            .await;
            Update::DirectoriesUpdated { session, result }
        }
        Command::RestartSession(session) => {
            let result = request(async {
                let response = client
                    .restart_session(pb::GetSessionRequest {
                        session: session.to_string(),
                    })
                    .await?
                    .into_inner();
                response
                    .session
                    .ok_or_else(|| anyhow::anyhow!("Missing restarted session"))?
                    .try_into()
                    .map_err(Into::into)
            })
            .await;
            Update::RestartedSession { session, result }
        }
        Command::CloseSession(session) => {
            let result = request(async {
                let response = client
                    .close_session(pb::GetSessionRequest {
                        session: session.to_string(),
                    })
                    .await?
                    .into_inner();
                response
                    .session
                    .ok_or_else(|| anyhow::anyhow!("Missing closed session"))?
                    .try_into()
                    .map_err(Into::into)
            })
            .await;
            Update::ClosedSession { session, result }
        }
        Command::RespondElicitation {
            session,
            request_id,
            response,
        } => {
            let result = protocol
                .respond(session, request_id.clone(), response)
                .map_err(Failure::from);
            Update::ElicitationResponded {
                session,
                request_id,
                result,
            }
        }
        Command::RespondPermission {
            session,
            request_id,
            outcome,
        } => {
            let result = serde_json::to_value(acp::RequestPermissionResponse::new(outcome))
                .map_err(anyhow::Error::from)
                .and_then(|response| protocol.respond(session, request_id.clone(), response))
                .map_err(Failure::from);
            Update::PermissionResponded {
                session,
                request_id,
                result,
            }
        }
        Command::Workspace(mutation) => {
            let result = request(async {
                match &mutation {
                    WorkspaceMutation::Create { cwd } => {
                        let workspace = client
                            .create_workspace(pb::CreateWorkspaceRequest {
                                cwd: bas::service::utf8_cwd(cwd)?.into(),
                                name: None,
                            })
                            .await?
                            .into_inner();
                        Ok(Some(workspace.try_into()?))
                    }
                    WorkspaceMutation::Archive { id, archived } => {
                        let workspace = client
                            .set_workspace_archived(pb::SetWorkspaceArchivedRequest {
                                workspace_id: id.to_string(),
                                archived: *archived,
                            })
                            .await?
                            .into_inner();
                        Ok(Some(workspace.try_into()?))
                    }
                    WorkspaceMutation::Delete { id, force } => {
                        client
                            .delete_workspace(pb::DeleteWorkspaceRequest {
                                workspace_id: id.to_string(),
                                force: *force,
                            })
                            .await?;
                        Ok(None)
                    }
                }
            })
            .await;
            Update::Workspace { mutation, result }
        }
        Command::Create(form) => create_session(&mut client, form).await,
        Command::Rename { session, title } => {
            let result = request(async {
                client
                    .set_title(pb::SetTitleRequest {
                        session: session.to_string(),
                        title: title.clone(),
                    })
                    .await?;
                Ok(())
            })
            .await;
            Update::Renamed { session, result }
        }
        Command::Archive { session, archived } => {
            let result = request(async {
                let response = client
                    .set_archived(pb::SetArchivedRequest {
                        session: session.to_string(),
                        archived,
                    })
                    .await
                    .map_err(|error| {
                        if error.code() == Code::Unimplemented {
                            anyhow::Error::from(
                                Failure::from(error).context(FailureContext::ArchiveUnsupported),
                            )
                        } else {
                            error.into()
                        }
                    })?
                    .into_inner();
                response
                    .session
                    .ok_or(Failure::Condition(Condition::MissingArchivedSession))?
                    .try_into()
                    .map_err(Into::into)
            })
            .await;
            Update::Archived { session, result }
        }
        Command::Delete(session) => {
            let result = request(async {
                client
                    .delete_session(pb::DeleteSessionRequest {
                        session: session.to_string(),
                    })
                    .await?;
                Ok(())
            })
            .await;
            Update::Deleted { session, result }
        }
        Command::Prompt {
            session,
            submission,
            title,
        } => {
            let result = crate::entities::session::submission::submit(
                &mut client,
                session,
                &submission,
                title,
            )
            .await;
            Update::Submission {
                session,
                submission,
                result,
            }
        }
        Command::ResolvePrompt {
            session,
            submission,
            retry,
        } => {
            let result = request(async {
                if retry {
                    Ok(client.submit_prompt(&submission).await?)
                } else {
                    client
                        .prompt_acceptance(session, submission.turn)
                        .await?
                        .map(PromptSubmissionOutcome::Accepted)
                        .ok_or_else(|| Failure::Condition(Condition::UnknownSubmission).into())
                }
            })
            .await;
            Update::Submission {
                session,
                submission,
                result,
            }
        }
        Command::SetConfig(config) => {
            let session = config
                .session_id
                .to_string()
                .parse()
                .expect("GUI uses daemon session ids");
            let result = request(async {
                Ok(protocol
                    .session(&client, session)
                    .await?
                    .peer
                    .send_request(config)
                    .block_task()
                    .await?)
            })
            .await;
            Update::Configured { session, result }
        }
        Command::Cancel(session) => {
            let result = request(async {
                protocol
                    .session(&client, session)
                    .await?
                    .peer
                    .send_notification(acp::CancelNotification::new(session.to_string()))?;
                Ok(())
            })
            .await;
            Update::Cancelled { session, result }
        }
        Command::Refresh
        | Command::Watch { .. }
        | Command::Resume { .. }
        | Command::OlderHistory { .. }
        | Command::HistoryOverview { .. }
        | Command::HistoryLocation { .. }
        | Command::ForgetHistory(_)
        | Command::Detach => {
            unreachable!("control commands are handled by run")
        }
    }
}

async fn watch_cached(
    mut client: Client,
    updates: mpsc::Sender<Update>,
    session: SessionId,
    epoch: u64,
    mut cursor: Option<u64>,
    store: history::Store,
) {
    let mut resynced = false;
    let mut management_cursor = cursor;
    let mut knowledge = history::Knowledge::default();
    if let Some(frontier) = cursor
        && let Some(window) = store.latest(session).await
        && window.cursor == frontier
    {
        knowledge.reset(&window);
    }
    if cursor.is_some() {
        let meta = request(async {
            let meta: SessionSnapshot = client
                .get_session(pb::GetSessionRequest {
                    session: session.to_string(),
                })
                .await?
                .into_inner()
                .session
                .ok_or(Failure::Condition(Condition::MissingSessionInfo))?
                .try_into()?;
            if meta.meta.id != session {
                anyhow::bail!("metadata returned another session");
            }
            Ok(meta)
        })
        .await;
        match meta {
            Ok(meta) => {
                if cursor.is_some_and(|cursor| cursor > meta.meta.revision.raw()) {
                    cursor = None;
                }
                management_cursor = Some(meta.meta.revision.raw());
                let failure = meta.runtime.failure.clone().map(Failure::Runtime);
                let _ = updates
                    .send(Update::HistoryMeta {
                        epoch,
                        session: meta.into(),
                    })
                    .await;
                if let Some(failure) = failure {
                    let _ = updates
                        .send(Update::Stream {
                            epoch,
                            state: StreamState::Unavailable(failure),
                        })
                        .await;
                    return;
                }
            }
            Err(error) => {
                let _ = updates
                    .send(Update::Stream {
                        epoch,
                        state: StreamState::Unavailable(error),
                    })
                    .await;
                return;
            }
        }
    }
    loop {
        if cursor.is_none() {
            let _ = updates
                .send(Update::Stream {
                    epoch,
                    state: StreamState::Loading,
                })
                .await;
            match load_initial(
                &mut client,
                &updates,
                session,
                epoch,
                &store,
                &mut knowledge,
            )
            .await
            {
                Ok(frontier) => {
                    cursor = Some(frontier);
                    management_cursor = Some(frontier);
                }
                Err(error) => {
                    tracing::warn!(%session, epoch, %error, "Conversation history load failed");
                    let _ = updates
                        .send(Update::Stream {
                            epoch,
                            state: StreamState::Unavailable(error),
                        })
                        .await;
                    return;
                }
            }
        }
        let result = tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.observe_acp(pb::AttachRequest {
                session: session.to_string(),
                cursor,
                replay: 0,
            }),
        )
        .await;
        let mut stream = match result {
            Ok(Ok(response)) => response.into_inner(),
            Ok(Err(error)) => {
                if error.code() == Code::DataLoss {
                    if resynced {
                        let _ = updates
                            .send(Update::Stream {
                                epoch,
                                state: StreamState::Unavailable(error.into()),
                            })
                            .await;
                        return;
                    }
                    resynced = true;
                    cursor = None;
                }
                if terminal_watch_status(error.code()) {
                    let _ = updates
                        .send(Update::Stream {
                            epoch,
                            state: StreamState::Unavailable(error.into()),
                        })
                        .await;
                    return;
                }
                if !retry(&updates, epoch, error.into()).await {
                    return;
                }
                continue;
            }
            Err(_) => {
                if !retry(
                    &updates,
                    epoch,
                    Failure::Timeout.context(FailureContext::StreamDeadline),
                )
                .await
                {
                    return;
                }
                continue;
            }
        };
        let state_after = management_cursor.unwrap_or(cursor.unwrap_or(0));
        let mut management = match tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.attach(pb::AttachRequest {
                session: session.to_string(),
                cursor: Some(state_after),
                replay: 0,
            }),
        )
        .await
        {
            Ok(Ok(response)) => response.into_inner(),
            Ok(Err(error)) => {
                if terminal_watch_status(error.code()) {
                    let _ = updates
                        .send(Update::Stream {
                            epoch,
                            state: StreamState::Unavailable(error.into()),
                        })
                        .await;
                    return;
                }
                if error.code() == Code::DataLoss {
                    cursor = None;
                    management_cursor = None;
                }
                if !retry(&updates, epoch, error.into()).await {
                    return;
                }
                continue;
            }
            Err(_) => {
                if !retry(&updates, epoch, Failure::Timeout).await {
                    return;
                }
                continue;
            }
        };
        if updates
            .send(Update::Stream {
                epoch,
                state: StreamState::Live,
            })
            .await
            .is_err()
        {
            return;
        }
        let mut management_open = true;
        let mut protocol_open = true;
        loop {
            if !protocol_open && !management_open {
                let _ = updates
                    .send(Update::Stream {
                        epoch,
                        state: StreamState::Closed,
                    })
                    .await;
                return;
            }
            let message = tokio::select! {
                event = stream.message(), if protocol_open => event,
                event = management.message(), if management_open => {
                    match event {
                        Ok(Some(raw)) => {
                            let event = match Event::try_from(raw) {
                                Ok(event) if event.session == session && event.cursor.raw() > state_after
                                    && event.management().is_some_and(|management| management.kind == event.kind) && event.protocol.is_none() => event,
                                _ => {
                                    let _ = updates.send(Update::Stream { epoch, state: StreamState::Unavailable(Failure::InvalidPayload("invalid BAS management event".into())) }).await;
                                    return;
                                }
                            };
                            if management_cursor.is_none_or(|cursor| event.cursor.raw() > cursor) {
                                management_cursor = Some(event.cursor.raw());
                                if !matches!(event.kind, EventKind::ActivityChanged { .. }) { store.event(&event).await; }
                                if updates.send(Update::Management { epoch, event: event.into() }).await.is_err() { return; }
                            }
                            continue;
                        }
                        Ok(None) => { management_open = false; continue; }
                        Err(error) => {
                            if terminal_watch_status(error.code()) {
                                let _ = updates.send(Update::Stream { epoch, state: StreamState::Unavailable(error.into()) }).await;
                                return;
                            }
                            if error.code() == Code::DataLoss { cursor = None; management_cursor = None; }
                            if !retry(&updates, epoch, error.into()).await { return; }
                            break;
                        }
                    }
                }
            };
            match message {
                Ok(Some(observation)) => {
                    if observation.session != session.to_string()
                        || (observation.cursor != 0 && observation.cursor <= cursor.unwrap_or(0))
                    {
                        let _ = updates
                            .send(Update::Stream {
                                epoch,
                                state: StreamState::Unavailable(Failure::InvalidPayload(
                                    "invalid ACP observation position".into(),
                                )),
                            })
                            .await;
                        return;
                    }
                    let events = match observation.history_events() {
                        Ok(events) => events,
                        Err(error) => {
                            let _ = updates
                                .send(Update::Stream {
                                    epoch,
                                    state: StreamState::Unavailable(
                                        Failure::InvalidPayload(error.to_string())
                                            .context(FailureContext::InvalidEvent),
                                    ),
                                })
                                .await;
                            return;
                        }
                    };
                    let mut reload = false;
                    for event in &events {
                        if event.is_transient() {
                            continue;
                        }
                        if knowledge.needs_reload(event) {
                            reload = true;
                            break;
                        }
                        knowledge.observe(event);
                    }
                    if reload {
                        knowledge.require(observation.cursor);
                        cursor = None;
                        break;
                    }
                    for event in events {
                        if !event.is_transient() {
                            store.event(&event).await;
                        }
                        if updates
                            .send(Update::Event {
                                epoch,
                                event: event.into(),
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    if observation.cursor != 0 {
                        cursor = Some(observation.cursor);
                        resynced = false;
                    }
                }
                Ok(None) => {
                    protocol_open = false;
                }
                Err(error) => {
                    if terminal_watch_status(error.code()) {
                        let _ = updates
                            .send(Update::Stream {
                                epoch,
                                state: StreamState::Unavailable(error.into()),
                            })
                            .await;
                        return;
                    }
                    if error.code() == Code::DataLoss {
                        if resynced {
                            let _ = updates
                                .send(Update::Stream {
                                    epoch,
                                    state: StreamState::Unavailable(error.into()),
                                })
                                .await;
                            return;
                        }
                        resynced = true;
                        cursor = None;
                    }
                    if !retry(&updates, epoch, error.into()).await {
                        return;
                    }
                    break;
                }
            }
        }
    }
}

async fn load_initial(
    client: &mut Client,
    updates: &mpsc::Sender<Update>,
    session: SessionId,
    epoch: u64,
    store: &history::Store,
    knowledge: &mut history::Knowledge,
) -> Result<u64, Failure> {
    let window = client
        .history_window(session, None, HISTORY_TURNS, REQUEST_TIMEOUT)
        .await?;
    let frontier = window.cursor;
    knowledge.validate(frontier)?;
    let failure = window.session.runtime.failure.clone();
    knowledge.reset(&window);
    store.window(&window).await;
    let _ = updates
        .send(Update::HistoryWindow {
            epoch,
            window: window.into(),
            cached: false,
        })
        .await;
    if let Some(failure) = failure {
        return Err(Failure::Runtime(failure));
    }
    Ok(frontier)
}

fn terminal_watch_status(code: Code) -> bool {
    matches!(
        code,
        Code::NotFound | Code::InvalidArgument | Code::FailedPrecondition
    )
}

async fn retry(updates: &mpsc::Sender<Update>, epoch: u64, error: Failure) -> bool {
    tracing::warn!(epoch, %error, "Conversation stream reconnecting");
    if updates
        .send(Update::Stream {
            epoch,
            state: StreamState::Reconnecting(error),
        })
        .await
        .is_err()
    {
        return false;
    }
    tokio::time::sleep(RECONNECT_DELAY).await;
    !updates.is_closed()
}

#[cfg(test)]
mod tests;
