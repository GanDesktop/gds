//! One desktop source observer per process; windows only forward native appearance events.

use crate::service::desktop::watch::Sources;
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, Global, Task, Window, WindowAppearance,
    WindowId,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Copy)]
pub enum Change {
    Files,
    Platform,
}

struct Shared(Entity<Observer>);
impl Global for Shared {}

pub struct Observer {
    owners: HashMap<&'static str, Vec<PathBuf>>,
    sources: Option<Sources>,
    appearance: WindowAppearance,
    windows: HashSet<WindowId>,
    _events: Task<()>,
}
impl EventEmitter<Change> for Observer {}

impl Observer {
    pub(crate) fn shared(cx: &mut App) -> Entity<Self> {
        if let Some(shared) = cx.try_global::<Shared>() {
            return shared.0.clone();
        }
        let observer = cx.new(|cx| {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
            let task = cx.spawn(async move |observer, cx| {
                while receiver.recv().await.is_some() {
                    while receiver.try_recv().is_ok() {}
                    if observer
                        .update(cx, |observer: &mut Self, cx| {
                            if let Some(sources) = observer.sources.as_mut()
                                && let Err(error) = sources.refresh()
                            {
                                tracing::warn!("cannot refresh theme watches: {error}");
                            }
                            cx.emit(Change::Files);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            // OS threads cannot wake GPUI's deterministic test executor. Tests
            // inject the same change event through `refresh` instead.
            let sources = (!cx.is_test()).then(|| Sources::new(sender));
            Self {
                owners: HashMap::new(),
                sources,
                appearance: cx.window_appearance(),
                windows: HashSet::new(),
                _events: task,
            }
        });
        cx.set_global(Shared(observer.clone()));
        observer
    }

    /// Register one window without transferring ownership of the process watcher to it.
    pub(crate) fn attach(window: &mut Window, cx: &mut App) {
        let observer = Self::shared(cx);
        let id = window.window_handle().window_id();
        let attached = observer.update(cx, |observer, cx| {
            observer
                .windows
                .retain(|id| cx.windows().iter().any(|handle| handle.window_id() == *id));
            !observer.windows.insert(id)
        });
        if attached {
            return;
        }
        let weak = observer.downgrade();
        window
            .observe_window_appearance(move |window, cx| {
                let _ = weak.update(cx, |observer, cx| {
                    let appearance = window.appearance();
                    if observer.appearance != appearance {
                        observer.appearance = appearance;
                        cx.emit(Change::Platform);
                    }
                });
            })
            .detach();
    }

    /// # Errors
    /// Reports platform watch failures; existing watches remain active.
    pub fn watch(&mut self, owner: &'static str, paths: Vec<PathBuf>) -> notify::Result<()> {
        self.owners.insert(owner, paths);
        if let Some(sources) = self.sources.as_mut() {
            sources.set_paths(self.owners.values().flatten().cloned().collect())?;
        }
        Ok(())
    }

    /// Request refresh after an application change or a synthetic platform event.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        cx.emit(Change::Files);
    }

    pub(in crate::appearance) fn refresh_if_active(cx: &mut App) {
        if let Some(shared) = cx.try_global::<Shared>() {
            let observer = shared.0.clone();
            observer.update(cx, |_, cx| cx.emit(Change::Files));
        }
    }
}
