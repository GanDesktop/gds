//! GPUI runtime services, appearance fallbacks, and global keybindings for Files.

use super::{APP_ID, APP_NAME};
use crate::entities::{
    location::volumes::VolumesExt as _,
    preferences::{self, PreferencesExt as _},
};
use crate::shared::{i18n, ui::actions};
use gds::app::AppExt as _;
use gds::appearance::{theme, typography};
use gds::service::task_manager::Scope;
use gpui_kit::App;

pub(super) struct Initializer<'a> {
    cx: &'a mut App,
}

impl<'a> Initializer<'a> {
    pub fn new(cx: &'a mut App) -> Self {
        Self { cx }
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
        logs: Option<gds::service::logs::Store>,
        notices: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        self.cx.gds_mut().init_ui(logs);
        self.cx
            .gds_mut()
            .appearance()
            .motion()
            .set_enabled(preferences.animations_enabled.unwrap_or(true));
        self.cx.gds_mut().appearance().motion().follow_system();
        self.cx.gds_mut().ui().logs().init(gds::ui::logs::Options {
            app_id: Some(APP_ID.into()),
            app_name: APP_NAME.into(),
            ignored_contexts: &[actions::TERMINAL_TABS_CONTEXT],
            ..Default::default()
        });
        self.cx.set_app_identity(APP_ID, APP_NAME);

        self.install_i18n(preferences.language)?;
        self.cx.gds_mut().ui().time().apply(preferences.time_format);
        self.apply_theme(&mut preferences.appearance, notices)?;
        self.apply_typography(&preferences.font, notices);
        self.cx
            .gds_mut()
            .appearance()
            .theme()
            .init_hot_reload(preferences.appearance.clone());
        self.install_preferences(preferences, tasks)?;
        self.cx.gds_mut().volumes().initialize(tasks);
        actions::Command::register(self.cx);
        Ok(())
    }

    /// # Errors
    /// Returns an error if the bundled Fluent catalog cannot be parsed.
    fn install_i18n(&mut self, language: Option<i18n::Language>) -> anyhow::Result<()> {
        let catalog = i18n::prepare(language.unwrap_or_else(i18n::Language::detect))
            .inspect_err(|error| tracing::error!("Cannot load application messages: {error}"))?;
        self.cx.gds_mut().i18n().install_catalog(catalog);
        Ok(())
    }

    /// # Errors
    /// Returns an error if both the requested theme and the built-in fallback fail to apply.
    fn apply_theme(
        &mut self,
        appearance: &mut theme::Appearance,
        notices: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        let Err(error) = self.cx.gds_mut().appearance().theme().apply(appearance) else {
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
        if let Err(error) = self.cx.gds_mut().appearance().theme().apply(&fallback) {
            tracing::error!("Cannot initialize the built-in theme: {error}");
            return Err(error.into());
        }
        if unavailable_system {
            *appearance = fallback;
        }
        Ok(())
    }

    fn apply_typography(&mut self, font: &typography::Preferences, notices: &mut Vec<String>) {
        match self.cx.gds_mut().appearance().typography().apply(font) {
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
                    .cx
                    .gds_mut()
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
        let entity = self
            .cx
            .gds_mut()
            .preferences()
            .initialize(preferences, tasks)?;
        self.cx
            .subscribe(&entity, |_, event: &preferences::SaveFailed, cx| {
                gds::ui::notifications::Report::new(
                    gds::ui::notifications::Level::Error,
                    gds::i18n::t!(@cx, "settings-save-failed", error = event.0.clone()),
                )
                .replacing::<preferences::SaveFailed>()
                .show(cx);
            })
            .detach();
        Ok(())
    }
}
