//! Reactive application preferences and an ordered writer drained at shutdown.

use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

use gds::{app::Gds, service::task_manager::Scope};
use tokio::sync::mpsc;
use zgui::prelude::*;

use super::{Preferences, api, presentation};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Path(#[from] super::api::Error),
    #[error(transparent)]
    Presentation(#[from] presentation::Error),
    #[error("application is shutting down")]
    Closed,
}

type Queue = Arc<Mutex<Option<mpsc::UnboundedSender<Preferences>>>>;

thread_local! {
    static INSTALLED: RefCell<Option<Service>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct Service(Rc<State>);

struct State {
    current: RwSignal<Preferences, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
    queue: Queue,
    tasks: Scope,
}

impl Service {
    /// # Errors
    /// Reports an unavailable configuration directory.
    pub fn install(preferences: Preferences, tasks: &Scope) -> Result<Self, Error> {
        let path = api::path()?;
        let service = Gds::current().with(|| {
            let service = Self::new(preferences, path, tasks);
            INSTALLED.with(|installed| *installed.borrow_mut() = Some(service.clone()));
            on_cleanup_local(|| {
                INSTALLED.with(|installed| installed.borrow_mut().take());
            });
            service
        });
        Ok(service)
    }

    pub fn current() -> Self {
        Self::try_current().expect("Files preferences installed before windows")
    }

    pub fn try_current() -> Option<Self> {
        INSTALLED.with(|installed| installed.borrow().clone())
    }

    pub fn snapshot(&self) -> Preferences {
        self.0.current.get_untracked()
    }

    pub fn state(&self) -> ReadSignal<Preferences, LocalStorage> {
        self.0.current.read_only()
    }

    pub fn error(&self) -> ReadSignal<Option<String>, LocalStorage> {
        self.0.error.read_only()
    }

    fn new(preferences: Preferences, path: PathBuf, tasks: &Scope) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<Preferences>();
        let queue = Arc::new(Mutex::new(Some(sender)));
        let closing = queue.clone();
        let cancel = tasks.token();
        // Closing the sender seals admission. The writer drains accepted changes
        // before its blocking task completes, including during application exit.
        tasks.spawn_named("Close preferences writer on exit", async move {
            cancel.cancelled().await;
            closing.lock().expect("preferences queue lock").take();
        });
        let (results, mut saved) = mpsc::unbounded_channel::<Result<(), String>>();
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
                let _ = results.send(result);
            }
        });
        let error = RwSignal::new_local(None);
        Gds::current().with(|| {
            zgui::task::spawn_local(async move {
                while let Some(result) = saved.recv().await {
                    if let Err(message) = result {
                        error.try_set(Some(message));
                    }
                }
            });
            Gds::current()
                .tasks()
                .spawn_ui("Observe preferences writer", writer, move |result| {
                    if let Err(failure) = result.and_then(std::convert::identity) {
                        error.try_set(Some(failure.to_string()));
                    }
                });
        });
        Self(Rc::new(State {
            current: RwSignal::new_local(preferences),
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
        // Persist failures leave this accepted snapshot active in every window.
        // The lock prevents shutdown from closing admission during the commit.
        sender
            .send(preferences.clone())
            .map_err(|_| Error::Closed)?;
        drop(queue);
        let appearance_changed = prepared.appearance_changed();
        prepared.install(&preferences);
        if appearance_changed {
            Gds::current()
                .appearance()
                .theme()
                .configure_hot_reload(preferences.appearance.clone());
        }
        if self
            .0
            .current
            .with_untracked(|current| *current != preferences)
        {
            self.0.current.set(preferences);
        }
        Ok(())
    }

    /// # Errors
    /// Rejects invalid presentation values and edits after shutdown admission closes.
    pub fn edit(&self, edit: impl FnOnce(&mut Preferences)) -> Result<(), Error> {
        let mut preferences = self.0.current.get_untracked();
        edit(&mut preferences);
        self.update(preferences)
    }
}

#[cfg(test)]
mod tests;
