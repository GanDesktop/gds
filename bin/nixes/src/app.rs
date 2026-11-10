use clap::Parser;
use gds::appearance::{theme, typography};
use gpui_kit::*;

#[derive(Parser)]
#[command(
    name = "gds-nixes",
    about = "Browse flake-based Home Manager configurations"
)]
struct Arguments {
    /// Local directory containing flake.nix.
    #[arg(long)]
    flake: Option<String>,
    #[arg(long)]
    enable_logs: bool,
}

pub fn run() -> anyhow::Result<()> {
    let args = Arguments::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();
    let logs = gds::service::logs::init_tracing("warn", args.enable_logs)?;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gds::appearance::init(cx);
            cx.set_app_identity("io.github.GanDesktop.GdsNixes", "GDS Nixes");
            if let Err(error) = crate::shared::i18n::install(cx) {
                eprintln!("{error}");
                cx.quit();
                return;
            }
            if let Err(error) = theme::apply(&theme::Appearance::default(), cx) {
                eprintln!("{error}");
            }
            if let Err(error) = typography::install(&typography::Preferences::default(), cx) {
                eprintln!("{error}");
            }
            if let Some(logs) = logs {
                gds::ui::logs::install(logs, cx);
                gds::ui::logs::init(
                    gds::ui::logs::Options {
                        app_id: Some("io.github.GanDesktop.GdsNixes".into()),
                        app_name: "GDS Nixes".into(),
                        ..Default::default()
                    },
                    cx,
                );
            }
            gds::ui::task_manager::init(cx);
            #[cfg(unix)]
            gds::ui::task_manager::signals(&handle, gds::ui::task_manager::request_quit, cx);
            cx.on_window_closed(|cx, _| {
                gds::ui::task_manager::quit_when_idle(cx);
            })
            .detach();
            let options = WindowOptions {
                app_id: Some("io.github.GanDesktop.GdsNixes".into()),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    gds::ui::window::fit_size(size(px(1120.), px(760.)), cx),
                    cx,
                ))),
                window_min_size: Some(size(px(800.), px(520.))),
                ..gds::ui::window::window_options()
            };
            if let Err(error) = gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("GDS Nixes");
                cx.new(|cx| crate::pages::home::HomePage::new(handle, args.flake, window, cx))
            }) {
                eprintln!("{error}");
                cx.quit();
            }
        });
    Ok(())
}
