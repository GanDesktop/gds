//! Shared palette refresh driven by the process's desktop source observer.

use super::{Appearance, Change, Error, Loaded, ModePreference, Observer, Source, apply_loaded};
use gpui_kit::{App, AppContext, Context, Entity, Global, Subscription, Task, Window};

struct Shared(Entity<HotReload>);
impl Global for Shared {}

pub struct HotReload {
    appearance: Appearance,
    generation: u64,
    load: Option<Task<()>>,
    error: Option<String>,
    _changes: Subscription,
}

impl HotReload {
    pub fn shared(appearance: Appearance, window: &mut Window, cx: &mut App) -> Entity<Self> {
        Observer::attach(window, cx);
        if let Some(shared) = cx.try_global::<Shared>() {
            return shared.0.clone();
        }
        let reload = cx.new(|cx| Self::new(appearance, window, cx));
        cx.set_global(Shared(reload.clone()));
        reload
    }

    pub fn new(appearance: Appearance, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Observer::attach(window, cx);
        let observer = Observer::shared(cx);
        let changes = cx.subscribe(&observer, |this, _, change, cx| {
            if matches!(change, Change::Files)
                || (this.appearance.mode == ModePreference::System
                    && this.appearance.source.uses_platform_mode())
            {
                this.request(cx);
            }
        });
        Self::watch(appearance.source_paths(), cx);
        let mut reload = Self {
            appearance,
            generation: 0,
            load: None,
            error: None,
            _changes: changes,
        };
        reload.request(cx);
        reload
    }

    pub fn set_appearance(
        &mut self,
        appearance: Appearance,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reload = self.appearance.source != appearance.source
            || self.appearance.mode != appearance.mode
            || (appearance.source == Source::Builtin
                && self.appearance.builtin != appearance.builtin);
        self.appearance = appearance;
        if reload {
            Self::watch(self.appearance.source_paths(), cx);
            self.request(cx);
        }
    }

    fn watch(paths: Vec<std::path::PathBuf>, cx: &mut App) {
        Observer::shared(cx).update(cx, |observer, _| {
            if let Err(error) = observer.watch("palette", paths) {
                tracing::warn!("cannot watch appearance sources: {error}");
            }
        });
    }

    fn request(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let appearance = self.appearance.clone();
        let platform = cx.window_appearance();
        self.load = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move { appearance.load_for(platform) })
                .await;
            let _ = this.update(cx, |this, cx| this.accept(generation, loaded, cx));
        }));
    }

    fn accept(&mut self, generation: u64, result: Result<Loaded, Error>, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        let result = result.and_then(|loaded| {
            let mut paths = self.appearance.source_paths();
            paths.extend(loaded.source_path.clone());
            Self::watch(paths, cx);
            apply_loaded(&self.appearance, loaded, cx)
        });
        match result {
            Ok(_) => self.error = None,
            Err(error) => {
                let error = error.to_string();
                if self.error.as_ref() != Some(&error) {
                    tracing::warn!("theme refresh failed, keeping the last applied theme: {error}");
                    self.error = Some(error);
                }
            }
        }
    }
}
