//! Initialize the runtime, services and first Agents window.
use gds::app::AppExt as _;

use crate::app::cli::Args;
use crate::entities::agent as agents;
use crate::features::agent_auth as authentication;
use crate::features::navigation as routing;
use crate::pages::workspace;
use crate::shared::api::http;
use crate::shared::config as configuration;
use crate::shared::lib::appearance as theme;
use crate::shared::ui::assets;
use crate::widgets::history;
use crate::widgets::settings;
use anyhow::{Context as _, Result};
use clap::Parser;
use gpui_kit::{AppContext, prelude::*, transparent_black};

pub(crate) fn run() -> Result<()> {
    authentication::terminal::run_helper_if_requested();
    let args = Args::parse();
    let logs = gds::service::logs::init_tracing(
        "warn,gds=debug,gds_agents=debug,bas::service::client::history=debug",
        args.enable_logs,
    )?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        build = env!("GDS_BUILD_TAG"),
        "Frontend started"
    );
    let supervisor = gds::service::task_manager::Supervisor::new(args.enable_logs)
        .context("cannot start the application task supervisor")?;
    let tasks = supervisor.scope();
    let cwd = args
        .cwd
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    gpui_kit::application()
        .with_quit_mode(gpui_kit::QuitMode::Explicit)
        .with_http_client(supervisor.block_on_named("Initialize HTTP client", http::client(&tasks))??)
        .with_assets(assets::Assets)
        .run(move |cx| {
            cx.gds().initialize(tasks.clone(), logs);
            cx.gds().appearance().motion().follow_system();
            configuration::init(args.settings_file, cx);
            let profiles = routing::shared(cx);
            let server = match profiles.read(cx).startup_target(args.server) {
                Ok(server) => server,
                Err(error) => {
                    tracing::error!(%error, "cannot select frontend connection; refusing to fall back");
                    cx.gds().tasks().request_quit();
                    return;
                }
            };
            agents::init(server.clone(), tasks.clone(), cx);
            agents::metadata::init(server.clone(), tasks.clone(), cx);
            workspace::windows::init_windows(tasks.clone(), cwd.clone(), args.compact, cx);
            theme::init(cx);
            theme::watch(cx);
            super::init(cx);
            #[cfg(unix)]
            cx.gds().tasks().signals(super::request_quit);
            workspace::bind_keys(cx);
            settings::window::bind_keys(cx);
            history::window::bind_keys(cx);
            cx.on_window_closed(|cx, _| {
                cx.gds().tasks().quit_when_idle();
            })
            .detach();
            cx.gds().tasks().ready(false);
            let options = workspace::windows::window_options(args.compact, cx);
            let opened = cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| {
                    workspace::Workspace::new(
                        server.clone(),
                        cwd,
                        args.compact,
                        &tasks,
                        window,
                        cx,
                    )
                });
                let view = workspace::windows::workspace_view(view, window, cx);
                cx.new(|cx| {
                    theme::window_root(view, window, cx)
                                                .bg(transparent_black())
                })
            });
            match opened {
                Ok(window) => workspace::windows::register(server, window, cx),
                Err(error) => {
                    tracing::error!(%error, "cannot open the Agents window");
                    cx.gds().tasks().request_quit();
                }
            }
        });
    Ok(())
}
