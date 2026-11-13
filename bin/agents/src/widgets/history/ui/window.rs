//! One independent history window per daemon, with its own connection lifetime.
use gds::app::AppExt as _;

use super::*;
use crate::shared::i18n::tr;
use crate::shared::lib::appearance as theme;
use crate::shared::ui::window_controls::TitleBar;
use gds_uim::el;
use gpui_kit::{
    AppContext, Bounds, ClickEvent, Global, Render, TestSupportExt, WeakEntity, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, actions,
    component::{ActiveTheme, Icon, Root},
    div,
    prelude::*,
    px, size,
};

const WIDTH: f32 = 1_000.;
const HEIGHT: f32 = 720.;

actions!(history_window, [CloseWindow]);

#[derive(Clone)]
struct OpenWindow {
    handle: WindowHandle<Root>,
    browser: WeakEntity<Browser>,
}

#[derive(Default)]
struct Windows(BTreeMap<bas::service::tls::ConnectionOptions, OpenWindow>);
impl Global for Windows {}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([gpui_kit::KeyBinding::new(
        if cfg!(target_os = "macos") {
            "cmd-w"
        } else {
            "ctrl-w"
        },
        CloseWindow,
        Some("BasAgentHistory"),
    )]);
}

/// # Errors
/// Reports a native window creation failure.
pub(crate) fn open(request: OpenRequest, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    if let Some(handle) = crate::features::navigation::show_history(request.clone(), cx)? {
        return Ok(handle);
    }
    if let Some(handle) = reopen(request.clone(), cx) {
        return Ok(handle);
    }
    let scope = cx.gds().tasks().scope();
    let (connection, events) = Connection::new(request.server.clone(), &scope);
    open_connected(request, connection, events, cx)
}

pub(crate) fn content(
    request: OpenRequest,
    window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<Entity<Browser>> {
    let scope = cx.gds().tasks().scope();
    let (connection, events) = Connection::new(request.server.clone(), &scope);
    Ok(connected_content(request, connection, events, window, cx))
}

pub(crate) fn connected_content(
    request: OpenRequest,
    connection: Connection,
    events: tokio::sync::mpsc::Receiver<Update>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Browser> {
    let browser = cx.new(|cx| Browser::new(request, connection, events, window, cx));
    focus(&browser, window, cx);
    browser
}

pub(crate) fn focus(browser: &Entity<Browser>, window: &mut Window, cx: &mut App) {
    browser.update(cx, |browser, cx| {
        browser
            .search
            .update(cx, |search, cx| search.focus(window, cx))
    });
}

fn reopen(request: OpenRequest, cx: &mut App) -> Option<WindowHandle<Root>> {
    let open = cx.try_global::<Windows>()?.0.get(&request.server)?.clone();
    open.handle
        .update(cx, |_, window, cx| {
            open.browser
                .update(cx, |browser, cx| browser.configure(request, cx))?;
            window.activate_window();
            Ok::<_, anyhow::Error>(())
        })
        .ok()?
        .ok()?;
    Some(open.handle)
}

/// # Errors
/// Returns a platform error without changing any conversation or draft.
pub(crate) fn open_connected(
    request: OpenRequest,
    connection: Connection,
    events: tokio::sync::mpsc::Receiver<Update>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Root>> {
    if let Some(handle) = reopen(request.clone(), cx) {
        return Ok(handle);
    }
    let server = request.server.clone();
    let mut browser = None;
    let handle = cx.open_window(window_options(cx), |window, cx| {
        window.set_window_title(tr!("agent-history-title"));
        let history = connected_content(request, connection, events, window, cx);
        browser = Some(history.downgrade());
        history.update(cx, |history, cx| {
            history
                .search
                .update(cx, |search, cx| search.focus(window, cx))
        });
        let host = cx.new(|_| Host { browser: history });
        window.activate_window();
        cx.new(|cx| theme::window_root(host, window, cx))
    })?;
    if !cx.has_global::<Windows>() {
        cx.set_global(Windows::default());
    }
    cx.update_global::<Windows, _>(|windows, _| {
        windows.0.insert(
            server,
            OpenWindow {
                handle,
                browser: browser.expect("history window created its browser"),
            },
        );
    });
    Ok(handle)
}

fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        kind: WindowKind::Floating,
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            gds::ui::window::fit_size(size(px(WIDTH), px(HEIGHT)), cx),
            cx,
        ))),
        window_background: cx.global::<theme::Appearance>().background,
        app_id: Some("io.github.GanDesktop.GdsAgents".into()),
        ..TitleBar::window_options()
    }
}

#[cfg(all(test, feature = "gui-tests"))]
pub(super) fn browser(
    server: bas::service::tls::ConnectionOptions,
    cx: &App,
) -> WeakEntity<Browser> {
    cx.global::<Windows>().0[&server].browser.clone()
}

struct Host {
    browser: Entity<Browser>,
}

impl Host {
    fn close(&mut self, _: &ClickEvent, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }
    fn close_action(&mut self, _: &CloseWindow, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(tr!("agent-history-title"));
        el! {
            .div {
                # "agent-history-window";
                test_support;
                key_context: "BasAgentHistory";
                size_full;
                flex;
                flex_col;
                bg: $(cx.gds().appearance().theme().content_background());
                text_color: $(cx.theme().foreground);
                on_action: $(cx.listener(Self::close_action));
            } [
                .TitleBar::default {
                    on_close_window: $(cx.listener(Self::close));
                } [
                    .div {
                        flex;
                        items_center;
                        gap_2;
                    } [
                        (ICON).Icon::new {
                            size: crate::shared::ui::ICON.px;
                        },
                        tr!("agent-history-title"),
                    ],
                ],
                .div {
                    flex_1;
                    min_h_0;
                } [
                    $(self.browser.clone()),
                ],
            ]
        }
    }
}
