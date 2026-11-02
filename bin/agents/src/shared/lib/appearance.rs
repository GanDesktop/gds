//! Resolve Agents preferences over desktop adapters, then apply the effective appearance.

mod loader;
pub(crate) mod preferences;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

use gpui_kit::{
    AnyView, App, Context, Global, Pixels, Styled, Subscription, Task, Window,
    WindowBackgroundAppearance,
    component::{Root, StyledExt, Theme, ThemeMode},
    px,
};
use loader::Loader;
use preferences::Preferences;
use std::{cell::Cell, rc::Rc};

pub(crate) use loader::Document;

pub(crate) struct Editor {
    loader: Loader,
    generation: Rc<Cell<u64>>,
}

impl Editor {
    pub(crate) fn current(cx: &App) -> Self {
        let config = cx.global::<WatchConfig>();
        Self {
            loader: config.loader.clone(),
            generation: config.generation.clone(),
        }
    }

    pub(crate) fn appearance_path(&self) -> &std::path::Path {
        self.loader.store.path()
    }

    /// # Errors
    /// Returns configuration read/parse errors, preserving the original file.
    pub(crate) fn load(&self) -> anyhow::Result<Document> {
        self.loader.document()
    }

    /// # Errors
    /// Validation, conflicts or IO failures leave the current appearance intact.
    pub(crate) fn save(
        &self,
        document: &mut Document,
        candidate: Preferences,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        let dark = ThemeMode::from(window.appearance()).is_dark();
        let effective = self.loader.save(document, candidate, dark)?;
        self.generation.set(self.generation.get().wrapping_add(1));
        apply(effective, Some(window), cx);
        Ok(())
    }
}

pub(crate) fn content_font_size(cx: &App) -> Pixels {
    px(cx.global::<Appearance>().content_font_size)
}

pub(crate) fn content_line_height(cx: &App) -> f32 {
    cx.global::<Appearance>().content_line_height
}

pub(crate) fn content_background(cx: &App) -> gpui_kit::Hsla {
    gds::appearance::theme::content_background(cx)
}

#[derive(Clone)]
struct WatchConfig {
    loader: Loader,
    generation: Rc<Cell<u64>>,
}
impl Global for WatchConfig {}

struct WatchTask {
    _task: Task<()>,
    _subscription: Subscription,
}
impl Global for WatchTask {}

#[derive(Clone)]
pub(crate) struct Appearance {
    pub name: String,
    pub background: WindowBackgroundAppearance,
    pub content_font_size: f32,
    pub content_line_height: f32,
    pub content_opacity: f32,
    pub sidebar_opacity: f32,
    pub composer_enter_behavior: preferences::ComposerEnterBehavior,
    pub default_agent: Option<String>,
    pub confirm_session_delete: bool,
    pub auto_collapse_activity: bool,
    pub font_fallbacks: gpui_kit::FontFallbacks,
}
impl Global for Appearance {}

#[cfg(all(test, feature = "gui-tests"))]
pub(crate) fn init_default(cx: &mut App) {
    let preferences = Preferences {
        theme_source: preferences::ThemeSource::Builtin,
        ..Preferences::default()
    };
    apply(
        loader::Resolved {
            effective: preferences::resolve(&preferences).expect("valid defaults"),
            palette: preferences::appearance(&preferences)
                .prepare(gpui_kit::WindowAppearance::Dark)
                .expect("built-in palette"),
        },
        None,
        cx,
    );
}

pub(crate) fn init(cx: &mut App) {
    let loader = Loader::new(crate::shared::config::store(cx));
    let dark = ThemeMode::from(cx.window_appearance()).is_dark();
    let effective = loader.read(dark).and_then(|snapshot| snapshot.resolve())
        .unwrap_or_else(|error| {
            tracing::warn!(error = %format!("{error:#}"), "cannot load appearance; using built-in defaults");
            let preferences = Preferences { theme_source: preferences::ThemeSource::Builtin, ..Preferences::default() };
            loader::Resolved {
                effective: preferences::resolve(&preferences).expect("valid defaults"),
                palette: preferences::appearance(&preferences).prepare(cx.window_appearance()).expect("built-in palette"),
            }
        });
    apply(effective, None, cx);
    cx.set_global(WatchConfig {
        loader,
        generation: Rc::default(),
    });
}

pub(crate) fn watch(cx: &mut App) {
    if cx.has_global::<WatchTask>() {
        return;
    }
    let config = cx.global::<WatchConfig>().clone();
    let observer = gds::appearance::theme::Observer::shared(cx);
    let mut paths = vec![config.loader.store.path().to_owned()];
    if let Ok(document) = config.loader.document() {
        paths.extend(preferences::appearance(&document.preferences).source_paths());
    }
    if let Ok(snapshot) = config
        .loader
        .read(ThemeMode::from(cx.window_appearance()).is_dark())
    {
        paths.extend(snapshot.source_paths());
    }
    observer.update(cx, |observer, _| {
        if let Err(error) = observer.watch("agents-preferences", paths) {
            tracing::warn!(%error, "cannot watch frontend appearance");
        }
    });
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let revision = config.generation.clone();
    let subscription = cx.subscribe(&observer, move |_, _, _| {
        revision.set(revision.get().wrapping_add(1));
        let _ = sender.try_send(());
    });
    let task = cx.spawn(async move |cx| {
        let mut reload = loader::Reload::default();
        let mut previous_error = None;
        while receiver.recv().await.is_some() {
            let generation = config.generation.get();
            let source = config.loader.clone();
            let dark = cx.update(|cx| ThemeMode::from(cx.window_appearance()).is_dark());
            let files = cx.background_executor().spawn(async move { source.read(dark) }).await;
            cx.update(|cx| {
                reload.invalidate_on_save(config.generation.get());
                if generation != config.generation.get() { return; }
                match files.and_then(|snapshot| {
                    let mut paths = snapshot.source_paths();
                    paths.push(config.loader.store.path().to_owned());
                    observer.update(cx, |observer, _| observer.watch("agents-preferences", paths))?;
                    reload.update(snapshot)
                }) {
                    Ok(effective) => {
                        previous_error = None;
                        if let Some(effective) = effective { apply(effective, None, cx); }
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        if previous_error.as_ref() != Some(&error) {
                            tracing::warn!(%error, "cannot reload appearance; keeping the last valid configuration");
                            previous_error = Some(error);
                        }
                    }
                }
            });
        }
    });
    cx.set_global(WatchTask {
        _task: task,
        _subscription: subscription,
    });
}

fn apply(resolved: loader::Resolved, window: Option<&mut Window>, cx: &mut App) {
    let effective = resolved.effective;
    let preferences = &effective.preferences;
    gds::appearance::motion::set_animations_enabled(
        preferences.animations_enabled.unwrap_or(true),
        cx,
    );
    let opacity = gds::appearance::tokens::SurfaceOpacity::default();
    crate::shared::i18n::install(preferences.language, cx);
    let applied = resolved
        .palette
        .install(cx)
        .expect("prepared palettes have validated opacity");
    let font = gds::appearance::typography::install(&effective.font, cx)
        .expect("resolved typography is validated");
    if let Some(family) = font.unavailable {
        tracing::warn!(%family, "font is unavailable; using the system font");
    }
    if let Some(family) = &preferences.mono_font_family {
        let fonts = cx.text_system().all_font_names();
        if fonts.contains(family) {
            Theme::global_mut(cx).mono_font_family = family.clone().into();
        }
    }
    cx.set_global(Appearance {
        name: if applied.source == gds::appearance::theme::Source::Builtin {
            crate::shared::i18n::text(preferences.builtin_theme.label_id()).to_owned()
        } else {
            format!("{:?}", applied.source)
        },
        background: effective.background,
        content_font_size: effective.content_font_size,
        content_line_height: effective.content_line_height,
        content_opacity: preferences.content_opacity.unwrap_or(opacity.content),
        sidebar_opacity: preferences.sidebar_opacity.unwrap_or(opacity.sidebar),
        composer_enter_behavior: preferences.composer_enter_behavior,
        default_agent: preferences.default_agent.clone(),
        confirm_session_delete: preferences.confirm_session_delete.unwrap_or(true),
        auto_collapse_activity: preferences.auto_collapse_activity.unwrap_or(true),
        font_fallbacks: gpui_kit::FontFallbacks::from_fonts(
            preferences.font_fallbacks.clone().unwrap_or_default(),
        ),
    });
    if let Some(window) = window {
        window.set_background_appearance(effective.background);
    }
    cx.refresh_windows();
    cx.defer(|cx| {
        let background = cx.global::<Appearance>().background;
        for handle in cx.windows() {
            if let Err(error) = handle.update(cx, |_, window, _| {
                window.set_background_appearance(background)
            }) {
                tracing::debug!(%error, "appearance target window has closed");
            }
        }
    });
}

/// All native windows share UI typography, including overlays hosted by Root.
pub(crate) fn window_root(
    view: impl Into<AnyView>,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> Root {
    gds::appearance::theme::Observer::attach(window, cx);
    cx.observe_global::<Appearance>(|root, cx| {
        root.style().text = font_style(cx).text;
        cx.notify();
    })
    .detach();
    let view = crate::shared::lib::diagnostics::wrap(view, window, cx);
    Root::new(view, window, cx).refine_style(&font_style(cx))
}

pub(crate) fn font_style(cx: &App) -> gpui_kit::StyleRefinement {
    let mut style = gpui_kit::StyleRefinement::default();
    style.text.font_size = Some(crate::shared::ui::SMALL.into());
    style.text.font_fallbacks = cx
        .try_global::<Appearance>()
        .map(|appearance| appearance.font_fallbacks.clone());
    style
}
