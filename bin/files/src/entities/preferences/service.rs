//! One application snapshot and an ordered writer drained by the task supervisor.

use super::{Preferences, api, presentation};
use gds::app::AppExt as _;
use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Global, Task};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

pub(crate) struct Current(pub Preferences);
impl Global for Current {}

impl Current {
    fn get(cx: &App) -> Preferences {
        cx.global::<Self>().0.clone()
    }
}

pub(crate) struct SaveFailed(pub String);

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Presentation(#[from] presentation::Error),
    #[error("Files is closing; preferences can no longer be changed")]
    Closed,
}

type Queue = Arc<Mutex<Option<mpsc::UnboundedSender<Preferences>>>>;

struct Installed(Entity<Service>);
impl Global for Installed {}

pub(crate) struct Service {
    queue: Queue,
    _updates: Task<()>,
}
impl EventEmitter<SaveFailed> for Service {}

impl Service {
    fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let tasks = cx.gds().tasks().scope();
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
            queue,
            _updates: updates,
        }
    }

    fn update(&self, preferences: Preferences, cx: &mut App) -> Result<(), Error> {
        let prepared = presentation::Prepared::new(&preferences, &Current::get(cx), cx)?;
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
        gds::appearance::theme::HotReload::get(cx).update(cx, |reload, cx| {
            reload.configure(preferences.appearance.clone(), cx);
        });
        if cx.global::<Current>().0 != preferences {
            cx.set_global(Current(preferences));
        }
        Ok(())
    }
}

pub(crate) trait PreferencesExt {
    fn preferences(&mut self) -> Accessor<'_>;
}

impl PreferencesExt for gds::app::Gds<'_> {
    fn preferences(&mut self) -> Accessor<'_> {
        Accessor { cx: self.app() }
    }
}

pub(crate) struct Accessor<'a> {
    cx: &'a mut App,
}

impl Accessor<'_> {
    pub(crate) fn current(&self) -> Preferences {
        Current::get(self.cx)
    }

    /// # Errors
    /// Reports an unavailable application configuration directory.
    pub(crate) fn initialize(
        &mut self,
        preferences: Preferences,
    ) -> Result<Entity<Service>, api::Error> {
        Ok(self.install(preferences, api::path()?))
    }

    fn install(&mut self, preferences: Preferences, path: PathBuf) -> Entity<Service> {
        assert!(
            !self.cx.has_global::<Installed>(),
            "preferences already initialized"
        );
        self.cx.set_global(Current(preferences));
        let service = self.cx.new(|cx| Service::new(path, cx));
        self.cx.set_global(Installed(service.clone()));
        service
    }

    /// # Errors
    /// Rejects invalid presentation values or updates during shutdown.
    pub(crate) fn update(&mut self, preferences: Preferences) -> Result<(), Error> {
        let service = self.cx.global::<Installed>().0.clone();
        service.update(self.cx, |service, cx| service.update(preferences, cx))
    }

    /// Edit the latest application snapshot, preserving other windows' changes.
    ///
    /// # Errors
    /// Rejects invalid presentation values or updates during shutdown.
    pub(crate) fn edit(&mut self, edit: impl FnOnce(&mut Preferences)) -> Result<(), Error> {
        let mut preferences = Current::get(self.cx);
        edit(&mut preferences);
        self.update(preferences)
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
