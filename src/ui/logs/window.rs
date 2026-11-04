//! Process-local log-window hosting and shortcut registration.
use super::*;
use anyhow::Context as _;
use gpui_kit::{
    AnyView, Bounds, Global, KeyBinding, NoAction, SharedString, WeakEntity,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};

pub(super) const WIDTH: f32 = 1_000.;
pub(super) const HEIGHT: f32 = 720.;
#[cfg(test)]
pub(super) const MIN_WIDTH: f32 = 780.;
#[cfg(test)]
pub(super) const MIN_HEIGHT: f32 = 480.;

actions!(logs, [OpenLogs]);

/// Applications provide identity and their normal window-root decoration.
#[derive(Clone)]
pub struct Options {
    pub app_id: Option<SharedString>,
    pub app_name: SharedString,
    pub background: fn(&App) -> WindowBackgroundAppearance,
    pub root: fn(AnyView, &mut Window, &mut Context<Root>) -> Root,
    pub ignored_contexts: &'static [&'static str],
}

impl Default for Options {
    fn default() -> Self {
        Self {
            app_id: None,
            app_name: "GDS".into(),
            background: |_| crate::appearance::tokens::WINDOW_BACKGROUND,
            root: |view, window, cx| {
                crate::appearance::theme::Observer::attach(window, cx);
                let surface = crate::ui::notifications::surface::wrap(view, window, cx);
                Root::new(surface, window, cx).text_size(crate::appearance::tokens::SMALL)
            },
            ignored_contexts: &[],
        }
    }
}

struct Records(Store);
impl Global for Records {}
struct Registered(Options);
impl Global for Registered {}

pub(super) struct OpenWindow {
    pub(super) handle: WindowHandle<Root>,
    pub(super) viewer: WeakEntity<Viewer>,
}
impl Global for OpenWindow {}

/// Install the collector created before the application's background tasks start.
pub fn install(store: Store, cx: &mut App) {
    cx.set_global(Records(store));
}

pub fn is_enabled(cx: &App) -> bool {
    cx.has_global::<Records>()
}

pub fn store(cx: &App) -> Option<Store> {
    cx.try_global::<Records>().map(|records| records.0.clone())
}

/// Register the viewer's actions once for this application process.
pub fn init(options: Options, cx: &mut App) {
    if !is_enabled(cx) || cx.has_global::<Registered>() {
        return;
    }
    let open_key = if cfg!(target_os = "macos") {
        "cmd-shift-l"
    } else {
        "ctrl-shift-l"
    };
    let close_key = if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-w"
    };
    cx.bind_keys([
        KeyBinding::new(open_key, OpenLogs, None),
        KeyBinding::new(close_key, CloseLogs, Some("GdsLogs")),
    ]);
    for context in std::iter::once(crate::ui::terminal::INPUT_CONTEXT)
        .chain(options.ignored_contexts.iter().copied())
    {
        cx.bind_keys([KeyBinding::new(open_key, NoAction, Some(context))]);
    }
    for context in ["GdsLogsList", "GdsLogs > Input"] {
        cx.bind_keys([
            KeyBinding::new("up", PreviousLog, Some(context)),
            KeyBinding::new("down", NextLog, Some(context)),
            KeyBinding::new("pageup", PreviousLogPage, Some(context)),
            KeyBinding::new("pagedown", NextLogPage, Some(context)),
        ]);
    }
    cx.set_global(Registered(options));
    cx.on_action(|_: &OpenLogs, cx| {
        // The originating popup or viewer may still be borrowed by dispatch.
        cx.defer(|cx| {
            if let Err(error) = open(cx) {
                if let Some(store) = store(cx) {
                    store.record(Level::Error, "gds::ui::logs", error.to_string());
                }
                crate::ui::notifications::error(
                    format!("{}: {error}", t!(@cx, "logs-open-error")),
                    cx,
                );
            }
        });
    });
}

/// Reveal the existing viewer or open a window for this process's collector.
///
/// # Errors
/// Reports disabled logging, missing initialization or native window creation errors.
pub fn open(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let store =
        store(cx).context("Logging is disabled; start the application with --enable-logs")?;
    let options = cx
        .try_global::<Registered>()
        .context("Log viewer is not initialized")?
        .0
        .clone();
    if let Some(state) = cx.try_global::<OpenWindow>() {
        let handle = state.handle;
        let viewer = state.viewer.clone();
        if handle
            .update(cx, |_, window, cx| {
                let _ = viewer.update(cx, |viewer, cx| {
                    viewer
                        .search
                        .update(cx, |search, cx| search.focus(window, cx));
                });
                window.activate_window();
            })
            .is_ok()
        {
            return Ok(handle);
        }
    }
    let mut viewer = None;
    let handle = cx.open_window(
        WindowOptions {
            kind: WindowKind::Floating,
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                gds::ui::window::fit_size(size(px(WIDTH), px(HEIGHT)), cx),
                cx,
            ))),
            window_background: (options.background)(cx),
            app_id: options.app_id.as_ref().map(ToString::to_string),
            ..TitleBar::window_options()
        },
        |window, cx| {
            window.set_window_title(&format!("{} · {}", options.app_name, t!(@cx, "logs-title")));
            let host = cx.new(|cx| Viewer::new(store, window, cx));
            viewer = Some(host.downgrade());
            host.read(cx)
                .search
                .clone()
                .update(cx, |search, cx| search.focus(window, cx));
            window.activate_window();
            cx.new(|cx| (options.root)(host.into(), window, cx))
        },
    )?;
    cx.set_global(OpenWindow {
        handle,
        viewer: viewer.expect("window creates its viewer"),
    });
    Ok(handle)
}
