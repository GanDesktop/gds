//! Source-driven palette refresh owned by the application root.
use super::{Appearance, Prepared};
use crate::{
    app::Gds,
    service::{desktop::watch::Sources, task_manager::Owner},
};
use anti_webui::prelude::ui_channel;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct HotReload(Rc<State>);
struct State {
    appearance: RefCell<Appearance>,
    sources: RefCell<Sources>,
    scope: Owner,
    generation: Cell<u64>,
    error: RefCell<Option<String>>,
}
impl HotReload {
    pub(crate) fn new(appearance: Appearance) -> Self {
        let (notify, mut changes) = tokio::sync::mpsc::channel(1);
        let state = Rc::new(State {
            appearance: RefCell::new(appearance),
            sources: RefCell::new(Sources::new(notify)),
            scope: Gds::current().tasks().scope().named_child("Appearance"),
            generation: Cell::new(0),
            error: RefCell::new(None),
        });
        state.watch(state.appearance.borrow().source_paths());
        let weak = Rc::downgrade(&state);
        let sender = ui_channel(move |()| {
            if let Some(state) = weak.upgrade() {
                if let Err(error) = state.sources.borrow_mut().refresh() {
                    tracing::warn!(%error, "cannot refresh appearance watches");
                }
                state.request();
            }
        });
        let cancel = state.scope.token();
        drop(
            state
                .scope
                .spawn_named("Watch appearance files", async move {
                    loop {
                        enum Event {
                            Cancelled,
                            Changed(Option<()>),
                        }
                        let event = tokio::select! {
                            _ = cancel.cancelled() => Event::Cancelled,
                            value = changes.recv() => Event::Changed(value),
                        };
                        match event {
                            Event::Cancelled | Event::Changed(None) => break,
                            Event::Changed(Some(())) => {
                                while changes.try_recv().is_ok() {}
                                sender.send(());
                            }
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
}
impl State {
    fn watch(&self, paths: Vec<std::path::PathBuf>) {
        if let Err(error) = self.sources.borrow_mut().set_paths(paths) {
            tracing::warn!(%error, "cannot watch appearance sources");
        }
    }
    fn request(self: &Rc<Self>) {
        Gds::current().with(|| self.request_owned());
    }
    fn request_owned(self: &Rc<Self>) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let appearance = self.appearance.borrow().clone();
        let mode = crate::app::appearance::Theme::platform_mode();
        let weak = Rc::downgrade(self);
        let sender = ui_channel(
            move |result: Result<
                Result<Prepared, super::Error>,
                crate::service::task_manager::Error,
            >| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                if generation != state.generation.get() {
                    return;
                }
                let result = result
                    .map_err(|error| error.to_string())
                    .and_then(|value| value.map_err(|error| error.to_string()));
                match result {
                    Ok(prepared) => {
                        state.watch(prepared.source_paths());
                        match Gds::current().appearance().theme().install(prepared) {
                            Ok(_) => *state.error.borrow_mut() = None,
                            Err(error) => state.report(error.to_string()),
                        }
                    }
                    Err(error) => state.report(error),
                }
            },
        );
        self.scope
            .spawn_blocking_named("Read appearance palette", move || appearance.prepare(mode))
            .on_complete(&self.scope, move |result| sender.send(result));
    }
    fn report(&self, message: String) {
        if self.error.borrow().as_ref() != Some(&message) {
            tracing::warn!(%message, "theme refresh failed; keeping the last palette");
            *self.error.borrow_mut() = Some(message);
        }
    }
}
