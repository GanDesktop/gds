//! Source-driven palette refresh owned by the application root.
use super::{Appearance, Error, Prepared};
use crate::{
    app::Gds,
    service::{
        desktop::watch::Sources,
        task_manager::{self, Owner},
    },
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
use tokio::sync::mpsc;

enum ReloadEvent {
    FilesChanged,
    Prepared {
        generation: u64,
        result: Result<Result<Prepared, Error>, task_manager::Error>,
    },
}

pub struct HotReload(Rc<State>);

struct State {
    appearance: RefCell<Appearance>,
    sources: RefCell<Sources>,
    scope: Owner,
    generation: Cell<u64>,
    error: RefCell<Option<String>>,
    events: mpsc::Sender<ReloadEvent>,
}

impl HotReload {
    pub(crate) fn new(appearance: Appearance) -> Self {
        let (notify, mut changes) = mpsc::channel(1);
        let (events_tx, events_rx) = mpsc::channel(16);
        let state = Rc::new(State {
            appearance: RefCell::new(appearance),
            sources: RefCell::new(Sources::new(notify)),
            scope: Gds::current().tasks().scope().named_child("Appearance"),
            generation: Cell::new(0),
            error: RefCell::new(None),
            events: events_tx.clone(),
        });
        state.watch(state.appearance.borrow().source_paths());

        let state_ui = Rc::clone(&state);
        drop(zgui::tokio::spawn_receiver(
            events_rx,
            move |event| match event {
                ReloadEvent::FilesChanged => {
                    if let Err(error) = state_ui.sources.borrow_mut().refresh() {
                        tracing::warn!(%error, "cannot refresh appearance watches");
                    }
                    state_ui.request();
                }
                ReloadEvent::Prepared { generation, result } => {
                    if generation != state_ui.generation.get() {
                        return;
                    }
                    let result = result
                        .map_err(|error| error.to_string())
                        .and_then(|value| value.map_err(|error| error.to_string()));
                    match result {
                        Ok(prepared) => {
                            state_ui.watch(prepared.source_paths());
                            match Gds::current().appearance().theme().install(prepared) {
                                Ok(_) => *state_ui.error.borrow_mut() = None,
                                Err(error) => state_ui.report(error.to_string()),
                            }
                        }
                        Err(error) => state_ui.report(error),
                    }
                }
            },
        ));

        let cancel = state.scope.token();
        drop(
            state
                .scope
                .spawn_named("Watch appearance files", async move {
                    loop {
                        let notified = tokio::select! {
                            _ = cancel.cancelled() => None,
                            value = changes.recv() => value,
                        };
                        let Some(()) = notified else {
                            break;
                        };
                        while changes.try_recv().is_ok() {}
                        match events_tx.try_send(ReloadEvent::FilesChanged) {
                            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                            Err(mpsc::error::TrySendError::Closed(_)) => break,
                        }
                    }
                }),
        );
        Self(state)
    }

    pub fn configure(&self, appearance: Appearance) {
        let changed = *self.0.appearance.borrow() != appearance;
        *self.0.appearance.borrow_mut() = appearance;
        if changed {
            self.0.watch(self.0.appearance.borrow().source_paths());
            self.0.request();
        }
    }

    pub fn refresh(&self) {
        self.0.request();
    }

    pub fn refresh_for_portal(&self, contrast_changed: bool) {
        let should_reload = contrast_changed || {
            let appearance = self.0.appearance.borrow();
            appearance.mode == super::ModePreference::System
                && appearance.source.uses_platform_mode()
        };
        if should_reload {
            self.0.request();
        }
    }
}

impl State {
    fn watch(&self, paths: Vec<PathBuf>) {
        if let Err(error) = self.sources.borrow_mut().set_paths(paths) {
            tracing::warn!(%error, "cannot watch appearance sources");
        }
    }

    fn request(&self) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let appearance = self.appearance.borrow().clone();
        let mode = crate::app::appearance::Theme::platform_mode();
        let task = self
            .scope
            .spawn_blocking_named("Read appearance palette", move || appearance.prepare(mode));
        let events = self.events.clone();
        drop(
            self.scope
                .spawn_named("Deliver appearance reload", async move {
                    let result = task.await;
                    let _ = events
                        .send(ReloadEvent::Prepared { generation, result })
                        .await;
                }),
        );
    }

    fn report(&self, message: String) {
        if self.error.borrow().as_ref() != Some(&message) {
            tracing::warn!(%message, "theme refresh failed; keeping the last palette");
            *self.error.borrow_mut() = Some(message);
        }
    }
}
