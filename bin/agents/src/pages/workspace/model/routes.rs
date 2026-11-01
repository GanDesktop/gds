use super::windows;
use crate::features::navigation::{HistoryRequest, Routes};
use crate::shared::ui::dialogs;
use gpui_kit::{AnyView, App, WindowHandle, component::Root};

pub(crate) fn routes() -> Routes {
    Routes {
        settings,
        catalog,
        history,
        about,
        open_backend: windows::open_for,
        switch_backend: windows::switch_in,
        target: windows::target_page,
    }
}
fn settings(cx: &mut App) -> anyhow::Result<Option<(WindowHandle<Root>, AnyView)>> {
    let Some((handle, host)) = windows::page_host(cx) else {
        return Ok(None);
    };
    let view = handle.update(cx, |_, window, cx| {
        host.update(cx, |host, cx| host.show_settings(window, cx))
    })?;
    Ok(Some((handle, view.into())))
}
fn catalog(view: Option<AnyView>, cx: &mut App) -> anyhow::Result<Option<WindowHandle<Root>>> {
    let Some((handle, host)) = windows::page_host(cx) else {
        return Ok(None);
    };
    let view = view.map(|view| {
        view.downcast()
            .expect("catalog routing receives an Agent-management view")
    });
    handle.update(cx, |_, window, cx| {
        host.update(cx, |host, cx| host.show_catalog(view, window, cx))
    })?;
    Ok(Some(handle))
}
fn history(request: HistoryRequest, cx: &mut App) -> anyhow::Result<Option<WindowHandle<Root>>> {
    let Some((handle, host)) = windows::page_host(cx) else {
        return Ok(None);
    };
    handle.update(cx, |_, window, cx| {
        host.update(cx, |host, cx| host.show_history(request, window, cx))
    })??;
    Ok(Some(handle))
}
fn about(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    if let Some((handle, host)) = windows::page_host(cx) {
        handle.update(cx, |_, window, cx| {
            host.update(cx, |host, cx| host.show_about(window, cx))
        })?;
        Ok(handle)
    } else {
        dialogs::open_about_window(cx)
    }
}
