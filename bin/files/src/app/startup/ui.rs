//! Files UI initialization. The application startup owns ordering and listening.
use super::{APP_ID, Config};
use crate::entities::preferences::{self, PreferencesExt as _};
use crate::shared::i18n;
use crate::shared::ui::actions;
use gds::app::AppExt as _;
use gds::{
    appearance::{theme, typography},
    ui::time,
};

pub(super) trait FilesExt {
    fn files(&mut self) -> Files<'_>;
}

impl FilesExt for gds::app::Gds<'_> {
    fn files(&mut self) -> Files<'_> {
        Files { cx: self.app() }
    }
}

/// Borrows the application context; acquiring the accessor starts no services.
pub(super) struct Files<'a> {
    cx: &'a mut gpui_kit::App,
}

impl Files<'_> {
    /// # Errors
    /// Reports unavailable translations or failure to initialize the fallback theme.
    pub fn initialize(
        &mut self,
        config: &mut Config,
        logs: Option<gds::service::logs::Store>,
    ) -> anyhow::Result<()> {
        let cx = &mut *self.cx;
        let tasks = cx.gds().tasks().scope();
        cx.gds().initialize(tasks.clone(), logs);
        gds::appearance::motion::set_animations_enabled(
            config.preferences.animations_enabled.unwrap_or(true),
            cx,
        );
        gds::appearance::motion::follow_system(&tasks, cx);
        if gds::ui::logs::is_enabled(cx) {
            gds::ui::logs::init(
                gds::ui::logs::Options {
                    app_id: Some(APP_ID.into()),
                    app_name: "GDS Files".into(),
                    ignored_contexts: &[crate::shared::ui::actions::TERMINAL_TABS_CONTEXT],
                    ..Default::default()
                },
                cx,
            );
        }
        cx.set_app_identity(APP_ID, "GDS Files");
        if let Err(error) = i18n::install(
            config
                .preferences
                .language
                .unwrap_or_else(i18n::Language::detect),
            cx,
        ) {
            tracing::error!("Cannot load application messages: {error}");
            return Err(error.into());
        }
        time::apply(config.preferences.time_format, cx);
        if let Err(error) = theme::apply(&config.preferences.appearance, cx) {
            let unavailable_system = config.preferences.appearance.source == theme::Source::System;
            if unavailable_system {
                tracing::warn!(%error, "system theme unavailable; using built-in theme");
            } else {
                config.report(error.to_string());
            }
            let fallback = config.preferences.appearance.builtin_fallback();
            if let Err(error) = theme::apply(&fallback, cx) {
                tracing::error!("Cannot initialize the built-in theme: {error}");
                return Err(error.into());
            }
            if unavailable_system {
                config.preferences.appearance = fallback;
            }
        }
        match typography::install(&config.preferences.font, cx) {
            Ok(applied) => {
                if let Some(unavailable) = applied.unavailable {
                    tracing::error!(
                        "Font family {unavailable:?} is not installed; using the system font."
                    );
                }
            }
            Err(error) => {
                config.report(error.to_string());
                // An unusable stored size must not stop the window from
                // opening, and the stored configuration is left untouched.
                if let Err(fallback) = typography::install(&typography::Preferences::default(), cx)
                {
                    tracing::error!("Cannot initialize the default font: {fallback}");
                }
            }
        }
        theme::HotReload::initialize(config.preferences.appearance.clone(), cx);
        let preferences = cx
            .gds()
            .preferences()
            .initialize(config.preferences.clone())?;
        cx.subscribe(&preferences, |_, event: &preferences::SaveFailed, cx| {
            let mut arguments = fluent_bundle::FluentArgs::new();
            arguments.set("error", event.0.clone());
            gds::ui::notifications::Report::new(
                gds::ui::notifications::Level::Error,
                gds::i18n::format("settings-save-failed", Some(&arguments), cx),
            )
            .replacing::<preferences::SaveFailed>()
            .show(cx);
        })
        .detach();
        crate::entities::location::volumes::Service::initialize(tasks, cx);
        actions::register(cx);
        gds::ui::terminal::init(cx);
        Ok(())
    }
}
