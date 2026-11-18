//! Reactive application preferences and an ordered writer drained at shutdown.

use super::{Preferences, api, presentation};
use anti_webui::prelude::*;
use gds::{app::Gds, service::task_manager::Scope};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Storage(#[from] api::Error),
    #[error(transparent)]
    Presentation(#[from] presentation::Error),
    #[error("Files is closing; preferences can no longer be changed")]
    Closed,
}

type Queue = Arc<Mutex<Option<mpsc::UnboundedSender<Preferences>>>>;

thread_local! {
    static INSTALLED: RefCell<Option<Service>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct Service(Rc<State>);

struct State {
    current: Signal<Preferences>,
    error: Signal<Option<String>>,
    queue: Queue,
    tasks: Scope,
}

impl Service {
    /// # Errors
    /// Reports an unavailable configuration directory.
    pub fn install(preferences: Preferences, tasks: &Scope) -> Result<Self, Error> {
        let service = Self::new(preferences, api::path()?, tasks);
        INSTALLED.with(|installed| *installed.borrow_mut() = Some(service.clone()));
        on_cleanup(|| {
            INSTALLED.with(|installed| installed.borrow_mut().take());
        });
        Ok(service)
    }

    pub fn current() -> Self {
        INSTALLED.with(|installed| {
            installed
                .borrow()
                .as_ref()
                .expect("Files preferences installed before windows")
                .clone()
        })
    }

    pub fn snapshot(&self) -> Preferences {
        self.0.current.get()
    }
    pub fn state(&self) -> Signal<Preferences> {
        self.0.current
    }
    pub fn error(&self) -> Signal<Option<String>> {
        self.0.error
    }

    fn new(preferences: Preferences, path: PathBuf, tasks: &Scope) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<Preferences>();
        let queue = Arc::new(Mutex::new(Some(sender)));
        let closing = queue.clone();
        let cancel = tasks.token();
        tasks.spawn_named("Close preferences writer on exit", async move {
            cancel.cancelled().await;
            closing.lock().expect("preferences queue lock").take();
        });
        let error = signal(None);
        let results = ui_channel(move |result: Result<(), String>| {
            if let Err(message) = result {
                error.set(Some(message));
            }
        });
        let mut store = api::Store::new(path);
        let writer = tasks.spawn_blocking_named("Save preferences", move || {
            while let Some(mut preferences) = receiver.blocking_recv() {
                while let Ok(newer) = receiver.try_recv() {
                    preferences = newer;
                }
                let result = store.save(&preferences).map_err(|error| error.to_string());
                if let Err(error) = &result {
                    tracing::error!(%error, "Cannot save Files preferences");
                }
                results.send(result);
            }
        });
        Gds::current()
            .tasks()
            .spawn("Observe preferences writer", writer)
            .then(move |result| {
                if let Err(failure) = result.and_then(std::convert::identity) {
                    error.set(Some(failure.to_string()));
                }
            });
        Self(Rc::new(State {
            current: signal(preferences),
            error,
            queue,
            tasks: tasks.clone(),
        }))
    }

    /// # Errors
    /// Rejects invalid presentation values and edits after shutdown admission closes.
    pub fn update(&self, preferences: Preferences) -> Result<(), Error> {
        let prepared = presentation::Prepared::new(&preferences, &self.0.current.get_untracked())?;
        let queue = self.0.queue.lock().expect("preferences queue lock");
        let sender = queue.as_ref().ok_or(Error::Closed)?;
        if self.0.tasks.token().is_cancelled() || sender.is_closed() {
            return Err(Error::Closed);
        }
        sender
            .send(preferences.clone())
            .map_err(|_| Error::Closed)?;
        drop(queue);
        batch(|| {
            prepared.install(&preferences);
            Gds::current()
                .appearance()
                .theme()
                .configure_hot_reload(preferences.appearance.clone());
            self.0.current.set_if_changed(preferences);
        });
        Ok(())
    }

    /// # Errors
    /// Rejects invalid presentation values or updates during shutdown.
    pub fn edit(&self, edit: impl FnOnce(&mut Preferences)) -> Result<(), Error> {
        let mut preferences = self.0.current.get_untracked();
        edit(&mut preferences);
        self.update(preferences)
    }
}

#[cfg(test)]
mod tests;
