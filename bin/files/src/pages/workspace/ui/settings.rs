//! Preferences, their editor and live appearance updates.

use super::*;
use crate::widgets::settings as widget;

pub(super) struct State {
    pub preferences: configuration::Preferences,
    pub view: Entity<widget::View>,
    pub theme_reload: Entity<gds::appearance::theme::HotReload>,
}

impl State {
    pub(super) fn new(
        preferences: configuration::Preferences,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        Self {
            view: cx
                .new(|cx| widget::View::new(preferences.clone(), Default::default(), window, cx)),
            theme_reload: gds::appearance::theme::HotReload::shared(
                preferences.appearance.clone(),
                window,
                cx,
            ),
            preferences,
        }
    }
}

impl Browser {
    pub(super) fn on_settings_event(
        &mut self,
        _: &Entity<widget::View>,
        event: &widget::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let widget::Event::Apply(preferences) = event;
        self.apply_preferences(preferences.clone(), window, cx);
    }

    pub(super) fn apply_preferences(
        &mut self,
        preferences: configuration::Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_preferences(preferences, true, window, cx);
    }

    pub(super) fn sync_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_preferences(configuration::current(cx), false, window, cx);
    }

    fn update_preferences(
        &mut self,
        preferences: configuration::Preferences,
        persist: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if preferences == self.settings.preferences {
            return;
        }
        let language_changed = preferences.language != self.settings.preferences.language;
        let appearance_changed = preferences.appearance != self.settings.preferences.appearance;
        let result = preferences.apply_shared(&self.settings.preferences, cx);
        if let Err(error) = result {
            let previous = self.settings.preferences.clone();
            self.settings
                .view
                .update(cx, |view, cx| view.set_preferences(previous, window, cx));
            report_settings_failure("settings-apply-failed", &error, cx);
            cx.notify();
            return;
        }
        let hidden_changed =
            preferences.files.show_hidden != self.settings.preferences.files.show_hidden;
        if !preferences.terminal_enabled() {
            self.terminal.visible = false;
            self.terminal.maximized = false;
        }
        self.settings.preferences = preferences.clone();
        self.apply_terminal_preferences(cx);
        if appearance_changed {
            self.settings.theme_reload.update(cx, |reload, cx| {
                reload.set_appearance(preferences.appearance.clone(), window, cx);
            });
        }
        if language_changed {
            // The shared language install succeeded; cached placeholder text and
            // the native window title are not read during render, so push them.
            self.sync_localized_fields(window, cx);
        }
        let save_error = persist
            .then(|| configuration::save(&preferences, cx))
            .and_then(Result::err);
        self.settings
            .view
            .update(cx, |view, cx| view.set_preferences(preferences, window, cx));
        if let Some(error) = save_error {
            report_settings_failure("settings-save-failed", &error, cx);
        }
        if hidden_changed {
            self.refresh_current(cx);
        }
        cx.notify();
    }
}
