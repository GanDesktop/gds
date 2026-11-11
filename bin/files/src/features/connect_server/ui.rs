mod form;
use super::model::{Intent, Request};
use form::Form;

use crate::shared::i18n;
use gds::service::task_manager::Scope;
use gds::service::task_manager::Task as IoTask;
use gds::{
    service::remote::{Address, ConnectOptions, HostKeyChallenge, Session},
    t,
    ui::{
        DialogActions,
        dialog::{DialogCommand, DialogDecision, DialogFooter},
        notifications,
    },
};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::{component::WindowExt, *};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Deref,
    rc::Rc,
};
use tokio::sync::mpsc;

const DIALOG_WIDTH: f32 = 480.0;
const HOST_KEY_DIALOG_WIDTH: f32 = 520.0;

pub(crate) struct Connected {
    pub tab: u64,
    pub address: Address,
    pub intent: Intent,
}

pub(crate) struct Failed {
    pub tab: u64,
    pub intent: Intent,
}

struct Connection {
    address: Address,
    session: Session,
}

#[derive(Clone)]
pub(crate) struct Pending {
    pub tab: u64,
    pub address: Address,
    pub intent: Intent,
}

struct Attempt {
    request: Request,
    pending: Pending,
}

enum State {
    Idle,
    Connecting {
        pending: Pending,
        task: Task<()>,
        challenges: Task<()>,
    },
}

#[doc(hidden)]
pub(crate) struct Status {
    pub connecting: bool,
}

const IDLE_STATUS: Status = Status { connecting: false };
const CONNECTING_STATUS: Status = Status { connecting: true };

pub(crate) struct Connector {
    tasks: Scope,
    sessions: HashMap<String, Connection>,
    state: State,
}

impl Deref for Connector {
    type Target = Status;

    fn deref(&self) -> &Self::Target {
        if self.is_connecting() {
            &CONNECTING_STATUS
        } else {
            &IDLE_STATUS
        }
    }
}

impl EventEmitter<Connected> for Connector {}
impl EventEmitter<Failed> for Connector {}

impl Connector {
    pub(crate) fn new(tasks: Scope) -> Self {
        Self {
            tasks,
            sessions: HashMap::new(),
            state: State::Idle,
        }
    }

    pub(crate) fn is_connecting(&self) -> bool {
        matches!(self.state, State::Connecting { .. })
    }

    pub(crate) fn session(&self, address: &Address) -> Option<Session> {
        self.sessions
            .get(&address.connection_key())
            .map(|c| c.session.clone())
    }

    pub(crate) fn connections(&self) -> Vec<Address> {
        let mut addresses = self
            .sessions
            .values()
            .map(|c| c.address.clone())
            .collect::<Vec<_>>();
        addresses.sort_by(|a, b| a.uri().cmp(b.uri()));
        addresses
    }

    pub(crate) fn pending(&self) -> Option<&Pending> {
        match &self.state {
            State::Idle => None,
            State::Connecting { pending, .. } => Some(pending),
        }
    }

    pub(crate) fn prompt(
        &mut self,
        address: Option<Address>,
        intent: Intent,
        tab: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_connecting() {
            notifications::info(t!(@cx, "remote-task-running"), cx);
            cx.emit(Failed { tab, intent });
            return;
        }
        let form = cx.new(|cx| Form::new(address, window, cx));
        let connector = cx.entity().downgrade();
        let started = Rc::new(Cell::new(false));
        let focus = form.clone();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let form = form.clone();
            let connector = connector.clone();
            let submit_form = form.clone();
            let close_connector = connector.clone();
            let submitted = started.clone();
            let closed = started.clone();
            let footer = if form.read(cx).protocol().is_some() {
                let primary_label = if intent == Intent::Add {
                    t!(@cx, "remote-add")
                } else {
                    t!(@cx, "remote-connect-action")
                };
                let primary_icon = if intent == Intent::Add {
                    IconName::Plus
                } else {
                    IconName::Plug
                };
                el! { .DialogActions::default {
                    primary_label: primary_label;
                    primary_icon: primary_icon;
                } }
                .into_any_element()
            } else {
                el! { .DialogFooter::default [DialogCommand::Cancel.DialogDecision::new] }
                    .into_any_element()
            };
            let title = if intent == Intent::Add {
                t!(@cx, "remote-add-location")
            } else {
                t!(@cx, "remote-connect")
            };
            let on_ok = move |_: &ClickEvent, window: &mut Window, cx: &mut App| match submit_form
                .read(cx)
                .request(cx)
            {
                Ok(request) => {
                    submitted.set(true);
                    let pending = Pending {
                        tab,
                        address: request.address.clone(),
                        intent,
                    };
                    let _ = connector.update(cx, |this, cx| {
                        this.begin(Attempt { request, pending }, window, cx)
                    });
                    true
                }
                Err(error) => {
                    notifications::error(i18n::text(error.label(), cx), cx);
                    false
                }
            };
            let on_close = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if !closed.get() {
                    let _ = close_connector.update(cx, |_, cx| cx.emit(Failed { tab, intent }));
                }
            };
            el! { dialog {
                confirm; width: DIALOG_WIDTH.px;
                title: title;
                footer: footer;
                on_ok: on_ok;
                on_close: on_close;
            } [form] }
        });
        window.on_next_frame(move |window, cx| {
            focus.update(cx, |form, cx| form.focus(window, cx));
        });
    }

    fn begin(&mut self, attempt: Attempt, window: &mut Window, cx: &mut Context<Self>) {
        let Attempt { request, pending } = attempt;
        let Request {
            address,
            credentials,
        } = request;
        let tab = pending.tab;
        let intent = pending.intent;
        let Some(dirs) = directories::BaseDirs::new() else {
            notifications::error(t!(@cx, "remote-state-unavailable"), cx);
            cx.emit(Failed { tab, intent });
            return;
        };
        let known_hosts = dirs
            .state_dir()
            .unwrap_or(dirs.data_local_dir())
            .join("gds/files/known_hosts");
        let (sender, mut receiver) = mpsc::channel::<HostKeyChallenge>(1);
        let target = window.window_handle();
        let challenges = cx.spawn(async move |_, cx| {
            while let Some(challenge) = receiver.recv().await {
                let _ = target.update(cx, |_, window, cx| Self::host_key(challenge, window, cx));
            }
        });
        let destination = address.clone();
        let work = IoTask::cancel_on_drop(self.tasks.spawn_cancellable_named(
            "Connect to remote server",
            async move {
                let session = Session::connect(
                    &destination,
                    ConnectOptions {
                        credentials,
                        known_hosts,
                    },
                    sender,
                )
                .await?;
                if intent == Intent::Add {
                    session.list(&destination).await?;
                }
                Ok::<_, gds::service::remote::Error>(session)
            },
        ));
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.state = State::Idle;
                match result {
                    Ok(Ok(session)) => {
                        this.sessions.insert(
                            address.connection_key(),
                            Connection {
                                address: address.clone(),
                                session,
                            },
                        );
                        cx.emit(Connected {
                            tab,
                            address,
                            intent,
                        });
                    }
                    Ok(Err(error)) => {
                        notifications::error(error.to_string(), cx);
                        cx.emit(Failed { tab, intent });
                    }
                    Err(error) => {
                        notifications::error(error.to_string(), cx);
                        cx.emit(Failed { tab, intent });
                    }
                }
                cx.notify();
            });
        });
        self.state = State::Connecting {
            pending,
            task,
            challenges,
        };
        cx.notify();
    }

    fn host_key(challenge: HostKeyChallenge, window: &mut Window, cx: &mut App) {
        let title = if challenge.changed {
            t!(@cx, "remote-host-changed")
        } else {
            t!(@cx, "remote-host-new")
        };
        let message = format!(
            "{}:{}\n{}",
            challenge.host, challenge.port, challenge.fingerprint
        );
        let answer = Rc::new(RefCell::new(Some(challenge.answer)));
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let accept = answer.clone();
            let close = answer.clone();
            let footer = el! { .DialogActions::default {
                primary_label: $(t!(@cx, "remote-host-trust"));
            } };
            let on_ok = move |_: &ClickEvent, _: &mut Window, _: &mut App| {
                if let Some(reply) = accept.borrow_mut().take() {
                    let _ = reply.send(true);
                }
                true
            };
            let on_close = move |_: &ClickEvent, _: &mut Window, _: &mut App| {
                if let Some(reply) = close.borrow_mut().take() {
                    let _ = reply.send(false);
                }
            };
            el! { dialog {
                confirm; width: HOST_KEY_DIALOG_WIDTH.px; title: ~title; description: ~message; footer: footer;
                on_ok: on_ok;
                on_close: on_close;
            } }
        });
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        if let State::Connecting {
            pending,
            task,
            challenges,
        } = std::mem::replace(&mut self.state, State::Idle)
        {
            drop(task);
            drop(challenges);
            cx.emit(Failed {
                tab: pending.tab,
                intent: pending.intent,
            });
        }
        cx.notify();
    }

    pub(crate) fn disconnect(&mut self, address: &Address, cx: &mut Context<Self>) {
        if let Some(connection) = self.sessions.remove(&address.connection_key()) {
            let session = connection.session;
            let work = self
                .tasks
                .spawn_named("Close remote session", async move { session.close().await });
            cx.spawn(async move |_, cx| match work.await {
                Ok(Ok(())) => (),
                Ok(Err(error)) => {
                    let _ = cx.update(|cx| notifications::error(error.to_string(), cx));
                }
                Err(error) => {
                    let _ = cx.update(|cx| notifications::error(error.to_string(), cx));
                }
            })
            .detach();
        }
        cx.notify();
    }
}
