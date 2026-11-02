//! Settings live in their workspace window; a standalone host supports settings-only launches.

use crate::shared::i18n::tr;
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, Focusable, Global, Render, Subscription, WeakEntity,
    Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, component::Root, prelude::*, px,
    size,
};

use crate::shared::lib::appearance as theme;
use crate::widgets::settings::ui::Settings;

const WIDTH: f32 = 1280.;
const HEIGHT: f32 = 820.;
const MIN_WIDTH: f32 = 1000.;
const MIN_HEIGHT: f32 = 640.;

pub(crate) use crate::shared::routes::{CloseWindow, NextPage, PreviousPage, ToggleSidebar};

#[derive(Clone)]
struct OpenWindow {
    handle: WindowHandle<Root>,
    settings: WeakEntity<Settings>,
}

#[derive(Default)]
struct OpenWindows(Vec<OpenWindow>);
impl Global for OpenWindows {}

pub(crate) fn bind_keys(cx: &mut App) {
    super::connections::bind_keys(cx);
    cx.bind_keys([
        gpui_kit::KeyBinding::new("escape", CloseWindow, Some("BasSettings")),
        gpui_kit::KeyBinding::new("alt-left", CloseWindow, Some("BasSettings")),
        gpui_kit::KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            },
            CloseWindow,
            Some("BasSettings"),
        ),
        gpui_kit::KeyBinding::new("ctrl-tab", NextPage, Some("BasSettings")),
        gpui_kit::KeyBinding::new("ctrl-shift-tab", PreviousPage, Some("BasSettings")),
        gpui_kit::KeyBinding::new("ctrl-b", ToggleSidebar, Some("BasSettings")),
    ]);
}

fn register(handle: WindowHandle<Root>, settings: WeakEntity<Settings>, cx: &mut App) {
    let windows = cx.windows();
    let state = cx.default_global::<OpenWindows>();
    state.0.retain(|entry| {
        entry.handle.window_id() != handle.window_id() && windows.contains(&entry.handle.into())
    });
    state.0.push(OpenWindow { handle, settings });
}

pub(crate) fn unregister(id: gpui_kit::WindowId, cx: &mut App) {
    if cx.has_global::<OpenWindows>() {
        cx.global_mut::<OpenWindows>()
            .0
            .retain(|entry| entry.handle.window_id() != id);
    }
}

fn selected(cx: &App) -> Option<OpenWindow> {
    let state = cx.try_global::<OpenWindows>()?;
    let active = cx.active_window().map(|window| window.window_id());
    state
        .0
        .iter()
        .rev()
        .filter(|entry| cx.windows().contains(&entry.handle.into()))
        .find(|entry| Some(entry.handle.window_id()) == active)
        .or_else(|| {
            state
                .0
                .iter()
                .rev()
                .find(|entry| cx.windows().contains(&entry.handle.into()))
        })
        .cloned()
}

fn settings(cx: &App, handle: WindowHandle<Root>) -> WeakEntity<Settings> {
    cx.global::<OpenWindows>()
        .0
        .iter()
        .find(|entry| entry.handle == handle)
        .expect("opening settings registers the view")
        .settings
        .clone()
}

/// # Errors
/// Reports a closed workspace or native window creation failure.
pub(crate) fn open(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    if let Some((handle, view)) = crate::features::navigation::show_settings(cx)? {
        let settings = view
            .downcast::<Settings>()
            .expect("the settings route creates a Settings view");
        register(handle, settings.downgrade(), cx);
        return Ok(handle);
    }
    if let Some(state) = selected(cx) {
        let view = state.settings.clone();
        if cx
            .update_window(state.handle.into(), |_, window, cx| {
                view.update(cx, |settings, cx| settings.on_backend_changed(window, cx))?;
                window.activate_window();
                anyhow::Ok(())
            })
            .is_ok_and(|result| result.is_ok())
        {
            return Ok(state.handle);
        }
    }
    let mut view = None;
    let handle = cx.open_window(window_options(cx), |window, cx| {
        window.set_window_title(tr!("settings-window-title"));
        let host = cx.new(|cx| Host::new(window, cx));
        view = Some(host.read(cx).settings.downgrade());
        cx.new(|cx| theme::window_root(host, window, cx))
    })?;
    register(handle, view.expect("settings host creates its view"), cx);
    Ok(handle)
}

fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        kind: WindowKind::Normal,
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        ))),
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        window_background: cx.global::<theme::Appearance>().background,
        app_id: Some("io.github.GanDesktop.GdsAgents".into()),
        ..gds::ui::window::window_options()
    }
}

/// # Errors
/// Returns an error if the settings window cannot be opened or updated.
pub(crate) fn open_connections(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let handle = open(cx)?;
    let settings = settings(cx, handle);
    handle.update(cx, |_, window, cx| {
        settings.update(cx, |settings, cx| {
            settings.select_category(super::Category::Connections, window, cx);
        })
    })??;
    Ok(handle)
}

/// Opens a local agent's settings and explicitly refreshes its ACP auth methods.
///
/// # Errors
/// Returns an error if the settings window cannot be opened or updated.
pub(crate) fn open_agent(agent: &str, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let handle = open(cx)?;
    let settings = settings(cx, handle);
    handle.update(cx, |_, window, cx| {
        settings.update(cx, |settings, cx| settings.show_agent(agent, window, cx))
    })??;
    Ok(handle)
}

/// # Errors
/// Returns an error if the settings window cannot be opened or updated.
pub(crate) fn open_agent_management(
    agent: &str,
    target: Option<bas::service::tls::ConnectionOptions>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Root>> {
    if let Some(target) = target {
        crate::features::navigation::manage(target, cx);
    } else {
        anyhow::ensure!(
            crate::features::navigation::current(cx).is_none(),
            "The originating standalone settings context is no longer active"
        );
    }
    let handle = open(cx)?;
    let settings = settings(cx, handle);
    handle.update(cx, |_, window, cx| {
        settings.update(cx, |settings, cx| {
            let index = settings
                .agents
                .read(cx)
                .configuration_entries()
                .iter()
                .position(|entry| entry.id == agent);
            settings.select_agents(index, window, cx);
        })
    })??;
    Ok(handle)
}

/// # Errors
/// Returns an error if the independent agent onboarding window cannot be opened.
pub(crate) fn open_catalog(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    super::agents::window::open_catalog(cx)
}

/// Persist valid edits in every open settings view before quitting.
pub(crate) fn flush_pending(cx: &mut App) {
    let entries = cx
        .try_global::<OpenWindows>()
        .map(|state| state.0.clone())
        .unwrap_or_default();
    for state in entries {
        if let Err(error) = cx.update_window(state.handle.into(), |_, window, cx| {
            state
                .settings
                .update(cx, |settings, cx| settings.prepare_to_close(window, cx))
        }) {
            tracing::debug!(%error, "settings window closed before flushing");
        }
    }
}

struct Host {
    settings: Entity<Settings>,
    _events: Subscription,
}

impl Host {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.new(|cx| Settings::new(window, cx));
        let weak = settings.downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            if let Err(error) =
                weak.update(cx, |settings, cx| settings.prepare_to_close(window, cx))
            {
                tracing::debug!(%error, "settings view has closed");
            }
            unregister(window.window_handle().window_id(), cx);
            true
        });
        let events = cx.subscribe_in(
            &settings,
            window,
            |_, settings, event, window, cx| match event {
                super::Event::Close | super::Event::CloseWindow => {
                    settings.update(cx, |settings, cx| settings.prepare_to_close(window, cx));
                    unregister(window.window_handle().window_id(), cx);
                    window.remove_window();
                }
            },
        );
        settings.read(cx).focus_handle(cx).focus(window, cx);
        window.activate_window();
        Self {
            settings,
            _events: events,
        }
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(tr!("settings-window-title"));
        self.settings.clone()
    }
}
