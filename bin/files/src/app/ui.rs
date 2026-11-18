//! Shared presentation setup before the first Files window.

use crate::entities::preferences;
use crate::shared::i18n;
use gds::{
    app::Gds,
    appearance::{theme, typography},
    service::task_manager::Scope,
};

pub(super) struct Initializer {
    app: Gds,
}
impl Initializer {
    pub fn new(app: Gds) -> Self {
        Self { app }
    }
    /// Initializes global UI services, appearance, preferences, and keybindings before opening
    /// windows.
    ///
    /// # Errors
    /// Returns an error if base translations cannot be loaded, the built-in fallback theme fails
    /// to initialize, or the preferences entity cannot be installed.
    pub fn run(
        &mut self,
        tasks: &Scope,
        mut preferences: preferences::Preferences,
        notices: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        self.app
            .appearance()
            .motion()
            .set_enabled(preferences.animations_enabled.unwrap_or(true));
        self.app.appearance().motion().follow_system();
        self.install_i18n(preferences.language)?;
        self.app.ui().time().apply(preferences.time_format);
        self.apply_theme(&mut preferences.appearance, notices)?;
        self.apply_typography(&preferences.font, notices);
        self.app
            .appearance()
            .theme()
            .configure_hot_reload(preferences.appearance.clone());
        self.install_preferences(preferences, tasks)?;
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

    /// # Errors
    /// Returns an error if the preferences configuration path cannot be resolved.
    fn install_preferences(
        &mut self,
        preferences: preferences::Preferences,
        tasks: &Scope,
    ) -> anyhow::Result<()> {
        preferences::Service::install(preferences, tasks)?;
        Ok(())
    }
}
