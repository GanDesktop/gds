//! GPUI runtime services, appearance fallbacks, and global keybindings for Files.

use super::{APP_ID, APP_NAME};
use crate::entities::preferences;
use crate::shared::{
    app::{Files, FilesExt as _},
    i18n,
    ui::actions,
};
use gds::app::AppExt as _;
use gds::appearance::{theme, typography};

impl Files<&mut gpui_kit::App> {
    /// Binds the task supervisor scope, Unix termination signals, and idle window-close handling
    /// before asynchronous startup I/O begins.
    pub(in crate::app) fn bind_lifecycle(&mut self, tasks: gds::service::task_manager::Scope) {
        let cx = self.app();
        cx.gds().tasks().init(tasks);
        #[cfg(unix)]
        cx.gds()
            .tasks()
            .signals(|cx| cx.gds().tasks().request_quit());
        cx.on_window_closed(|cx, _| cx.gds().tasks().quit_when_idle())
            .detach();
    }

    /// Initializes global UI services, appearance, preferences, and keybindings before opening
    /// windows.
    ///
    /// # Errors
    /// Returns an error if base translations cannot be loaded, the built-in fallback theme fails
    /// to initialize, or the preferences entity cannot be installed.
    pub(in crate::app) fn init_ui(
        &mut self,
        mut preferences: preferences::Preferences,
        logs: Option<gds::service::logs::Store>,
        notices: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        let cx = self.app();
        let tasks = cx.gds_ro().tasks().scope();
        cx.gds().initialize(tasks, logs);
        cx.gds()
            .appearance()
            .motion()
            .set_enabled(preferences.animations_enabled.unwrap_or(true));
        cx.gds().appearance().motion().follow_system();
        cx.gds().ui().logs().init(gds::ui::logs::Options {
            app_id: Some(APP_ID.into()),
            app_name: APP_NAME.into(),
            ignored_contexts: &[actions::TERMINAL_TABS_CONTEXT],
            ..Default::default()
        });
        cx.set_app_identity(APP_ID, APP_NAME);

        self.install_i18n(preferences.language)?;
        self.app().gds().ui().time().apply(preferences.time_format);
        self.apply_theme(&mut preferences.appearance, notices)?;
        self.apply_typography(&preferences.font, notices);
        self.app()
            .gds()
            .appearance()
            .theme()
            .init_hot_reload(preferences.appearance.clone());
        self.install_preferences(preferences)?;
        self.app().gds().files().volumes().initialize();
        self.register_actions();
        Ok(())
    }

    /// # Errors
    /// Returns an error if the bundled Fluent catalog cannot be parsed.
    fn install_i18n(&mut self, language: Option<i18n::Language>) -> anyhow::Result<()> {
        let catalog = i18n::prepare(language.unwrap_or_else(i18n::Language::detect))
            .inspect_err(|error| tracing::error!("Cannot load application messages: {error}"))?;
        self.app().gds().i18n().install_catalog(catalog);
        Ok(())
    }

    /// # Errors
    /// Returns an error if both the requested theme and the built-in fallback fail to apply.
    fn apply_theme(
        &mut self,
        appearance: &mut theme::Appearance,
        notices: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        let cx = self.app();
        let Err(error) = cx.gds().appearance().theme().apply(appearance) else {
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
        if let Err(error) = cx.gds().appearance().theme().apply(&fallback) {
            tracing::error!("Cannot initialize the built-in theme: {error}");
            return Err(error.into());
        }
        if unavailable_system {
            *appearance = fallback;
        }
        Ok(())
    }

    fn apply_typography(&mut self, font: &typography::Preferences, notices: &mut Vec<String>) {
        let cx = self.app();
        match cx.gds().appearance().typography().apply(font) {
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
                if let Err(fallback) = cx
                    .gds()
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
    fn install_preferences(&mut self, preferences: preferences::Preferences) -> anyhow::Result<()> {
        let entity = self
            .app()
            .gds()
            .files()
            .preferences()
            .initialize(preferences)?;
        self.app()
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
