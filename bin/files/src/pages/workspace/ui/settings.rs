//! Preferences, their editor and live appearance updates.

use super::*;
use crate::entities::preferences::PreferencesExt as _;
use crate::widgets::settings as widget;
use gds::app::AppExt as _;

pub(super) struct State {
    pub preferences: configuration::Preferences,
    pub view: Entity<widget::View>,
}

impl State {
    pub(super) fn new(
        preferences: configuration::Preferences,
        tasks: Scope,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        Self {
            view: cx.new(|cx| {
                widget::View::new(preferences.clone(), Default::default(), tasks, window, cx)
            }),
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
        if let Err(error) = cx.gds().preferences().update(preferences) {
            report_settings_failure("settings-apply-failed", error, cx);
        }
        self.sync_preferences(window, cx);
    }

    pub(super) fn edit_preferences(
        &mut self,
        edit: impl FnOnce(&mut configuration::Preferences),
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = cx.gds().preferences().edit(edit) {
            report_settings_failure("settings-apply-failed", error, cx);
        }
    }

    pub(super) fn sync_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferences = cx.gds().preferences().current();
        self.settings.view.update(cx, |view, cx| {
            view.set_preferences(preferences.clone(), window, cx);
        });
        if preferences == self.settings.preferences {
            return;
        }
        let language_changed = preferences.language != self.settings.preferences.language;
        let hidden_changed =
            preferences.files.show_hidden != self.settings.preferences.files.show_hidden;
        let tags_changed = preferences.tags != self.settings.preferences.tags;
        let roots_changed = preferences.files.volume_mount_points
            != self.settings.preferences.files.volume_mount_points;
        if !preferences.terminal_enabled() {
            self.terminal.visible = false;
            self.terminal.maximized = false;
        }
        self.settings.preferences = preferences.clone();
        if !preferences.tags.enabled && self.sidebar.regions.location == LocationTarget::Tags {
            self.sidebar.regions.location = LocationTarget::Home;
        }
        self.apply_terminal_preferences(cx);
        if language_changed {
            // The shared language install succeeded; cached placeholder text and
            // the native window title are not read during render, so push them.
            self.sync_localized_fields(window, cx);
        }
        if hidden_changed {
            self.refresh_current(cx);
        }
        if roots_changed {
            let tabs: Vec<_> = self
                .tabs
                .items
                .iter()
                .filter(|tab| tab.device.is_some())
                .map(|tab| tab.id)
                .collect();
            for tab in tabs {
                self.refresh_tab(tab, cx);
            }
        }
        if tags_changed {
            let targets: Vec<_> = self
                .tabs
                .items
                .iter()
                .filter_map(|slot| {
                    let location = slot
                        .navigation
                        .pending_location()
                        .unwrap_or(slot.navigation.location());
                    matches!(location, Location::Tags(_)).then(|| (slot.id, location.clone()))
                })
                .collect();
            for (id, location) in targets {
                if let Some(slot) = self.tab_slot_mut(id) {
                    slot.tags_load = None;
                    slot.navigation.cancel_pending();
                }
                self.navigate_location(id, location, cx);
            }
        }
        cx.notify();
    }
}
