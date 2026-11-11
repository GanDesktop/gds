//! GUI services and shutdown hooks, driven by GPUI on the main thread.
#[cfg(target_os = "linux")]
use super::desktop;
use super::{APP_ID, ApplicationHandle, Config, Resources};
use crate::features::file_operations::commands as actions;
use crate::pages::workspace;
use crate::shared::{i18n, ui::assets};
use crate::{entities::preferences as configuration, features::pick_files as picker};
use gds::{
    appearance::{theme, typography},
    ui::time,
};

impl ApplicationHandle {
    /// Drive the main-thread GUI loop until shutdown, retaining service lifetimes.
    pub(super) fn wait(self) {
        let ApplicationHandle {
            mut config,
            resources,
            logs,
            #[cfg(target_os = "linux")]
            desktop,
        } = self;
        let Resources { runtime, storage } = resources;
        let handle = runtime.handle().clone();
        #[cfg(target_os = "linux")]
        let desktop::Desktop {
            server: _server,
            receiver: mut manager_receiver,
        } = desktop;
        let picking = config.picker.is_some();
        #[cfg(target_os = "linux")]
        let dbus_service = config.dbus_service;
        gpui_kit::application()
            .with_assets(assets::Assets)
            .run(move |cx| {
                if let Err(error) = config.initialize_ui(logs, &handle, cx) {
                    tracing::error!(%error, "Cannot initialize Files UI");
                    if picking {
                        picker::mark_failed();
                    }
                    cx.quit();
                    return;
                }
                actions::register(cx);
                gds::ui::terminal::init(cx);
                gds::ui::task_manager::init(cx);
                listen_for_shutdown(&handle, cx);
                let Config {
                    home, start, preferences, startup_error, picker: picker_options, ..
                } = config;
                configuration::publish(preferences.clone(), cx);
                let manager = crate::app::windows::Windows::install(
                    handle.clone(), home.clone(), storage.clone(), cx,
                );
                cx.spawn(async move |cx| {
                    // D-Bus activation waits for its first request while the GUI loop
                    // remains available to process shutdown signals.
                    #[cfg(target_os = "linux")]
                    let initial_command = if dbus_service {
                        match manager_receiver.as_mut() {
                            Some(receiver) => receiver.recv().await,
                            None => None,
                        }
                    } else {
                        None
                    };
                    #[cfg(target_os = "linux")]
                    let start = initial_command.as_ref()
                        .and_then(|command| command.targets.first())
                        .map(|target| target.directory.clone())
                        .unwrap_or(start);
                    let places = tokio_util::task::AbortOnDropHandle::new(
                        handle.spawn(async move { storage.load().await }),
                    )
                    .await
                    .unwrap_or_else(|error| Err(error.into()));
                    cx.update(move |cx| {
                        if gds::ui::task_manager::is_shutting_down(cx) {
                            return;
                        }
                        let opened = manager.update(cx, |manager, cx| {
                            manager.open(workspace::LaunchOptions {
                                runtime: handle, places, start, home, preferences,
                                startup_error, picker: picker_options,
                            }, cx)
                        });
                        match opened {
                            Ok((window, browser)) => {
                                #[cfg(feature = "files-e2e")]
                                if std::env::var_os("GDS_E2E_ARTIFACTS").is_some() {
                                    workspace::e2e::start(window, browser.clone(), cx);
                                }
                                #[cfg(target_os = "linux")]
                                if let Some(command) = initial_command {
                                    if let Err(error) = window.update(cx, |_, window, cx| {
                                        browser.update(cx, |browser, cx| {
                                            browser.apply_desktop_command(command, window, cx);
                                        });
                                    }) {
                                        tracing::warn!(%error, "Files window closed before initial request");
                                    }
                                }
                                #[cfg(target_os = "linux")]
                                if let Some(mut receiver) = manager_receiver {
                                    cx.spawn(async move |cx| {
                                        while let Some(command) = receiver.recv().await {
                                            manager.update(cx, |manager, cx| {
                                                manager.desktop_command(command, cx);
                                            });
                                        }
                                    }).detach();
                                }
                            }
                            Err(error) => {
                                tracing::error!(%error, "Cannot open the Files window");
                                if picking {
                                    picker::mark_failed();
                                }
                                cx.quit();
                            }
                        }
                    });
                }).detach();
            });
    }
}

impl Config {
    fn initialize_ui(
        &mut self,
        logs: Option<gds::service::logs::Store>,
        handle: &tokio::runtime::Handle,
        cx: &mut gpui_kit::App,
    ) -> anyhow::Result<()> {
        gds::appearance::init(cx);
        gds::appearance::motion::set_animations_enabled(
            self.preferences.animations_enabled.unwrap_or(true),
            cx,
        );
        gds::appearance::motion::follow_system(handle, cx);
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
            self.preferences
                .language
                .unwrap_or_else(i18n::Language::detect),
            cx,
        ) {
            tracing::error!("Cannot load application messages: {error}");
            return Err(error.into());
        }
        time::apply(self.preferences.time_format, cx);
        if let Err(error) = theme::apply(&self.preferences.appearance, cx) {
            let unavailable_system = self.preferences.appearance.source == theme::Source::System;
            if unavailable_system {
                tracing::warn!(%error, "system theme unavailable; using built-in theme");
            } else {
                self.report(error.to_string());
            }
            let fallback = self.preferences.appearance.builtin_fallback();
            if let Err(error) = theme::apply(&fallback, cx) {
                tracing::error!("Cannot initialize the built-in theme: {error}");
                return Err(error.into());
            }
            if unavailable_system {
                self.preferences.appearance = fallback;
            }
        }
        match typography::install(&self.preferences.font, cx) {
            Ok(applied) => {
                if let Some(unavailable) = applied.unavailable {
                    tracing::error!(
                        "Font family {unavailable:?} is not installed; using the system font."
                    );
                }
            }
            Err(error) => {
                self.report(error.to_string());
                // An unusable stored size must not stop the window from
                // opening, and the stored configuration is left untouched.
                if let Err(fallback) = typography::install(&typography::Preferences::default(), cx)
                {
                    tracing::error!("Cannot initialize the default font: {fallback}");
                }
            }
        }
        Ok(())
    }
}

fn listen_for_shutdown(handle: &tokio::runtime::Handle, cx: &mut gpui_kit::App) {
    #[cfg(unix)]
    gds::ui::task_manager::signals(handle, gds::ui::task_manager::request_quit, cx);
    cx.on_app_quit(|_| async { gds::service::terminal::shutdown_all().await })
        .detach();
    cx.on_window_closed(|cx, _| {
        gds::ui::task_manager::quit_when_idle(cx);
    })
    .detach();
}
