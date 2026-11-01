//! Agent installation content; a standalone host also supports isolated previews.
use super::{Agents, Editor, Page};
use crate::shared::i18n::tr;
use crate::shared::lib::appearance as theme;
use crate::shared::ui::window_controls::TitleBar;
use gds_uim::el;
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Global, Render, Subscription, Window,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, component::Root, div, prelude::*, px,
    size,
};

#[derive(Default)]
struct OpenWindow(
    std::collections::HashMap<Option<bas::service::tls::ConnectionOptions>, WindowHandle<Root>>,
);
impl Global for OpenWindow {}

const WIDTH: f32 = 900.;
const HEIGHT: f32 = 720.;
const MIN_WIDTH: f32 = 640.;
const MIN_HEIGHT: f32 = 480.;

gpui_kit::actions!(agent_installation_window, [CloseWindow]);

/// # Errors
/// Returns a platform error if the onboarding window cannot be created.
pub(in crate::widgets::settings) fn open_catalog(
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Root>> {
    open_inner(None, cx)
}

/// # Errors
/// Returns a platform error if the onboarding window cannot be created.
pub(in crate::widgets::settings) fn open(
    agents: Entity<Agents>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Root>> {
    open_inner(Some(agents), cx)
}

/// # Errors
/// Returns a platform error when native window creation fails.
fn open_inner(agents: Option<Entity<Agents>>, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    if let Some(handle) =
        crate::features::navigation::show_catalog(agents.clone().map(Into::into), cx)?
    {
        return Ok(handle);
    }
    let target = agents.as_ref().map_or_else(
        || super::selected_target(cx),
        |agents| agents.read(cx).target(),
    );
    if let Some(handle) = current_for(target.clone(), cx)
        && cx.windows().contains(&handle.into())
    {
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        });
        return Ok(handle);
    }
    cx.bind_keys([gpui_kit::KeyBinding::new(
        if cfg!(target_os = "macos") {
            "cmd-w"
        } else {
            "ctrl-w"
        },
        CloseWindow,
        Some("BasAgentInstallation"),
    )]);
    let handle = cx.open_window(
        WindowOptions {
            kind: WindowKind::Floating,
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(WIDTH), px(HEIGHT)),
                cx,
            ))),
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            window_background: cx.global::<theme::Appearance>().background,
            app_id: Some("io.github.GanDesktop.GdsAgents".into()),
            ..TitleBar::window_options()
        },
        |window, cx| {
            let agents = agents.unwrap_or_else(|| super::shared(window, cx));
            let host = cx.new(|cx| {
                let observation = cx.observe(&agents, |_, _, cx| cx.notify());
                let weak = agents.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    let _ = weak.update(cx, |agents, cx| {
                        cancel_draft(agents, cx);
                    });
                    true
                });
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                Host {
                    agents,
                    focus,
                    _observation: observation,
                }
            });
            cx.new(|cx| theme::window_root(host, window, cx))
        },
    )?;
    if !cx.has_global::<OpenWindow>() {
        cx.set_global(OpenWindow::default());
    }
    cx.update_global::<OpenWindow, _>(|state, _| {
        state.0.insert(target, handle);
    });
    Ok(handle)
}

pub(super) fn current_for(
    target: Option<bas::service::tls::ConnectionOptions>,
    cx: &App,
) -> Option<WindowHandle<Root>> {
    cx.try_global::<OpenWindow>()
        .and_then(|state| state.0.get(&target).copied())
}

struct Host {
    agents: Entity<Agents>,
    focus: FocusHandle,
    _observation: Subscription,
}

pub(crate) struct Content {
    agents: Entity<Agents>,
    _observation: Subscription,
}

pub(crate) fn content(
    agents: Option<Entity<Agents>>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Content> {
    let agents = agents.unwrap_or_else(|| super::shared(window, cx));
    cx.new(|cx| Content {
        _observation: cx.observe(&agents, |_, _, cx| cx.notify()),
        agents,
    })
}

impl Content {
    pub(crate) fn prepare_to_leave(&self, cx: &mut App) {
        self.agents.update(cx, cancel_draft);
    }
}

impl Render for Content {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.agents.update(cx, |agents, cx| {
            agents.render_page(Page::Onboarding, window, cx)
        })
    }
}

fn cancel_draft(agents: &mut Agents, cx: &mut Context<Agents>) {
    if matches!(agents.editor, Editor::Add(_)) {
        agents.editor = Editor::Idle;
        agents.message.clear();
        cx.notify();
    }
}

impl Host {
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agents.update(cx, cancel_draft);
        window.remove_window();
    }

    fn on_close_click(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close(window, cx);
    }

    fn on_close_action(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(tr!("agent-install-action"));
        let content = self.agents.update(cx, |agents, cx| {
            agents.render_page(Page::Onboarding, window, cx)
        });
        el! {
            .div {
                size_full;
                flex;
                flex_col;
                key_context: "BasAgentInstallation";
                track_focus: $(&self.focus);
                on_action: $(cx.listener(Self::on_close_action));
            } [
                .TitleBar::default {
                    on_close_window: $(cx.listener(Self::on_close_click));
                } [
                    tr!("agent-install-action"),
                ],
                .div {
                    flex_1;
                    min_h_0;
                } [
                    content,
                ],
            ]
        }
    }
}
