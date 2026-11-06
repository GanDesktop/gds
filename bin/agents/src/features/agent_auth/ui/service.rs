//! Owns the daemon authentication stream; dropping it cancels its ACP process.
use crate::shared::i18n::tr;
use std::path::PathBuf;

use bas::{
    authentication::{Invocation, Method, elicitation::Request},
    config::AgentSpec,
    service::pb,
};
use futures::channel::mpsc as futures_mpsc;
use tokio::{runtime::Handle, sync::mpsc};
use tokio_util::task::AbortOnDropHandle;

use crate::entities::agent::{Status, check};

mod failure;
pub(super) use failure::Failure;

pub(super) enum Event {
    Progress(String),
    Check(Status),
    Elicitation {
        request_id: String,
        request: Result<Request, Failure>,
    },
    ElicitationComplete(Option<serde_json::Value>),
    ElicitationCancelled(String),
    Terminal(Result<Invocation, Failure>),
}

pub(super) enum Control {
    ElicitationResponse {
        request_id: String,
        response: serde_json::Value,
    },
    TerminalResult {
        exit_code: i32,
    },
}

fn preparation(spec: &AgentSpec, method: &Method) -> Result<pb::PrepareAcpRequest, Failure> {
    Ok(pb::PrepareAcpRequest {
        callbacks_only: false,
        target: Some(pb::prepare_acp_request::Target::Authentication(
            pb::AuthenticateAgentRequest {
                agent: spec.id.to_string(),
                expected_spec_json: serde_json::to_vec(spec)
                    .map_err(|error| Failure::External(error.to_string()))?,
                method_id: method.id.clone(),
                expected_method_json: serde_json::to_vec(method)
                    .map_err(|error| Failure::External(error.to_string()))?,
            },
        )),
        interactive: true,
    })
}

pub(super) struct Stream {
    pub input: mpsc::UnboundedSender<Control>,
    pub events: futures_mpsc::UnboundedReceiver<Result<Event, Failure>>,
    pub task: AbortOnDropHandle<()>,
}

pub(super) fn start(
    service: check::Service,
    spec: AgentSpec,
    method: Method,
    path: PathBuf,
) -> Stream {
    let (input, receive) = mpsc::unbounded_channel();
    let (events, receiver) = futures_mpsc::unbounded();
    let runtime = service.runtime();
    let task = AbortOnDropHandle::new(runtime.spawn(async move {
        let result = run(service, spec, method, path, receive, events.clone()).await;
        if let Err(error) = result {
            let _ = events.unbounded_send(Err(error));
        }
    }));
    Stream {
        input,
        events: receiver,
        task,
    }
}

async fn run(
    service: check::Service,
    spec: AgentSpec,
    method: Method,
    path: PathBuf,
    mut input: mpsc::UnboundedReceiver<Control>,
    events: futures_mpsc::UnboundedSender<Result<Event, Failure>>,
) -> Result<(), Failure> {
    let mut client = check::verified_client(service.address(), &path)
        .await
        .map_err(Failure::External)?;
    use bas::service::acp::{Incoming, Link, prepare, sdk};
    use futures::StreamExt;
    let prepared = prepare(client.channel(), preparation(&spec, &method)?)
        .await
        .map_err(|error| Failure::StartRpc(error.into()))?;
    let (link, mut incoming) = Link::open(prepared)
        .await
        .map_err(|error| Failure::StartRpc(error.into()))?;
    let mut control = link.prepared.control.clone();
    let mut auxiliary = control
        .watch_auxiliary(pb::AcpConnectionRequest {
            connection: link.prepared.id.clone(),
        })
        .await
        .map_err(|error| Failure::StartRpc(error.into()))?
        .into_inner();
    link.peer
        .send_request(
            bas::acp::InitializeRequest::new(sdk::schema::ProtocolVersion::V1).client_capabilities(
                bas::acp::ClientCapabilities::new().elicitation(
                    bas::acp::ElicitationCapabilities::new()
                        .form(bas::acp::ElicitationFormCapabilities::new())
                        .url(bas::acp::ElicitationUrlCapabilities::new()),
                ),
            ),
        )
        .block_task()
        .await
        .map_err(|error| Failure::External(error.to_string()))?;
    let authenticate = link
        .peer
        .send_request(bas::acp::AuthenticateRequest::new(method.id.clone()))
        .block_task();
    tokio::pin!(authenticate);
    let mut pending = std::collections::HashMap::<String, sdk::Responder>::new();
    let mut cancelled = futures::stream::FuturesUnordered::new();
    loop {
        tokio::select! {
            result = &mut authenticate => {
                result.map_err(|error| Failure::External(error.to_string()))?;
                let response = client.check_agent(pb::CheckAgentRequest {
                    agent: spec.id.to_string(), expected_spec_json: serde_json::to_vec(&spec).map_err(|error| Failure::External(error.to_string()))?, connect: true,
                }).await.map_err(|error| Failure::StreamRpc(error.into()))?.into_inner();
                let status = check::decode_response(response).unwrap_or_else(Status::Failed);
                let _ = events.unbounded_send(Ok(Event::Check(status)));
                return Ok(());
            }
            id = cancelled.next(), if !cancelled.is_empty() => {
                let id: String = id.expect("pending cancellation");
                if pending.remove(&id).is_some() { let _ = events.unbounded_send(Ok(Event::ElicitationCancelled(id))); }
            }
            next = input.recv() => {
                match next {
                    None => return Ok(()),
                    Some(Control::TerminalResult { exit_code }) => {
                        control.terminal_result(pb::AcpTerminalResultRequest { connection: link.prepared.id.clone(), exit_code }).await.map_err(|error| Failure::StreamRpc(error.into()))?;
                    }
                    Some(Control::ElicitationResponse { request_id, response }) => {
                        let responder = pending.remove(&request_id).ok_or(Failure::InputClosed)?;
                        responder.respond_with_result(Ok(response)).map_err(|error| Failure::External(error.to_string()))?;
                    }
                }
            }
            next = incoming.recv() => {
                match next.ok_or(Failure::StreamEnded)? {
                    Incoming::Request(request, responder) if request.method == "elicitation/create" => {
                        let request_id = serde_json::to_string(responder.id()).map_err(|error| Failure::External(error.to_string()))?;
                        let request = bas::authentication::elicitation::parse_request(&request.params).map_err(|error| Failure::InvalidPayload(error.to_string()));
                        let cancellation = responder.cancellation();
                        let id = request_id.clone();
                        cancelled.push(async move { cancellation.cancelled().await; id });
                        pending.insert(request_id.clone(), responder);
                        let _ = events.unbounded_send(Ok(Event::Elicitation { request_id, request }));
                    }
                    Incoming::Request(_, responder) => { responder.respond_with_error(bas::acp::Error::method_not_found()).map_err(|error| Failure::External(error.to_string()))?; }
                    Incoming::Notification(notification) if notification.method == "elicitation/complete" => { let _ = events.unbounded_send(Ok(Event::ElicitationComplete(Some(notification.params)))); }
                    Incoming::Notification(_) => {}
                }
            }
            next = auxiliary.message() => {
                let next = next.map_err(|error| Failure::StreamRpc(error.into()))?.ok_or(Failure::StreamEnded)?;
                let event = match next.kind {
                    Some(pb::acp_auxiliary_event::Kind::Terminal(payload)) => Event::Terminal(payload.decode().map_err(|error| Failure::InvalidPayload(error.to_string()))),
                    Some(pb::acp_auxiliary_event::Kind::Progress(message)) => Event::Progress(message),
                    None => return Err(Failure::InvalidPayload("empty authentication control event".into())),
                };
                if events.unbounded_send(Ok(event)).is_err() { return Ok(()); }
            }
        }
    }
}

pub(super) fn terminal(
    runtime: &Handle,
    plan: bas::authentication::Invocation,
) -> impl Future<Output = Result<i32, String>> + use<> {
    let task = AbortOnDropHandle::new(runtime.spawn(async move {
        async {
            let invocation =
                super::terminal::Invocation::from_plan(plan).map_err(|error| error.to_string())?;
            let login = super::terminal::launch(invocation).map_err(|error| error.to_string())?;
            login.complete_code().await
        }
        .await
    }));
    async move {
        task.await.unwrap_or_else(|error| {
            tracing::error!(%error, "Agent request task failed");
            Err(tr!("auth-terminal-ended").into())
        })
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
