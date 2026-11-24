use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gds::ui::ToggleSwitchProps;
use gds::ui::button::{ButtonProps, ButtonSize, ButtonVariant};
use gds::ui::field::FieldLabelProps;
use gds::ui::settings::{
    SettingActionsProps, SettingControlProps, SettingLabelProps, SettingRowProps,
};
use gds::{app::Gds, i18n, service::task_manager};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, Owner as UiOwner, ReadSignal, RenderEffect, RwSignal, on_cleanup_local,
    spawn_local,
};
use zgui::view::install_stylesheet;
use zgui::{css, view};
use zgui_ui_primitives::Binding;

use super::{Preferences, View};
use crate::entities::tags;

const TAGS_SHEET: &str = "gds-files-settings-tags";
const TAGS_CSS: &str = css!(
    r#"
    .gds-settings-tags {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0;
    }

    .gds-settings-tags__title {
        font-size: var(--gds-text-md, 16px);
        font-weight: 600;
    }

    .gds-settings-tags__probe {
        display: flex;
        flex-direction: row;
        align-items: flex-start;
        gap: 12px;
        padding: 12px 0;
    }

    .gds-settings-tags__status {
        flex: 1 1 0%;
        min-width: 0;
        white-space: normal;
        color: var(--gds-muted-foreground);
    }
    "#
);

#[derive(Clone, Debug, PartialEq)]
pub(super) enum State {
    Checking(tags::Commands),
    Ready(tags::Commands),
    Failed {
        commands: tags::Commands,
        error: String,
    },
}

#[derive(Clone)]
pub(super) struct Probe {
    state: RwSignal<State, LocalStorage>,
    refresh: RwSignal<u64, LocalStorage>,
}

impl Probe {
    pub(super) fn new(preferences: ReadSignal<Preferences, LocalStorage>) -> Self {
        Self::new_with(preferences, |commands| async move {
            tags::connect(&commands).await.map(|_| ())
        })
    }

    pub(super) fn new_with<F, Fut>(
        preferences: ReadSignal<Preferences, LocalStorage>,
        probe_fn: F,
    ) -> Self
    where
        F: Fn(tags::Commands) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let initial_commands = preferences.with_untracked(|p| p.tags.commands.clone());
        let state = RwSignal::new_local(State::Checking(initial_commands));
        let refresh = RwSignal::new_local(0_u64);

        let parent_scope = Gds::current().tasks().owner_scope("Settings tags");
        let ui_owner = UiOwner::current();
        let in_flight: Rc<RefCell<Option<task_manager::Owner>>> = Rc::new(RefCell::new(None));
        let generation: Rc<Cell<u64>> = Rc::new(Cell::new(0));
        let last_key: Rc<RefCell<Option<(u64, tags::Commands)>>> = Rc::new(RefCell::new(None));

        let in_flight_cleanup = Rc::clone(&in_flight);
        on_cleanup_local(move || {
            let previous = in_flight_cleanup.borrow_mut().take();
            drop(previous);
        });

        let fx = RenderEffect::new(move |_| {
            let seq = refresh.get();
            let commands = preferences.with(|p| p.tags.commands.clone());

            let unchanged = last_key
                .borrow()
                .as_ref()
                .is_some_and(|(prev_seq, prev_cmds)| *prev_seq == seq && *prev_cmds == commands);
            if unchanged {
                return;
            }
            *last_key.borrow_mut() = Some((seq, commands.clone()));

            let probe_gen = generation.get().wrapping_add(1);
            generation.set(probe_gen);

            // Replacing `in_flight` drops the prior `task_manager::Owner`, cancelling its token
            // and aborting any stale `spawn_cancellable_named` probe immediately.
            let attempt_owner = parent_scope.named_child("Check tag backend");
            let attempt_scope = (*attempt_owner).clone();
            let cancelled_previous = in_flight.borrow_mut().replace(attempt_owner);
            drop(cancelled_previous);

            state.set(State::Checking(commands.clone()));

            let future = probe_fn(commands.clone());
            let task = attempt_scope.spawn_cancellable_named("Check tag backend", future);
            let generation_check = Rc::clone(&generation);
            let in_flight_done = Rc::clone(&in_flight);
            let probe_commands = commands;

            let deliver = move || {
                spawn_local(async move {
                    let result = task.await;
                    if generation_check.get() != probe_gen {
                        return;
                    }
                    if preferences.with_untracked(|p| p.tags.commands != probe_commands) {
                        return;
                    }
                    let finished = in_flight_done.borrow_mut().take();
                    drop(finished);

                    let next = match result {
                        Ok(Ok(())) => State::Ready(probe_commands),
                        Ok(Err(error)) => State::Failed {
                            commands: probe_commands,
                            error,
                        },
                        Err(error) => State::Failed {
                            commands: probe_commands,
                            error: error.to_string(),
                        },
                    };
                    state.set(next);
                });
            };

            if let Some(ref owner) = ui_owner {
                owner.with(deliver);
            } else {
                Gds::current().with(deliver);
            }
        });
        on_cleanup_local(move || drop(fx));

        Self { state, refresh }
    }

    fn ready(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with(|p| p.tags.commands.clone());
        self.state
            .with(|state| matches!(state, State::Ready(checked) if *checked == current))
    }

    fn ready_untracked(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with_untracked(|p| p.tags.commands.clone());
        self.state
            .with_untracked(|state| matches!(state, State::Ready(checked) if *checked == current))
    }

    fn checking(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with(|p| p.tags.commands.clone());
        self.state
            .with(|state| matches!(state, State::Checking(checked) if *checked == current))
    }

    #[cfg(test)]
    fn checking_untracked(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with_untracked(|p| p.tags.commands.clone());
        self.state.with_untracked(
            |state| matches!(state, State::Checking(checked) if *checked == current),
        )
    }

    fn status(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> String {
        let current = preferences.with(|p| p.tags.commands.clone());
        self.state.with(|state| match state {
            State::Checking(checked) if *checked == current => i18n::text("tags-checking"),
            State::Ready(checked) if *checked == current => i18n::text("tags-ready"),
            State::Failed { commands, error } if *commands == current => error.clone(),
            _ => i18n::text("tags-unavailable"),
        })
    }
}

impl View {
    pub(super) fn tags_section(&self) -> AnyView {
        install_stylesheet(TAGS_SHEET, TAGS_CSS);
        let probe = self.tags.clone();
        let probe_ready = self.tags.clone();
        let probe_checking = self.tags.clone();
        let probe_status = self.tags.clone();
        let probe_refresh = self.tags.refresh;
        let preferences = self.preferences;
        let view = self.clone();
        let title_ref = NodeRef::new();

        let enabled = Binding::controlled(
            Signal::derive_local(move || preferences.with(|p| p.tags.enabled)),
            move |enabled: bool| {
                if !enabled || probe.ready_untracked(preferences) {
                    view.apply(|p| p.tags.enabled = enabled);
                }
            },
        );
        let switch_disabled = Signal::derive_local(move || {
            !preferences.with(|p| p.tags.enabled) && !probe_ready.ready(preferences)
        });
        let reset = self.reset(
            "tags-enabled",
            |p| p.tags.enabled,
            |p, v| p.tags.enabled = v,
        );
        let commands = self
            .subform(|p| p.tags.commands.clone(), |p, v| p.tags.commands = v)
            .view();

        AnyView::new(view! {
            column(class = "gds-settings-tags") {
                text(class = "gds-settings-tags__title") {{move || i18n::text("tags-title")}}
                SettingRow {
                    SettingLabel {
                        FieldLabel(node_ref = title_ref) {{move || i18n::text("tags-enabled")}}
                    }
                    SettingActions {
                        {reset}
                        SettingControl {
                            ToggleSwitch(
                                checked = enabled,
                                disabled = switch_disabled,
                                labelled_by = Some(title_ref),
                            )
                        }
                    }
                }
                row(class = "gds-settings-tags__probe") {
                    text(class = "gds-settings-tags__status") {{move || probe_status.status(preferences)}}
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        disabled = Signal::derive_local(move || probe_checking.checking(preferences)),
                        tooltip = Signal::derive_local(|| i18n::text("tags-detect")),
                        on:click = move |_| probe_refresh.update(|n| *n = n.wrapping_add(1)),
                    ) {{move || i18n::text("tags-detect")}}
                }
                {commands}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use gds::app::Gds;
    use gds::service::task_manager::Supervisor;
    use tokio::sync::oneshot;
    use zgui::prelude::*;
    use zgui::reactive::{Mounted, RwSignal, TestWaker, flush, install, set_frame_waker};

    use super::{Probe, State};
    use crate::entities::preferences::Preferences;

    type BoxProbeFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

    struct DropSignal {
        dropped: Arc<AtomicUsize>,
        tx: Option<oneshot::Sender<()>>,
    }

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }
    }

    #[test]
    fn changing_tag_commands_cancels_stale_inflight_probe_and_ignores_unrelated_edits() {
        let _ = install();
        set_frame_waker(Arc::new(TestWaker::default()));
        let supervisor = Supervisor::new(false).expect("start test supervisor");

        let started = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let (first_started_tx, first_started_rx) = oneshot::channel();
        let (first_dropped_tx, first_dropped_rx) = oneshot::channel();
        let (second_release_tx, second_release_rx) = oneshot::channel::<()>();
        let (second_done_tx, second_done_rx) = oneshot::channel();

        let dropped_first = Arc::clone(&dropped);
        let first_fut: BoxProbeFuture = Box::pin(async move {
            let _guard = DropSignal {
                dropped: dropped_first,
                tx: Some(first_dropped_tx),
            };
            let _ = first_started_tx.send(());
            std::future::pending::<Result<(), String>>().await
        });

        let second_fut: BoxProbeFuture = Box::pin(async move {
            let _ = second_release_rx.await;
            let _ = second_done_tx.send(());
            Ok(())
        });

        let queue: Arc<Mutex<VecDeque<BoxProbeFuture>>> =
            Arc::new(Mutex::new(VecDeque::from([first_fut, second_fut])));
        let started_probe = Arc::clone(&started);
        let queue_probe = Arc::clone(&queue);

        let root = Mounted::new();
        root.with(|| {
            let _gds = Gds::install(supervisor.scope());
            let prefs = RwSignal::new_local(Preferences::default());
            let probe = Probe::new_with(prefs.read_only(), move |_cmds| {
                started_probe.fetch_add(1, Ordering::SeqCst);
                queue_probe
                    .lock()
                    .expect("probe queue")
                    .pop_front()
                    .expect("scheduled probe future")
            });

            flush();
            supervisor
                .block_on(first_started_rx)
                .expect("supervisor active")
                .expect("first probe started");
            assert_eq!(started.load(Ordering::SeqCst), 1);
            assert!(probe.checking_untracked(prefs.read_only()));

            // Unrelated preference edits MUST NOT restart the tag backend probe.
            prefs.update(|p| p.appearance.rounded_corners = true);
            flush();
            assert_eq!(started.load(Ordering::SeqCst), 1);

            // Editing `tags.commands` MUST cancel the first in-flight probe immediately.
            prefs.update(|p| {
                p.tags.commands.control = Some(PathBuf::from("/usr/bin/balooctl6"));
            });
            flush();

            supervisor
                .block_on(first_dropped_rx)
                .expect("supervisor active")
                .expect("first probe cancelled and dropped");
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
            assert_eq!(started.load(Ordering::SeqCst), 2);

            second_release_tx.send(()).expect("release second probe");
            supervisor
                .block_on(second_done_rx)
                .expect("supervisor active")
                .expect("second probe finished");
            // Yield once so Tokio completes the wrapper future and wakes the local UI task.
            supervisor
                .block_on(tokio::task::yield_now())
                .expect("supervisor yield");
            flush();
            flush();

            let expected_cmds = prefs.with_untracked(|p| p.tags.commands.clone());
            assert_eq!(probe.state.get_untracked(), State::Ready(expected_cmds));
            assert!(probe.ready_untracked(prefs.read_only()));
        });
        root.unmount();
        supervisor.shutdown();
    }
}
