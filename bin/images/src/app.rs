//! Native viewer startup. Empty screenshot pipelines exit before creating a window.
mod cli;
mod config;

use std::process::ExitCode;

use clap::Parser;
use gds::{
    app::Gds,
    appearance::{theme, typography},
    i18n::{Catalog, Language},
    service::{
        logs,
        task_manager::{Scope, Supervisor},
    },
};
#[cfg(unix)]
use tokio::sync::mpsc;
use zgui::prelude::*;

use crate::{entities::preferences, pages::workspace, shared::i18n};

pub fn run() -> ExitCode {
    match launch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gds-images: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn launch() -> anyhow::Result<()> {
    let config::StartupOutcome::Launch(config) = config::resolve_startup(&cli::Arguments::parse())?
    else {
        return Ok(());
    };
    let logs = logs::init_tracing("warn,gds=debug,gds_images=debug", config.enable_logs)?;
    let supervisor = Supervisor::new(config.enable_logs)?;
    let tasks = supervisor.scope();
    let mut startup = preferences::read_startup()?;
    preferences::normalize(&mut startup.preferences);
    supervisor.block_on(startup.preferences.appearance.prime_session())?;
    let language = Language::detect();
    let catalog = Catalog::new(language, &[i18n::resource_for(language)])?;
    let title = if config.is_capture {
        "app-capture-title"
    } else {
        "app-title"
    };
    let app_id = config.app_id;
    let outcome = zgui::app()
        .without_primary_window()
        .with_application_id(app_id)
        .with_context(move || {
            let gds = Gds::install_process_with_logs(tasks.clone(), logs);
            bind_signals(&tasks);
            gds.i18n().install_catalog(catalog);
            if config.enable_logs {
                let app_name = if config.is_capture {
                    "GDS Images (Capture)"
                } else {
                    "GDS Images"
                };
                gds::ui::logs::init(gds::ui::logs::Options {
                    app_id: Some(app_id.into()),
                    app_name: app_name.into(),
                    ignored_contexts: &["Input", "textarea", "input"],
                });
            }

            let mut notices = Vec::new();
            if let Some(error) = startup.notice.take() {
                notices.push(gds::t_untracked!("settings-load-failed", error = error));
            }
            let prefs = &mut startup.preferences;
            apply_theme(&gds, &mut prefs.appearance, &mut notices);
            gds.appearance()
                .theme()
                .configure_hot_reload(prefs.appearance.clone());
            apply_typography(&gds, &prefs.font, &mut notices);
            gds.appearance()
                .motion()
                .set_enabled(prefs.animations_enabled.unwrap_or(true));
            gds.appearance().motion().follow_system();

            let preferences = preferences::Service::install(startup, &tasks);
            let options =
                gds::ui::window::workspace_options(gds::i18n::text_untracked(title), Some(app_id));
            use_windows().open(options, move || {
                workspace::mount(
                    config.mode.clone(),
                    config.workflow.clone(),
                    config.is_capture,
                    preferences.clone(),
                    notices.clone(),
                )
            });
        })
        .run(|| ());
    supervisor.shutdown();
    outcome.map_err(|err| anyhow::anyhow!(err.to_string()))
}

fn bind_signals(tasks: &Scope) {
    #[cfg(unix)]
    {
        let (tx, rx) = mpsc::channel(1);
        zgui::tokio::spawn_receiver(rx, |()| {
            if let Some(gds) = Gds::try_current() {
                gds.tasks().request_quit();
            } else if let Some(windows) = try_use_windows() {
                windows.quit();
            }
        });
        let cancel = tasks.token();
        let watcher = tasks.spawn_named("Listen for termination signals", async move {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let mut interrupt =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
            enum Event {
                Cancelled,
                Terminate,
            }
            let event = tokio::select! {
                _ = cancel.cancelled() => Event::Cancelled,
                _ = terminate.recv() => Event::Terminate,
                _ = interrupt.recv() => Event::Terminate,
            };
            if matches!(event, Event::Terminate) {
                let _ = tx.send(()).await;
            }
            Ok::<_, std::io::Error>(())
        });
        zgui::task::spawn_local(async move {
            if let Err(error) = watcher
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
            {
                tracing::error!(%error, "cannot listen for termination signals");
            }
        });
    }
    #[cfg(not(unix))]
    let _ = tasks;
}

fn apply_theme(gds: &Gds, appearance: &mut theme::Appearance, notices: &mut Vec<String>) {
    let theme = gds.appearance().theme();
    let Err(error) = theme.apply(appearance) else {
        return;
    };
    // Missing desktop theme integration degrades quietly and updates the active runtime source;
    // an invalid user-selected theme surfaces a startup notice while keeping the stored preference
    // intact so the user can inspect or repair it.
    let unavailable_system = appearance.source == theme::Source::System;
    if unavailable_system {
        tracing::warn!(%error, "system theme unavailable; using built-in theme");
    } else {
        notices.push(error.to_string());
    }
    let fallback = appearance.builtin_fallback();
    if let Err(fallback_error) = theme.apply(&fallback) {
        tracing::error!(error = %fallback_error, "cannot initialize the built-in theme");
    } else if unavailable_system {
        *appearance = fallback;
    }
}

fn apply_typography(gds: &Gds, font: &typography::Preferences, notices: &mut Vec<String>) {
    let typography = gds.appearance().typography();
    match typography.apply(font) {
        Ok(applied) => {
            if let Some(unavailable) = applied.unavailable {
                tracing::warn!(
                    "Font family {unavailable:?} is not installed; using the system font."
                );
            }
        }
        Err(error) => {
            notices.push(error.to_string());
            // An out-of-range stored font size must not block window creation; apply the default
            // metrics for this session while leaving the stored preference untouched.
            if let Err(fallback) = typography.apply(&typography::Preferences::default()) {
                tracing::error!(error = %fallback, "cannot initialize the default font");
            }
        }
    }
}
