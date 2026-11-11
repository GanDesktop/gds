mod form;
use super::model::Intent;
use form::Form;

use crate::shared::i18n;
use gds::service::task_manager::Scope;
use gds::service::task_manager::Task as IoTask;
use gds::{
    service::remote::{Address, ConnectOptions, Credentials, HostKeyChallenge, Session},
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
    rc::Rc,
};
use tokio::sync::mpsc;

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
pub(crate) struct Pending {
    pub tab: u64,
    pub address: Address,
    pub intent: Intent,
}

pub(crate) struct Connector {
    tasks: Scope,
    sessions: HashMap<String, Connection>,
    task: Option<Task<()>>,
    challenges: Option<Task<()>>,
    pub connecting: bool,
    pending: Option<Pending>,
}
impl EventEmitter<Connected> for Connector {}
impl EventEmitter<Failed> for Connector {}

impl Connector {
    pub(crate) fn new(tasks: Scope) -> Self {
        Self {
            tasks,
            sessions: HashMap::new(),
            task: None,
            challenges: None,
            connecting: false,
            pending: None,
        }
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
        self.pending.as_ref()
    }

    pub(crate) fn prompt(
        &mut self,
        address: Option<Address>,
        intent: Intent,
        tab: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.connecting {
            notifications::info(i18n::text("remote-task-running", cx), cx);
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
                el! { .DialogActions::default {
                    primary_label: $i18n::text(if intent == Intent::Add { "remote-add" } else { "remote-connect-action" }, cx);
                    primary_icon: $(if intent == Intent::Add { IconName::Plus } else { IconName::Plug });
                } }.into_any_element()
            } else {
                el! { .DialogFooter::default [DialogCommand::Cancel.DialogDecision::new] }.into_any_element()
            };
            el! { dialog {
                confirm; width: 480.0.px;
                title: $i18n::text(if intent == Intent::Add { "remote-add-location" } else { "remote-connect" }, cx);
                footer: footer;
                on_ok: $move |_, window, cx| {
                    let request = submit_form.read(cx).request(cx);
                    match request {
                        Ok(request) => {
                            submitted.set(true);
                            let _ = connector.update(cx, |this, cx| {
                                this.begin(
                                    request.address,
                                    request.credentials,
                                    intent,
                                    tab,
                                    window,
                                    cx,
                                )
                            });
                            true
                        }
                        Err(error) => {
                            notifications::error(error, cx);
                            false
                        }
                    }
                };
                on_close: $move |_, _, cx| {
                    if !closed.get() {
                        let _ = close_connector.update(cx, |_, cx| cx.emit(Failed { tab, intent }));
                    }
                };
            } [form] }
        });
        window.on_next_frame(move |window, cx| {
            focus.update(cx, |form, cx| form.focus(window, cx));
        });
    }

    fn begin(
        &mut self,
        address: Address,
        credentials: Credentials,
        intent: Intent,
        tab: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dirs) = directories::BaseDirs::new() else {
            notifications::error(i18n::text("remote-state-unavailable", cx), cx);
            cx.emit(Failed { tab, intent });
            return;
        };
        let known_hosts = dirs
            .state_dir()
            .unwrap_or(dirs.data_local_dir())
            .join("gds/files/known_hosts");
        let (sender, mut receiver) = mpsc::channel::<HostKeyChallenge>(1);
        let target = window.window_handle();
        self.connecting = true;
        self.pending = Some(Pending {
            tab,
            address: address.clone(),
            intent,
        });
        self.challenges = Some(cx.spawn(async move |_, cx| {
            while let Some(challenge) = receiver.recv().await {
                let _ = target.update(cx, |_, window, cx| Self::host_key(challenge, window, cx));
            }
        }));
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
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| v.map_err(|e| e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.connecting = false;
                this.pending = None;
                match result {
                    Ok(session) => {
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
                    Err(error) => {
                        notifications::error(error, cx);
                        cx.emit(Failed { tab, intent });
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn host_key(challenge: HostKeyChallenge, window: &mut Window, cx: &mut App) {
        let title = i18n::text(
            if challenge.changed {
                "remote-host-changed"
            } else {
                "remote-host-new"
            },
            cx,
        );
        let message = format!(
            "{}:{}\n{}",
            challenge.host, challenge.port, challenge.fingerprint
        );
        let answer = Rc::new(RefCell::new(Some(challenge.answer)));
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let accept = answer.clone();
            let close = answer.clone();
            let footer = el! { .DialogActions::default {
                primary_label: $i18n::text("remote-host-trust", cx);
            } };
            el! { dialog {
                confirm; width: 520.0.px; title: ~title; description: ~message; footer: footer;
                on_ok: $move |_, _, _| {
                    if let Some(reply) = accept.borrow_mut().take() {
                        let _ = reply.send(true);
                    }
                    true
                };
                on_close: $move |_, _, _| {
                    if let Some(reply) = close.borrow_mut().take() {
                        let _ = reply.send(false);
                    }
                };
            } }
        });
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending.take() {
            cx.emit(Failed {
                tab: pending.tab,
                intent: pending.intent,
            });
        }
        self.task = None;
        self.challenges = None;
        self.connecting = false;
        cx.notify();
    }

    pub(crate) fn disconnect(&mut self, address: &Address, cx: &mut Context<Self>) {
        if let Some(connection) = self.sessions.remove(&address.connection_key()) {
            let session = connection.session;
            let work = IoTask::cancel_on_drop(
                self.tasks
                    .spawn_named("Close remote session", async move { session.close().await }),
            );
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
