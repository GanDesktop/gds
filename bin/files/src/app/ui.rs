//! Native runtime services, appearance fallbacks, and shared UI initialization for Files.

use gds::{
    app::Gds,
    appearance::{theme, typography},
    service::task_manager::Scope,
    ui::notifications::{self, Level, Report},
};
use zgui::reactive::{RenderEffect, on_cleanup_local, prelude::*};

use super::{APP_ID, APP_NAME};
use crate::entities::preferences;
use crate::shared::i18n;

pub(super) struct Initializer {
    app: Gds,
}

impl Initializer {
    pub fn new(app: Gds) -> Self {
        Self { app }
    }

    /// Initializes global UI services, appearance, preferences, and logging options before
    /// opening windows.
    ///
    /// # Errors
    /// Returns an error if base translations cannot be loaded or the built-in fallback theme fails
    /// to initialize.
    pub fn run(
        &mut self,
        tasks: &Scope,
        mut startup: preferences::StartupRead,
        notices: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        self.app
            .appearance()
            .motion()
            .set_enabled(startup.preferences.animations_enabled.unwrap_or(true));
        self.app.appearance().motion().follow_system();
        gds::ui::logs::init(gds::ui::logs::Options {
            app_id: Some(APP_ID.into()),
            app_name: APP_NAME.into(),
            ignored_contexts: &["GdsTerminal", "Input", "textarea", "input"],
        });

        self.install_i18n(startup.preferences.language)?;
        gds::ui::time::apply(startup.preferences.time_format);
        self.apply_theme(&mut startup.preferences.appearance, notices)?;
        self.apply_typography(&startup.preferences.font, notices);
        self.app
            .appearance()
            .theme()
            .configure_hot_reload(startup.preferences.appearance.clone());
        self.install_preferences(startup, tasks);
        Ok(())
    }

    /// # Errors
    /// Returns an error if the bundled Fluent catalog cannot be parsed.
    fn install_i18n(&mut self, language: Option<i18n::Language>) -> anyhow::Result<()> {
        let catalog = i18n::prepare(language.unwrap_or_else(i18n::Language::detect))
            .inspect_err(|error| tracing::error!("Cannot load application messages: {error}"))?;
        self.app.i18n().install_catalog(catalog);
        Ok(())
    }

    /// # Errors
    /// Returns an error if both the requested theme and the built-in fallback fail to apply.
    fn apply_theme(
        &mut self,
        appearance: &mut theme::Appearance,
        notices: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        let Err(error) = self.app.appearance().theme().apply(appearance) else {
            return Ok(());
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
        if let Err(error) = self.app.appearance().theme().apply(&fallback) {
            tracing::error!("Cannot initialize the built-in theme: {error}");
            return Err(error.into());
        }
        if unavailable_system {
            *appearance = fallback;
        }
        Ok(())
    }

    fn apply_typography(&mut self, font: &typography::Preferences, notices: &mut Vec<String>) {
        match self.app.appearance().typography().apply(font) {
            Ok(applied) => {
                if let Some(unavailable) = applied.unavailable {
                    tracing::error!(
                        "Font family {unavailable:?} is not installed; using the system font."
                    );
                }
            }
            Err(error) => {
                notices.push(error.to_string());
                // An out-of-range stored font size must not block window creation; apply the default
                // metrics for this session while leaving the stored preference untouched.
                if let Err(fallback) = self
                    .app
                    .appearance()
                    .typography()
                    .apply(&typography::Preferences::default())
                {
                    tracing::error!("Cannot initialize the default font: {fallback}");
                }
            }
        }
    }

    fn install_preferences(&mut self, startup: preferences::StartupRead, tasks: &Scope) {
        let service = preferences::Service::install_loaded(startup, tasks);
        let error_sig = service.error();
        let app = self.app.clone();
        self.app.with(|| {
            let fx = RenderEffect::new(move |prev: Option<Option<String>>| {
                let current = error_sig.get();
                if let Some(message) = &current
                    && prev.as_ref().and_then(Option::as_ref) != Some(message)
                {
                    notifications::report(Report::new(
                        Level::Error,
                        gds::t_untracked!(@app, "settings-save-failed", error = message.clone()),
                    ));
                }
                current
            });
            on_cleanup_local(move || drop(fx));
        });
    }
}
