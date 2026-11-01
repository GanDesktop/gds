use crate::entities::backend::shared;
use bas::service::tls::ConnectionOptions as Target;
use gpui_kit::{App, BorrowAppContext};

pub(crate) fn current(cx: &App) -> Option<Target> {
    cx.try_global::<crate::entities::agent::catalog::Service>()
        .map(|service| service.address())
}

/// Selects a saved backend in the originating workspace window.
///
/// # Errors
/// Returns configuration or window errors without retargeting an existing workspace.
pub(crate) fn choose(
    id: &str,
    window: gpui_kit::AnyWindowHandle,
    cx: &mut App,
) -> Result<(), String> {
    let profiles = shared(cx);
    let target = profiles.update(cx, |profiles, cx| profiles.select(id, cx))?;
    super::switch_in(target.clone(), window, cx).map_err(|error| error.to_string())?;
    manage(target, cx);
    Ok(())
}

/// Changes only the settings management context, never an existing chat's target.
pub(crate) fn manage(target: Target, cx: &mut App) {
    let Some(service) = cx
        .try_global::<crate::entities::agent::catalog::Service>()
        .cloned()
    else {
        return;
    };
    if service.address() != target {
        crate::entities::agent::init(target, service.runtime().clone(), cx);
        cx.update_global::<crate::entities::agent::Changes, _>(|changes, _| {
            changes.target = None;
            changes.select = None;
            changes.ready = None;
            changes.server = None;
        });
    }
}
