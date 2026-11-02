//! Initialize the runtime, services and first Agents window.

use crate::app::cli::Args;
use crate::entities::agent as agents;
use crate::features::agent_auth as authentication;
use crate::features::navigation as routing;
use crate::pages::workspace;
use crate::shared::api::http;
use crate::shared::config as configuration;
use crate::shared::lib::appearance as theme;
use crate::shared::lib::diagnostics;
use crate::shared::ui::assets;
use crate::widgets::history;
use crate::widgets::settings;
use anyhow::{Context as _, Result};
use clap::Parser;
use gpui_kit::{AppContext, prelude::*, transparent_black};

const IO_THREADS: usize = 2;

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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(IO_THREADS)
        .thread_name("gds-io")
        .enable_all()
        .build()
        .context("cannot start the application I/O runtime")?;
    let handle = runtime.handle().clone();
    let cwd = args
        .cwd
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    gpui_kit::application()
        .with_http_client(http::client(&handle)?)
        .with_assets(assets::Assets)
        .run(move |cx| {
            gds::appearance::init(cx);
            gds::appearance::motion::follow_system(&handle, cx);
            if let Some(logs) = logs { diagnostics::init(logs, cx); }
            configuration::init(args.settings_file, cx);
            let profiles = routing::shared(cx);
            let server = match profiles.read(cx).startup_target(args.server) {
                Ok(server) => server,
                Err(error) => {
                    tracing::error!(%error, "cannot select frontend connection; refusing to fall back");
                    cx.quit();
                    return;
                }
            };
            agents::init(server.clone(), handle.clone(), cx);
            agents::metadata::init(server.clone(), handle.clone(), cx);
            workspace::windows::init_windows(handle.clone(), cwd.clone(), args.compact, cx);
            theme::init(cx);
            theme::watch(cx);
            super::init(cx);
            workspace::bind_keys(cx);
            settings::window::bind_keys(cx);
            history::window::bind_keys(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let options = workspace::windows::window_options(args.compact, cx);
            let opened = cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| {
                    workspace::Workspace::new(
                        server.clone(),
                        cwd,
                        args.compact,
                        &handle,
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
                    cx.quit();
                }
            }
        });
    Ok(())
}
