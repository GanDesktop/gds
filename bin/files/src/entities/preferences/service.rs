//! One application snapshot and an ordered writer drained by the task supervisor.

use super::{Preferences, api, presentation};
use gds::app::AppExt as _;
use gds::service::task_manager::Scope;
use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Global, Task};
use std::{
    ops::Deref,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

pub struct SaveFailed(pub String);

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

struct Installed(Entity<Service>);
impl Global for Installed {}

pub struct Service {
    current: Preferences,
    queue: Queue,
    _updates: Task<()>,
}
impl EventEmitter<SaveFailed> for Service {}

impl Service {
    pub fn current(&self) -> &Preferences {
        &self.current
    }

    fn new(preferences: Preferences, path: PathBuf, tasks: &Scope, cx: &mut Context<Self>) -> Self {
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
        let (results, mut saved) = mpsc::unbounded_channel();
        let mut store = api::Store::new(path);
        let writer = tasks.spawn_blocking_named("Save preferences", move || {
            while let Some(mut preferences) = receiver.blocking_recv() {
                while let Ok(newer) = receiver.try_recv() {
                    preferences = newer;
                }
                let result = store.save(&preferences);
                if let Err(error) = &result {
                    tracing::error!(%error, "Cannot save Files preferences");
                }
                let _ = results.send(result);
            }
        });
        let updates = cx.spawn(async move |this, cx| {
            while let Some(result) = saved.recv().await {
                if let Err(error) = result {
                    let _ = this.update(cx, |_, cx| cx.emit(SaveFailed(error.to_string())));
                }
            }
            if let Err(error) = writer.await {
                tracing::error!(%error, "Files preferences writer stopped");
                let _ = this.update(cx, |_, cx| cx.emit(SaveFailed(error.to_string())));
            }
        });
        Self {
            current: preferences,
            queue,
            _updates: updates,
        }
    }

    fn update(&mut self, preferences: Preferences, cx: &mut Context<Self>) -> Result<(), Error> {
        let prepared = presentation::Prepared::new(&preferences, &self.current, cx)?;
        let queue = self.queue.lock().expect("preferences queue lock");
        let sender = queue.as_ref().ok_or(Error::Closed)?;
        if cx.gds().tasks().is_shutting_down() || sender.is_closed() {
            return Err(Error::Closed);
        }
        // Persist failures leave this accepted snapshot active in every window.
        // The lock prevents shutdown from closing admission during the commit.
        sender
            .send(preferences.clone())
            .map_err(|_| Error::Closed)?;
        prepared.install(&preferences, cx);
        cx.gds_mut()
            .appearance()
            .theme()
            .configure_hot_reload(preferences.appearance.clone());
        if self.current != preferences {
            self.current = preferences;
            cx.notify();
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Accessor<C> {
    cx: C,
}

pub(crate) trait PreferencesExt {
    type Context<'a>: gds::app::Context
    where
        Self: 'a;

    fn preferences(&mut self) -> Accessor<Self::Context<'_>>;
}

impl<C: gds::app::Context> PreferencesExt for gds::app::Gds<C> {
    type Context<'a>
        = C::Reborrowed<'a>
    where
        C: 'a;

    fn preferences(&mut self) -> Accessor<C::Reborrowed<'_>> {
        Accessor {
            cx: self.reborrow().into_inner(),
        }
    }
}

impl<C: Deref<Target = App>> Accessor<C> {
    pub(crate) fn current(&self) -> Preferences {
        self.service().read(&self.cx).current.clone()
    }

    pub(crate) fn service(&self) -> Entity<Service> {
        self.cx.global::<Installed>().0.clone()
    }
}

impl Accessor<&mut App> {
    /// # Errors
    /// Reports an unavailable application configuration directory.
    pub(crate) fn initialize(
        &mut self,
        preferences: Preferences,
        tasks: &Scope,
    ) -> Result<Entity<Service>, Error> {
        Ok(self.install(preferences, api::path()?, tasks))
    }

    #[cfg(all(test, feature = "gui-tests"))]
    pub(crate) fn init_test(
        &mut self,
        preferences: Preferences,
        path: PathBuf,
        tasks: &Scope,
    ) -> Entity<Service> {
        self.install(preferences, path, tasks)
    }

    fn install(
        &mut self,
        preferences: Preferences,
        path: PathBuf,
        tasks: &Scope,
    ) -> Entity<Service> {
        assert!(
            !self.cx.has_global::<Installed>(),
            "preferences already initialized"
        );
        let service = self.cx.new(|cx| Service::new(preferences, path, tasks, cx));
        self.cx.set_global(Installed(service.clone()));
        service
    }

    /// # Errors
    /// Rejects invalid presentation values or updates during shutdown.
    pub(crate) fn update(&mut self, preferences: Preferences) -> Result<(), Error> {
        let service = self.service();
        service.update(self.cx, |service, cx| service.update(preferences, cx))
    }

    /// Edit the latest application snapshot, preserving other windows' changes.
    ///
    /// # Errors
    /// Rejects invalid presentation values or updates during shutdown.
    pub(crate) fn edit(&mut self, edit: impl FnOnce(&mut Preferences)) -> Result<(), Error> {
        let mut preferences = self.current();
        edit(&mut preferences);
        self.update(preferences)
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
