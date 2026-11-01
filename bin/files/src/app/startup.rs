//! Install preferences and open the first Files workspace or picker window.

use crate::app::cli;
use crate::entities::location::places::Places;
use crate::entities::preferences as configuration;
use crate::features::file_operations::commands as actions;
use crate::features::pick_files as picker;
use crate::pages::workspace;
use crate::shared::i18n;
use crate::shared::ui::assets;
use clap::Parser;
use std::process::ExitCode;

use anyhow::Context as _;
use directories::BaseDirs;
use fluent_bundle::FluentArgs;
use gds::{
    appearance::{theme, typography},
    ui::time,
};
use gpui_kit::*;

const IO_THREADS: usize = 2;
const APP_ID: &str = "io.github.GanDesktop.GdsFiles";
const PICKER_APP_ID: &str = "io.github.GanDesktop.GdsFiles.Picker";
const PICKER_WIDTH: f32 = 1100.;
const PICKER_HEIGHT: f32 = 740.;
const INITIAL_WIDTH: f32 = 1280.;
const INITIAL_HEIGHT: f32 = 820.;
/// The window has to hold the sidebar column, the file content and the details
/// panel side by side; below this they would collide in the toolbar and the
/// content area. The dialog and column layouts do not shrink past these.
const MIN_WIDTH: f32 = 1000.;
const MIN_HEIGHT: f32 = 640.;

/// # Errors
/// Reports invalid arguments or unavailable user/current directories.
pub(crate) fn run() -> anyhow::Result<ExitCode> {
    let arguments = cli::Arguments::parse();
    let picking = arguments.is_picker();
    match launch(arguments) {
        Ok(()) if picking => Ok(ExitCode::from(picker::exit_code())),
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            eprintln!("gds-files: {error:#}");
            if picking {
                picker::mark_failed();
                return Ok(ExitCode::from(picker::exit_code()));
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

/// # Errors
/// Reports unavailable user/current directories.
fn launch(arguments: cli::Arguments) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(IO_THREADS)
        .thread_name("gds-io")
        .enable_all()
        .build()
        .context("cannot start the application I/O runtime")?;
    let handle = runtime.handle().clone();
    let logs =
        gds::service::logs::init_tracing("warn,gds=debug,gds_files=debug", arguments.enable_logs)?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        build = env!("GDS_BUILD_TAG"),
        "Files started"
    );
    let home = BaseDirs::new()
        .context("cannot determine the home directory")?
        .home_dir()
        .to_path_buf();
    let picker_options = if arguments.portal_request {
        Some(picker::Options::from_request(serde_json::from_reader(
            std::io::stdin().lock(),
        )?)?)
    } else {
        arguments.picker()
    };
    let suggested = picker_options
        .as_ref()
        .and_then(|options| options.portal.as_ref())
        .and_then(|request| {
            request
                .current_file
                .as_ref()
                .map(std::path::PathBuf::from)
                .and_then(|file| file.parent().map(std::path::Path::to_owned))
                .or_else(|| {
                    request
                        .current_folder
                        .as_ref()
                        .map(std::path::PathBuf::from)
                })
        })
        .filter(|path| path.is_absolute() && path.is_dir());
    let mut start = match arguments.directory.or(suggested) {
        Some(path) if path.is_absolute() => path,
        Some(path) => std::env::current_dir()?.join(path),
        None => home.clone(),
    };
    let picking = picker_options.is_some();
    #[cfg(target_os = "linux")]
    let mut manager_receiver = None;
    #[cfg(target_os = "linux")]
    let mut initial_command = None;
    #[cfg(target_os = "linux")]
    let mut manager_notice = None;
    #[cfg(target_os = "linux")]
    let _manager = if !picking {
        use gds::service::desktop::file_manager::{Error, Server};
        let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
        match runtime.block_on(Server::connect(APP_ID, sender)) {
            Ok(server) => {
                if let Some(provider) = &server.provider_conflict {
                    manager_notice = Some(format!(
                        "org.freedesktop.FileManager1 已由 {provider} 提供；Files 未接管该接口。"
                    ));
                }
                if arguments.dbus_service {
                    initial_command = runtime.block_on(receiver.recv());
                    if let Some(target) = initial_command
                        .as_ref()
                        .and_then(|command| command.targets.first())
                    {
                        start = target.directory.clone();
                    }
                }
                manager_receiver = Some(receiver);
                Some(server)
            }
            Err(Error::AlreadyRunning) => {
                if !arguments.dbus_service {
                    let startup_id = std::env::var("XDG_ACTIVATION_TOKEN")
                        .or_else(|_| std::env::var("DESKTOP_STARTUP_ID"))
                        .unwrap_or_default();
                    runtime.block_on(Server::open_existing(APP_ID, &start, &startup_id))?;
                }
                return Ok(());
            }
            Err(error) => {
                manager_notice = Some(error.to_string());
                None
            }
        }
    } else {
        None
    };
    let (preferences, migration_error, mut startup_error) = match configuration::load_startup() {
        Ok(loaded) => (loaded.preferences, loaded.migration, None),
        Err(error) => (
            configuration::Preferences::default(),
            None,
            Some(error.to_string()),
        ),
    };
    #[cfg(target_os = "linux")]
    if let Some(message) = manager_notice {
        tracing::warn!(%message, "file-manager integration unavailable");
        startup_error = Some(match startup_error.take() {
            Some(previous) => format!("{previous}\n{message}"),
            None => message,
        });
    }
    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gds::appearance::init(cx);
            if let Some(logs) = logs {
                gds::ui::logs::install(logs, cx);
                gds::ui::logs::init(
                    gds::ui::logs::Options {
                        app_id: Some(APP_ID.into()),
                        app_name: "GDS Files".into(),
                        ignored_contexts: &[
                            crate::features::file_operations::commands::TERMINAL_TABS_CONTEXT,
                        ],
                        ..Default::default()
                    },
                    cx,
                );
            }
            cx.set_app_identity(APP_ID, "GDS Files");
            if let Err(error) = i18n::install(
                preferences.language.unwrap_or_else(i18n::Language::detect),
                cx,
            ) {
                tracing::error!("Cannot load application messages: {error}");
                if picking {
                    picker::mark_failed();
                }
                cx.quit();
                return;
            }
            time::apply(preferences.time_format, cx);
            if let Err(error) = theme::apply(&preferences.appearance, cx) {
                startup_error = Some(match startup_error.take() {
                    Some(previous) => format!("{previous}\n{error}"),
                    None => error.to_string(),
                });
                let fallback = preferences.appearance.builtin_fallback();
                if let Err(error) = theme::apply(&fallback, cx) {
                    tracing::error!("Cannot initialize the built-in theme: {error}");
                    if picking {
                        picker::mark_failed();
                    }
                    cx.quit();
                    return;
                }
            }
            match typography::install(&preferences.font, cx) {
                Ok(applied) => {
                    if let Some(unavailable) = applied.unavailable {
                        tracing::error!(
                            "Font family {unavailable:?} is not installed; using the system font."
                        );
                    }
                }
                Err(error) => {
                    startup_error = Some(match startup_error.take() {
                        Some(previous) => format!("{previous}\n{error}"),
                        None => error.to_string(),
                    });
                    // An unusable stored size must not stop the window from
                    // opening, and the stored configuration is left untouched.
                    if let Err(fallback) =
                        typography::install(&typography::Preferences::default(), cx)
                    {
                        tracing::error!("Cannot initialize the default font: {fallback}");
                    }
                }
            }
            actions::register(cx);
            gds::ui::terminal::init(cx);
            cx.on_app_quit(|_| async { gds::service::terminal::shutdown_all().await })
                .detach();
            if let Some(error) = migration_error {
                let mut arguments = FluentArgs::new();
                arguments.set("error", error.to_string());
                let message = i18n::format("settings-migrate-failed", Some(&arguments), cx);
                startup_error = Some(match startup_error.take() {
                    Some(previous) => format!("{previous}\n{message}"),
                    None => message.to_string(),
                });
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let dimensions = if picking {
                size(px(PICKER_WIDTH), px(PICKER_HEIGHT))
            } else {
                size(px(INITIAL_WIDTH), px(INITIAL_HEIGHT))
            };
            let bounds = Bounds::centered(None, dimensions, cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
                app_id: Some(if picking { PICKER_APP_ID } else { APP_ID }.to_owned()),
                kind: if picking && picker_options.as_ref().and_then(|options| options.portal.as_ref()).is_none_or(|request| request.modal) {
                    WindowKind::Dialog
                } else if picking {
                    WindowKind::Floating
                } else {
                    WindowKind::Normal
                },
                ..gds::ui::window::window_options()
            };
            cx.spawn(async move |cx| {
                let places = tokio_util::task::AbortOnDropHandle::new(handle.spawn(Places::load()))
                    .await
                    .unwrap_or_else(|error| Err(error.into()));
                cx.update(move |cx| {
                    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                        let request = picker_options.as_ref().and_then(|options| options.portal.as_ref());
                        let title = request.map(|request| request.title.as_str()).filter(|title| !title.is_empty());
                        let default_title = i18n::text("app-title", cx);
                        window.set_window_title(title.unwrap_or(default_title.as_ref()));
                        if let Some(request) = request.filter(|request| !request.parent_window.is_empty()) {
                            if let Err(error) = window.set_external_parent(&request.parent_window) {
                                tracing::warn!(parent = %request.parent_window, %error, "cannot associate chooser with its parent");
                            }
                        }
                        let launch = workspace::LaunchOptions {
                            runtime: handle,
                            places,
                            start,
                            home,
                            preferences,
                            startup_error,
                            picker: picker_options,
                        };
                        let browser = cx.new(|cx| workspace::Browser::new(launch, window, cx));
                        #[cfg(target_os = "linux")]
                        if let Some(command) = initial_command {
                            browser.update(cx, |browser, cx| browser.apply_desktop_command(command, window, cx));
                        }
                        browser
                    });
                    if let Ok((window, _)) = &opened {
                        let main_window = window.window_id();
                        cx.on_window_closed(move |cx, closed| { if closed == main_window { cx.quit(); } }).detach();
                    }
                    #[cfg(target_os = "linux")]
                    if let (Ok((window, browser)), Some(mut receiver)) = (&opened, manager_receiver) {
                        let window = *window;
                        let browser = browser.downgrade();
                        cx.spawn(async move |cx| {
                            while let Some(command) = receiver.recv().await {
                                let Some(browser) = browser.upgrade() else { break };
                                if let Err(error) = window.update(cx, |_, window, cx| {
                                    browser.update(cx, |browser, cx| browser.apply_desktop_command(command, window, cx));
                                }) { tracing::warn!(%error, "Files window closed before desktop request"); break; }
                            }
                        }).detach();
                    }
                    if let Err(error) = opened {
                        tracing::error!("Cannot open the Files window: {error}");
                        if picking {
                            picker::mark_failed();
                        }
                        cx.quit();
                    }
                });
            })
            .detach();
        });
    Ok(())
}
