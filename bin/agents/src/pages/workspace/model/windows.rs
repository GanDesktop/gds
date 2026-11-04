use std::{collections::HashMap, path::PathBuf};

use bas::service::tls::ConnectionOptions as Target;
use gpui_kit::{
    App, AppContext, Bounds, Global, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    component::Root, prelude::*, transparent_black,
};
use tokio::runtime::Handle;

mod host;
pub(crate) use host::bind_page_keys;
pub(crate) use host::{Host as WorkspaceHost, workspace_view};

#[derive(Clone)]
struct Launch {
    runtime: Handle,
    cwd: PathBuf,
    compact: bool,
}
impl Global for Launch {}

#[derive(Default)]
struct Windows(HashMap<Target, WindowHandle<Root>>);
impl Global for Windows {}

pub(crate) fn target_page(window: gpui_kit::AnyWindowHandle, cx: &mut App) -> bool {
    if !cx.windows().contains(&window) {
        return false;
    }
    cx.default_global::<crate::entities::agent::Changes>()
        .target = Some(window.window_id());
    true
}

pub(crate) fn page_host(cx: &App) -> Option<(WindowHandle<Root>, gpui_kit::Entity<WorkspaceHost>)> {
    let preferred = cx
        .try_global::<crate::entities::agent::Changes>()
        .and_then(|changes| changes.target)
        .or_else(|| cx.active_window().map(|window| window.window_id()));
    let mut windows = cx.windows();
    windows.sort_by_key(|window| Some(window.window_id()) != preferred);
    windows.into_iter().find_map(|window| {
        let handle = window.downcast::<Root>()?;
        let root = handle.read(cx).ok()?;
        let host = crate::shared::lib::diagnostics::content(root, cx)
            .downcast::<WorkspaceHost>()
            .ok()?;
        Some((handle, host))
    })
}

/// Called only after the application-wide quit confirmation has been accepted.
pub(crate) fn shutdown_terminals(cx: &mut App) -> Vec<gds::ui::terminal::Shutdown> {
    let hosts: Vec<_> = cx
        .windows()
        .into_iter()
        .filter_map(|window| {
            let handle = window.downcast::<Root>()?;
            let root = handle.read(cx).ok()?;
            crate::shared::lib::diagnostics::content(root, cx)
                .downcast::<WorkspaceHost>()
                .ok()
        })
        .collect();
    hosts
        .into_iter()
        .flat_map(|host| host.update(cx, |host, cx| host.shutdown_terminals(cx)))
        .collect()
}

pub(crate) fn init_windows(runtime: Handle, cwd: PathBuf, compact: bool, cx: &mut App) {
    cx.set_global(Launch {
        runtime,
        cwd,
        compact,
    });
    cx.set_global(Windows::default());
}

pub(crate) fn register(target: Target, handle: WindowHandle<Root>, cx: &mut App) {
    if !cx.has_global::<Windows>() {
        cx.set_global(Windows::default());
    }
    cx.update_global::<Windows, _>(|windows, _| {
        windows.0.insert(target, handle);
    });
    cx.update_global::<crate::entities::agent::Changes, _>(|changes, _| {
        changes.target = Some(handle.window_id());
        changes.server = None;
        changes.select = None;
        changes.ready = None;
    });
}

pub(crate) fn window_options(compact: bool, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            gds::ui::window::fit_size(crate::pages::workspace::initial_size(compact), cx),
            cx,
        ))),
        kind: WindowKind::Normal,
        window_background: cx
            .global::<crate::shared::lib::appearance::Appearance>()
            .background,
        app_id: Some("io.github.GanDesktop.GdsAgents".into()),
        ..gds::ui::window::window_options()
    }
}

/// Opens or activates this backend's window without repurposing another backend's drafts.
///
/// # Errors
/// Returns a missing-runtime or platform window error without changing management context.
pub(crate) fn open_for(target: Target, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let launch = cx
        .try_global::<Launch>()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("The workspace runtime is unavailable"))?;
    if let Some(handle) = cx
        .try_global::<Windows>()
        .and_then(|windows| windows.0.get(&target))
        .copied()
        && let Ok(result) = handle.update(cx, |root, window, cx| {
            if let Ok(host) =
                crate::shared::lib::diagnostics::content(root, cx).downcast::<WorkspaceHost>()
            {
                host.update(cx, |host, cx| host.select(target.clone(), window, cx))?;
            }
            window.activate_window();
            anyhow::Ok(())
        })
    {
        result?;
        register(target, handle, cx);
        return Ok(handle);
    }
    let handle = cx.open_window(window_options(launch.compact, cx), |window, cx| {
        let view = cx.new(|cx| {
            crate::pages::workspace::Workspace::new(
                target.clone(),
                launch.cwd,
                launch.compact,
                &launch.runtime,
                window,
                cx,
            )
        });
        let view = workspace_view(view, window, cx);
        cx.new(|cx| {
            crate::shared::lib::appearance::window_root(view, window, cx).bg(transparent_black())
        })
    })?;
    register(target, handle, cx);
    Ok(handle)
}

pub(super) fn switch_in(
    target: Target,
    handle: gpui_kit::AnyWindowHandle,
    cx: &mut App,
) -> anyhow::Result<()> {
    let handle = handle
        .downcast::<Root>()
        .ok_or_else(|| anyhow::anyhow!("The workspace window is unavailable"))?;
    handle.update(cx, |root, window, cx| {
        let host = crate::shared::lib::diagnostics::content(root, cx)
            .downcast::<WorkspaceHost>()
            .map_err(|_| anyhow::anyhow!("This window does not contain a workspace"))?;
        host.update(cx, |host, cx| host.select(target.clone(), window, cx))
    })??;
    register(target, handle, cx);
    Ok(())
}
