mod form;
use super::model::{Intent, Request};
use anti_webui::prelude::*;
use form::Form;
use gds::{
    i18n,
    service::{
        remote::{Address, ConnectOptions, HostKeyChallenge, Session},
        task_manager::{Owner as TaskOwner, Scope},
    },
    ui::{DialogActions, dialog::Dialog},
};
use gds_uim::el;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use tokio::sync::{mpsc, oneshot};

type Completion = Rc<dyn Fn(Result<Address, String>)>;

#[derive(Clone)]
struct Attempt {
    intent: Intent,
    completed: Completion,
}
#[derive(Clone)]
struct Challenge {
    changed: bool,
    description: String,
    answer: Rc<RefCell<Option<oneshot::Sender<bool>>>>,
}
impl Challenge {
    fn answer(&self, trusted: bool) {
        if let Some(answer) = self.answer.borrow_mut().take() {
            let _ = answer.send(trusted);
        }
    }
}
#[derive(Clone)]
enum DialogState {
    Closed,
    Form {
        form: Form,
        attempt: Attempt,
        error: Option<String>,
    },
    Connecting {
        address: Address,
        attempt: Attempt,
        tasks: Rc<TaskOwner>,
        challenge: Option<Challenge>,
    },
}
struct Connection {
    address: Address,
    session: Session,
}
struct State {
    tasks: Scope,
    owner: Owner,
    dialog: Signal<DialogState>,
    sessions: RefCell<HashMap<String, Connection>>,
}
#[derive(Clone)]
pub(crate) struct Connector(Rc<State>);

impl Connector {
    pub fn new(tasks: Scope) -> Self {
        Self(Rc::new(State {
            tasks,
            owner: Owner::current().expect("connector belongs to a window"),
            dialog: signal(DialogState::Closed),
            sessions: RefCell::new(HashMap::new()),
        }))
    }
    pub fn is_connecting(&self) -> bool {
        matches!(self.0.dialog.get(), DialogState::Connecting { .. })
    }
    pub fn pending(&self) -> Option<(Address, Intent)> {
        match self.0.dialog.get() {
            DialogState::Connecting {
                address, attempt, ..
            } => Some((address, attempt.intent)),
            _ => None,
        }
    }
    pub fn session(&self, address: &Address) -> Option<Session> {
        self.0
            .sessions
            .borrow()
            .get(&address.connection_key())
            .map(|connection| connection.session.clone())
    }
    pub fn connections(&self) -> Vec<Address> {
        let mut addresses = self
            .0
            .sessions
            .borrow()
            .values()
            .map(|connection| connection.address.clone())
            .collect::<Vec<_>>();
        addresses.sort_by(|a, b| a.uri().cmp(b.uri()));
        addresses
    }
    pub fn prompt(
        &self,
        address: Option<Address>,
        intent: Intent,
        completed: impl Fn(Result<Address, String>) + 'static,
    ) {
        if !matches!(self.0.dialog.get_untracked(), DialogState::Closed) {
            completed(Err(i18n::text("remote-task-running")));
            return;
        }
        self.0.owner.with(|| {
            let form = Form::new(address);
            form.focus();
            self.0.dialog.set(DialogState::Form {
                form,
                attempt: Attempt {
                    intent,
                    completed: Rc::new(completed),
                },
                error: None,
            })
        });
    }
    pub fn cancel(&self) {
        let current = self.0.dialog.get_untracked();
        self.0.dialog.set(DialogState::Closed);
        let attempt = match current {
            DialogState::Closed => return,
            DialogState::Form { attempt, .. } => attempt,
            DialogState::Connecting {
                tasks,
                challenge,
                attempt,
                ..
            } => {
                if let Some(challenge) = challenge {
                    challenge.answer(false);
                }
                tasks.cancel();
                attempt
            }
        };
        (attempt.completed)(Err(i18n::text("remote-cancelled")));
    }
    fn submit(&self) {
        let DialogState::Form { form, attempt, .. } = self.0.dialog.get_untracked() else {
            return;
        };
        match form.request() {
            Ok(request) => self.begin(request, attempt),
            Err(error) => self.0.dialog.set(DialogState::Form {
                form,
                attempt,
                error: Some(i18n::text(error.label())),
            }),
        }
    }
    fn begin(&self, request: Request, attempt: Attempt) {
        let Some(dirs) = directories::BaseDirs::new() else {
            (attempt.completed)(Err(i18n::text("remote-state-unavailable")));
            self.0.dialog.set(DialogState::Closed);
            return;
        };
        let known_hosts = dirs
            .state_dir()
            .unwrap_or(dirs.data_local_dir())
            .join("gds/files/known_hosts");
        let Request {
            address,
            credentials,
        } = request;
        let tasks = Rc::new(self.0.tasks.named_child("Remote connection"));
        let weak = Rc::downgrade(&self.0);
        let challenge_tasks = tasks.clone();
        let challenges = self.0.owner.with(|| {
            ui_channel(move |challenge: HostKeyChallenge| {
                let Some(state) = weak.upgrade() else { return };
                state.dialog.update(|dialog| {
                    if let DialogState::Connecting {
                        tasks,
                        challenge: pending,
                        ..
                    } = dialog
                        && Rc::ptr_eq(tasks, &challenge_tasks)
                    {
                        *pending = Some(Challenge {
                            changed: challenge.changed,
                            description: format!(
                                "{}:{}\n{}",
                                challenge.host, challenge.port, challenge.fingerprint
                            ),
                            answer: Rc::new(RefCell::new(Some(challenge.answer))),
                        });
                    }
                });
            })
        });
        let (sender, mut receiver) = mpsc::channel(1);
        let cancel = tasks.token();
        tasks.spawn_named("Verify remote host", async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    challenge = receiver.recv() => match challenge {
                        Some(challenge) => challenges.send(challenge),
                        None => break,
                    },
                }
            }
        });
        let destination = address.clone();
        let intent = attempt.intent;
        let work = tasks.spawn_cancellable_named("Connect to remote server", async move {
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
        });
        self.0.dialog.set(DialogState::Connecting {
            address: address.clone(),
            attempt: attempt.clone(),
            tasks: tasks.clone(),
            challenge: None,
        });
        let weak = Rc::downgrade(&self.0);
        self.0.owner.with(|| {
            let (delivery, done) = Task::pending();
            work.on_complete(&tasks, move |result| {
                done.complete(
                    result
                        .map_err(|error| error.to_string())
                        .and_then(|result| result.map_err(|error| error.to_string())),
                )
            });
            delivery.then(move |result| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let current = state.dialog.get_untracked();
                if !matches!(&current, DialogState::Connecting { tasks: active, .. } if Rc::ptr_eq(active, &tasks)) {
                    return;
                }
                let result = result.map(|session| {
                    state.sessions.borrow_mut().insert(
                        address.connection_key(),
                        Connection {
                            address: address.clone(),
                            session,
                        },
                    );
                    address
                });
                state.dialog.set(DialogState::Closed);
                (attempt.completed)(result);
            });
        });
    }
    pub fn disconnect(
        &self,
        address: Address,
        finished: impl FnOnce(Result<(), String>) + 'static,
    ) {
        let Some(connection) = self
            .0
            .sessions
            .borrow_mut()
            .remove(&address.connection_key())
        else {
            finished(Ok(()));
            return;
        };
        self.0.owner.with(|| {
            let (delivery, done) = Task::pending();
            self.0
                .tasks
                .spawn_named("Close remote session", async move {
                    connection.session.close().await
                })
                .on_complete(&self.0.tasks, move |result| {
                    done.complete(
                        result
                            .map_err(|error| error.to_string())
                            .and_then(|result| result.map_err(|error| error.to_string())),
                    )
                });
            delivery.then(finished);
        });
    }
    pub fn view(&self) -> impl IntoView {
        let shown = self.clone();
        let cancel = self.clone();
        let content = self.clone();
        Dialog::show(
            move || !matches!(shown.0.dialog.get(), DialogState::Closed),
            move || cancel.cancel(),
            move || content.dialog(),
        )
    }
    fn dialog(&self) -> View {
        let cancel = self.clone();
        match self.0.dialog.get() {
            DialogState::Closed => div().into_view(),
            DialogState::Form {
                form,
                attempt,
                error,
            } => {
                let submit = self.clone();
                let enter = self.clone();
                let ready = form.clone();
                let (title, label) = match attempt.intent {
                    Intent::Add => ("remote-add-location", "remote-add"),
                    Intent::Open => ("remote-connect", "remote-connect-action"),
                };
                let actions = el! { .DialogActions::default.[
                    $(i18n::text("dialog-cancel")).button.{on_click: move _ => $(cancel.cancel())},
                    $(show(move || ready.ready(), move || {
                        let submit = submit.clone();
                        el! { $(i18n::text(label)).primary_button.{
                            on_click: move _ => $(submit.submit());
                        } }
                    })),
                ] };
                el! { .Dialog::default.{
                    title: $(i18n::text(title)); w: 480.0_f32; actions: actions;
                }.[
                    $(form.view(move || enter.submit())),
                    if let Some(error) = error [
                        error.text.{text_color: $(theme().danger)},
                    ],
                ] }
                .into_view()
            }
            DialogState::Connecting {
                address,
                challenge: None,
                ..
            } => {
                let actions = el! { .DialogActions::default.[
                    $(i18n::text("dialog-cancel")).button.{on_click: move _ => $(cancel.cancel())},
                ] };
                el! { .Dialog::default.{title: $(i18n::text("remote-connect")); actions: actions}.[
                    $(address.to_string()).text,
                    $(i18n::text("remote-connecting")).text,
                ] }
                .into_view()
            }
            DialogState::Connecting {
                challenge: Some(challenge),
                ..
            } => {
                let trust = challenge.clone();
                let connector = self.clone();
                let title = if challenge.changed {
                    "remote-host-changed"
                } else {
                    "remote-host-new"
                };
                let actions = el! { .DialogActions::default.[
                        $(i18n::text("dialog-cancel")).button.{on_click: move _ => $(cancel.cancel())},
                        $(i18n::text("remote-host-trust")).primary_button.{on_click: move _ => ${
                            trust.answer(true);
                            connector.0.dialog.update(|dialog| {
                                if let DialogState::Connecting { challenge, .. } = dialog {
                                    *challenge = None;
                                }
                            });
                        }},
                ] };
                el! { .Dialog::default.{title: $(i18n::text(title)); w: 520.0_f32; actions: actions}.[
                    challenge->description.text,
                ] }.into_view()
            }
        }
    }
}
