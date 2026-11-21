mod form;

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use form::Form;
use gds::{
    app::Gds,
    i18n,
    service::{
        remote::{Address, ConnectOptions, HostKeyChallenge, Session},
        task_manager::{Owner as TaskOwner, Scope},
    },
    ui::{
        IconName,
        button::{ButtonProps, ButtonVariant},
        dialog::{
            DialogActionsProps, DialogCommand, DialogDecision, DialogDescriptionProps,
            DialogFooterProps, DialogHeaderProps, DialogTitleProps, ModalDialogProps,
            handle_dialog_key_down,
        },
    },
};
use tokio::sync::{mpsc, oneshot};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, Owner, RenderEffect, RwSignal, Signal, UnsyncCallback, on_cleanup_local,
};
use zgui::view::{
    EventCx, ListenerGuard, ListenerOptions, current_host, events, install_stylesheet,
};
use zgui::{css, view};
use zgui_ui::overlay::OverlayState;
use zgui_ui_primitives::Binding;

use super::model::{Intent, Request};

type Completion = Rc<dyn Fn(Result<Address, String>)>;

const DIALOG_SHEET: &str = "gds-files-connect-dialog";
const DIALOG_CSS: &str = css!(
    r#"
    .gds-connect-dialog,
    .zui-dialog.gds-dialog-content.gds-connect-dialog {
        width: 480px;
        max-width: 92vw;
    }

    .gds-connect-dialog[data-host-key="true"],
    .zui-dialog.gds-dialog-content.gds-connect-dialog[data-host-key="true"] {
        width: 520px;
        max-width: 92vw;
    }

    .gds-connect-dialog__key-scope {
        width: 100%;
        min-width: 0;
    }

    .gds-connect-dialog__error {
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-error);
    }

    .gds-connect-dialog__body {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 100%;
        min-width: 0;
    }

    .gds-connect-dialog__host-key {
        white-space: pre-wrap;
        word-break: break-all;
        font-family: var(--gds-font-mono, monospace);
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
    }
    "#
);

fn install_connect_dialog_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(DIALOG_SHEET, DIALOG_CSS);
    }
}

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
        let sender = self.answer.borrow_mut().take();
        if let Some(answer) = sender {
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

impl DialogState {
    fn is_modal_open(&self) -> bool {
        matches!(
            self,
            Self::Form { .. }
                | Self::Connecting {
                    challenge: Some(_),
                    ..
                }
        )
    }

    fn is_host_key(&self) -> bool {
        matches!(
            self,
            Self::Connecting {
                challenge: Some(_),
                ..
            }
        )
    }
}

struct Connection {
    address: Address,
    session: Session,
}

struct State {
    tasks: Scope,
    owner: Owner,
    dialog: RwSignal<DialogState, LocalStorage>,
    departing: RefCell<Option<DialogState>>,
    sessions: RefCell<HashMap<String, Connection>>,
    sessions_revision: RwSignal<u64, LocalStorage>,
}

#[derive(Clone)]
pub(crate) struct Connector(Rc<State>);

impl Connector {
    pub fn new(tasks: Scope) -> Self {
        Self(Rc::new(State {
            tasks,
            owner: Owner::current().expect("connector belongs to a window"),
            dialog: RwSignal::new_local(DialogState::Closed),
            departing: RefCell::new(None),
            sessions: RefCell::new(HashMap::new()),
            sessions_revision: RwSignal::new_local(0),
        }))
    }

    fn bump_sessions(&self) {
        self.0
            .sessions_revision
            .update(|rev| *rev = rev.wrapping_add(1));
    }

    fn set_dialog(&self, next: DialogState) {
        let current = self.0.dialog.get_untracked();
        if current.is_modal_open() {
            *self.0.departing.borrow_mut() = Some(current);
        }
        if next.is_modal_open() {
            *self.0.departing.borrow_mut() = Some(next.clone());
        }
        self.0.dialog.set(next);
    }

    pub fn is_connecting(&self) -> bool {
        matches!(self.0.dialog.get(), DialogState::Connecting { .. })
    }

    pub fn is_connecting_untracked(&self) -> bool {
        matches!(
            self.0.dialog.get_untracked(),
            DialogState::Connecting { .. }
        )
    }

    pub fn pending(&self) -> Option<(Address, Intent)> {
        match self.0.dialog.get() {
            DialogState::Connecting {
                address, attempt, ..
            } => Some((address, attempt.intent)),
            _ => None,
        }
    }

    pub fn pending_untracked(&self) -> Option<(Address, Intent)> {
        match self.0.dialog.get_untracked() {
            DialogState::Connecting {
                address, attempt, ..
            } => Some((address, attempt.intent)),
            _ => None,
        }
    }

    pub fn session(&self, address: &Address) -> Option<Session> {
        let _ = self.0.sessions_revision.get();
        self.0
            .sessions
            .borrow()
            .get(&address.connection_key())
            .map(|connection| connection.session.clone())
    }

    pub fn connections(&self) -> Vec<Address> {
        let _ = self.0.sessions_revision.get();
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
            completed(Err(i18n::text_untracked("remote-task-running")));
            return;
        }
        self.0.owner.with(|| {
            let form = Form::new(address);
            self.set_dialog(DialogState::Form {
                form,
                attempt: Attempt {
                    intent,
                    completed: Rc::new(completed),
                },
                error: None,
            });
        });
    }

    pub fn cancel(&self) {
        let current = self.0.dialog.get_untracked();
        self.set_dialog(DialogState::Closed);
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
        (attempt.completed)(Err(String::new()));
    }

    fn confirm_action(&self) {
        match self.0.dialog.get_untracked() {
            DialogState::Form { .. } => self.submit(),
            DialogState::Connecting {
                challenge: Some(challenge),
                ..
            } => {
                challenge.answer(true);
                let current = self.0.dialog.get_untracked();
                if current.is_modal_open() {
                    *self.0.departing.borrow_mut() = Some(current);
                }
                self.0.dialog.update(|dialog| {
                    if let DialogState::Connecting { challenge, .. } = dialog {
                        *challenge = None;
                    }
                });
            }
            DialogState::Closed
            | DialogState::Connecting {
                challenge: None, ..
            } => {}
        }
    }

    fn submit(&self) {
        let DialogState::Form { form, attempt, .. } = self.0.dialog.get_untracked() else {
            return;
        };
        match form.request() {
            Ok(request) => self.begin(request, attempt),
            Err(error) => self.set_dialog(DialogState::Form {
                form,
                attempt,
                error: Some(i18n::text_untracked(error.label())),
            }),
        }
    }

    fn begin(&self, request: Request, attempt: Attempt) {
        let Some(dirs) = directories::BaseDirs::new() else {
            let err = i18n::text_untracked("remote-state-unavailable");
            self.set_dialog(DialogState::Closed);
            (attempt.completed)(Err(err));
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
        let (sender, receiver) = mpsc::channel::<HostKeyChallenge>(1);
        self.0.owner.with(|| {
            zgui::tokio::spawn_receiver(receiver, move |challenge| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let incoming = Challenge {
                    changed: challenge.changed,
                    description: format!(
                        "{}:{}\n{}",
                        challenge.host, challenge.port, challenge.fingerprint
                    ),
                    answer: Rc::new(RefCell::new(Some(challenge.answer))),
                };
                let mut next_snapshot = None;
                state.dialog.update(|dialog| {
                    if let DialogState::Connecting {
                        tasks,
                        challenge: pending,
                        ..
                    } = dialog
                        && Rc::ptr_eq(tasks, &challenge_tasks)
                    {
                        *pending = Some(incoming);
                        next_snapshot = Some(dialog.clone());
                    }
                });
                if let Some(snapshot) = next_snapshot {
                    *state.departing.borrow_mut() = Some(snapshot);
                }
            });
        });

        let destination = address.clone();
        let intent = attempt.intent;
        let active_tasks: Scope = (*tasks).clone();
        let weak = Rc::downgrade(&self.0);
        self.set_dialog(DialogState::Connecting {
            address: address.clone(),
            attempt: attempt.clone(),
            tasks: tasks.clone(),
            challenge: None,
        });

        self.0.owner.with(|| {
            Gds::current().tasks().spawn_ui(
                "Connect to remote server",
                async move {
                    let work = active_tasks.spawn_cancellable_named(
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
                    );
                    work.await
                },
                move |result| {
                    let Some(state) = weak.upgrade() else {
                        return;
                    };
                    let connector = Connector(state);
                    let current = connector.0.dialog.get_untracked();
                    if !matches!(
                        &current,
                        DialogState::Connecting { tasks: active, .. } if Rc::ptr_eq(active, &tasks)
                    ) {
                        return;
                    }
                    let outcome = result
                        .map_err(|error| error.to_string())
                        .and_then(|inner| inner.map_err(|error| error.to_string()))
                        .and_then(|inner| inner.map_err(|error| error.to_string()))
                        .map(|session| {
                            {
                                connector.0.sessions.borrow_mut().insert(
                                    address.connection_key(),
                                    Connection {
                                        address: address.clone(),
                                        session,
                                    },
                                );
                            }
                            connector.bump_sessions();
                            address
                        });
                    connector.set_dialog(DialogState::Closed);
                    (attempt.completed)(outcome);
                },
            );
        });
    }

    pub fn disconnect(
        &self,
        address: Address,
        finished: impl FnOnce(Result<(), String>) + 'static,
    ) {
        let removed = self
            .0
            .sessions
            .borrow_mut()
            .remove(&address.connection_key());
        let Some(connection) = removed else {
            finished(Ok(()));
            return;
        };
        self.bump_sessions();
        let finished = RefCell::new(Some(finished));
        self.0.owner.with(|| {
            Gds::current().tasks().spawn_ui(
                "Close remote session",
                async move { connection.session.close().await },
                move |result| {
                    let result = result
                        .map_err(|error| error.to_string())
                        .and_then(|inner| inner.map_err(|error| error.to_string()));
                    let callback = finished.borrow_mut().take();
                    if let Some(callback) = callback {
                        callback(result);
                    }
                },
            );
        });
    }

    pub fn view(&self) -> AnyView {
        install_connect_dialog_stylesheet();
        let dialog_state = self.0.dialog;
        let state_ref = Rc::clone(&self.0);
        let open = Signal::derive_local(move || dialog_state.get().is_modal_open());
        let host_key_state = Rc::clone(&self.0);
        let is_host_key = Signal::derive_local(move || {
            let current = dialog_state.get();
            if current.is_modal_open() {
                current.is_host_key()
            } else {
                host_key_state
                    .departing
                    .borrow()
                    .as_ref()
                    .is_some_and(DialogState::is_host_key)
            }
        });
        let confirm_disabled = Signal::derive_local(move || match dialog_state.get() {
            DialogState::Form { form, .. } => !form.ready(),
            DialogState::Connecting {
                challenge: Some(_), ..
            } => false,
            DialogState::Closed
            | DialogState::Connecting {
                challenge: None, ..
            } => true,
        });

        let decision_connector = self.clone();
        let on_decision = UnsyncCallback::new(move |decision: DialogDecision| match decision {
            DialogDecision::Confirm => decision_connector.confirm_action(),
            DialogDecision::Cancel | DialogDecision::Dismiss => decision_connector.cancel(),
        });

        let modal_decision = on_decision;
        let key_decision = on_decision;
        let connector = self.clone();

        AnyView::new(view! {
            ModalDialog(
                open = Binding::controlled(open, |_| {}),
                disabled = Signal::stored_local(true),
                on_decision = modal_decision,
                class = "gds-connect-dialog",
                attr:data-host-key = move || is_host_key.get().then(|| "true".to_owned()),
            ) {
                {{
                    if let Some(overlay) = OverlayState::current() {
                        let content_node = overlay.content();
                        let guard_slot: Rc<RefCell<Option<ListenerGuard>>> =
                            Rc::new(RefCell::new(None));
                        let guard_watch = Rc::clone(&guard_slot);
                        let attach_fx = RenderEffect::new(move |_| {
                            if content_node.get().is_none() || guard_watch.borrow().is_some() {
                                return;
                            }
                            let guard = content_node.listen(
                                events::KEY_DOWN,
                                ListenerOptions::CAPTURE,
                                move |ev: &mut EventCx<'_, events::KeyDown>| {
                                    handle_dialog_key_down(
                                        confirm_disabled.get_untracked(),
                                        key_decision,
                                        ev,
                                    );
                                },
                            );
                            *guard_watch.borrow_mut() = guard;
                        });
                        on_cleanup_local(move || {
                            drop(attach_fx);
                            drop(guard_slot.borrow_mut().take());
                        });
                    }
                    view! {
                        column(class = "gds-connect-dialog__key-scope") {
                            {render_dialog(connector.clone(), Rc::clone(&state_ref), on_decision)}
                        }
                    }
                }}
            }
        })
    }
}

fn render_dialog(
    connector: Connector,
    state_ref: Rc<State>,
    on_decision: UnsyncCallback<DialogDecision>,
) -> AnyView {
    let dialog_state = connector.0.dialog;
    AnyView::new(move || {
        let current = dialog_state.get();
        let is_live = current.is_modal_open();
        let effective = if is_live {
            Some(current)
        } else {
            state_ref.departing.borrow().clone()
        };

        match effective {
            None
            | Some(
                DialogState::Closed
                | DialogState::Connecting {
                    challenge: None, ..
                },
            ) => AnyView::new(view! { box() }),
            Some(DialogState::Form {
                form,
                attempt,
                error,
            }) => {
                let enter = connector.clone();
                let ready_form = form.clone();
                let cancel_connector = connector.clone();
                let (title, label, primary_icon) = match attempt.intent {
                    Intent::Add => ("remote-add-location", "remote-add", IconName::Plus),
                    Intent::Open => ("remote-connect", "remote-connect-action", IconName::Plug),
                };
                let cancel_label = Signal::derive_local(|| i18n::text("operation-cancel"));
                AnyView::new(view! {
                    column(class = "gds-connect-dialog__body") {
                        DialogHeader {
                            DialogTitle {{i18n::text(title)}}
                        }
                        {form.view(is_live, move || enter.submit())}
                        {error.map(|message| view! {
                            text(class = "gds-connect-dialog__error") {{message}}
                        }.into_view())}
                        DialogFooter {
                            {move || {
                                if ready_form.ready() {
                                    AnyView::new(view! {
                                        DialogActions(
                                            primary_label = i18n::text(label),
                                            primary_icon = primary_icon,
                                            cancel_label = cancel_label,
                                            on_decision = on_decision,
                                        )
                                    })
                                } else {
                                    let cancel_click = cancel_connector.clone();
                                    AnyView::new(view! {
                                        Button(
                                            variant = ButtonVariant::Outline,
                                            icon = IconName::X,
                                            tooltip = cancel_label,
                                            shortcut = DialogCommand::Cancel.default_stroke(),
                                            on:click = move |_| cancel_click.cancel(),
                                        ) {{move || cancel_label.get()}}
                                    })
                                }
                            }}
                        }
                    }
                })
            }
            Some(DialogState::Connecting {
                challenge: Some(challenge),
                ..
            }) => {
                let title = if challenge.changed {
                    "remote-host-changed"
                } else {
                    "remote-host-new"
                };
                let cancel_label = Signal::derive_local(|| i18n::text("operation-cancel"));
                AnyView::new(view! {
                    column(class = "gds-connect-dialog__body") {
                        DialogHeader {
                            DialogTitle {{i18n::text(title)}}
                            DialogDescription(class = "gds-connect-dialog__host-key") {
                                {challenge.description}
                            }
                        }
                        DialogFooter {
                            DialogActions(
                                primary_label = i18n::text("remote-host-trust"),
                                primary_icon = IconName::Check,
                                cancel_label = cancel_label,
                                on_decision = on_decision,
                            )
                        }
                    }
                })
            }
        }
    })
}
