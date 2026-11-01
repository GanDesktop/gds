//! Navigate between workspaces and management surfaces without owning their views.
pub(crate) mod commands;
mod model;
mod request;
pub(crate) use crate::entities::backend::{Profiles, presentation, shared};
pub(crate) use model::{choose, current, manage};
pub(crate) use request::{HistoryRequest, OpenSession};

use bas::service::tls::ConnectionOptions as Target;
use gpui_kit::{AnyView, AnyWindowHandle, App, Global, WindowHandle, component::Root};

type Handle = WindowHandle<Root>;
type ViewRoute = anyhow::Result<Option<(Handle, AnyView)>>;
type WindowRoute = anyhow::Result<Option<Handle>>;

/// Native routing is installed by the application after its services are ready.
pub(crate) struct Routes {
    pub settings: fn(&mut App) -> ViewRoute,
    pub catalog: fn(Option<AnyView>, &mut App) -> WindowRoute,
    pub history: fn(HistoryRequest, &mut App) -> WindowRoute,
    pub about: fn(&mut App) -> anyhow::Result<Handle>,
    pub open_backend: fn(Target, &mut App) -> anyhow::Result<Handle>,
    pub switch_backend: fn(Target, AnyWindowHandle, &mut App) -> anyhow::Result<()>,
    pub target: fn(AnyWindowHandle, &mut App) -> bool,
}
impl Global for Routes {}

pub(crate) fn install(routes: Routes, cx: &mut App) {
    cx.set_global(routes);
}
pub(crate) fn show_settings(cx: &mut App) -> ViewRoute {
    match cx.try_global::<Routes>() {
        Some(routes) => (routes.settings)(cx),
        None => Ok(None),
    }
}
pub(crate) fn show_catalog(view: Option<AnyView>, cx: &mut App) -> WindowRoute {
    match cx.try_global::<Routes>() {
        Some(routes) => (routes.catalog)(view, cx),
        None => Ok(None),
    }
}
pub(crate) fn show_history(request: HistoryRequest, cx: &mut App) -> WindowRoute {
    match cx.try_global::<Routes>() {
        Some(routes) => (routes.history)(request, cx),
        None => Ok(None),
    }
}
/// # Errors
/// Reports a native-window failure.
pub(crate) fn open_about(cx: &mut App) -> anyhow::Result<Handle> {
    match cx.try_global::<Routes>() {
        Some(routes) => (routes.about)(cx),
        None => crate::shared::ui::dialogs::open_about_window(cx),
    }
}
/// # Errors
/// Reports an uninitialized workspace router or native-window failure.
pub(crate) fn open_for(target: Target, cx: &mut App) -> anyhow::Result<Handle> {
    let route = cx
        .try_global::<Routes>()
        .ok_or_else(|| anyhow::anyhow!("workspace navigation is not initialized"))?
        .open_backend;
    route(target, cx)
}
fn switch_in(target: Target, window: AnyWindowHandle, cx: &mut App) -> anyhow::Result<()> {
    let route = cx
        .try_global::<Routes>()
        .ok_or_else(|| anyhow::anyhow!("workspace navigation is not initialized"))?
        .switch_backend;
    route(target, window, cx)
}
pub(crate) fn target_page(window: AnyWindowHandle, cx: &mut App) -> bool {
    cx.try_global::<Routes>()
        .map(|routes| routes.target)
        .is_some_and(|route| route(window, cx))
}
