//! Preferences, their editor and live appearance updates.

use super::*;
use crate::widgets::settings as widget;

pub(in crate::pages::workspace) struct State {
    pub preferences: configuration::Preferences,
    pub view: Option<Entity<widget::View>>,
}

impl State {
    pub(in crate::pages::workspace) fn new(preferences: configuration::Preferences) -> Self {
        Self {
            preferences,
            view: None,
        }
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn ensure_settings_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<widget::View> {
        if let Some(view) = &self.settings.view {
            return view.clone();
        }
        let preferences = self.settings.preferences.clone();
        let presentation = self.sidebar.presentation;
        let tasks = self.window.tasks.clone();
        let view = cx.new(|cx| widget::View::new(preferences, presentation, tasks, window, cx));
        self.window
            .subscriptions
            .push(cx.subscribe_in(&view, window, Self::on_settings_event));
        self.settings.view = Some(view.clone());
        view
    }

    pub(in crate::pages::workspace) fn open_settings_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.settings.view.clone() {
            let preferences = self.settings.preferences.clone();
            let presentation = self.sidebar.presentation;
            view.update(cx, |view, cx| {
                view.set_preferences(preferences, window, cx);
                view.set_sidebar_presentation(presentation, cx);
                view.refresh_themes(window, cx);
            });
            return;
        }
        self.ensure_settings_view(window, cx);
    }

    pub(in crate::pages::workspace) fn on_settings_event(
        &mut self,
        _: &Entity<widget::View>,
        event: &widget::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let widget::Event::Apply(preferences) = event;
        self.apply_preferences(preferences.clone(), window, cx);
    }

    pub(in crate::pages::workspace) fn apply_preferences(
        &mut self,
        preferences: configuration::Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = cx.gds_mut().preferences().update(preferences) {
            report_settings_failure(error, cx);
        }
        let current = cx.gds().preferences().current();
        self.sync_preferences(current, window, cx);
    }

    pub(in crate::pages::workspace) fn edit_preferences(
        &mut self,
        edit: impl FnOnce(&mut configuration::Preferences),
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = cx.gds_mut().preferences().edit(edit) {
            report_settings_failure(error, cx);
        }
    }

    pub(in crate::pages::workspace) fn sync_preferences(
        &mut self,
        preferences: configuration::Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = &self.settings.view {
            view.update(cx, |view, cx| {
                view.set_preferences(preferences.clone(), window, cx);
            });
        }
        if preferences == self.settings.preferences {
            return;
        }
        let language_changed = preferences.language != self.settings.preferences.language;
        let hidden_changed =
            preferences.files.show_hidden != self.settings.preferences.files.show_hidden;
        let child_limit_changed = preferences.files.enrichment.child_count_limit
            != self.settings.preferences.files.enrichment.child_count_limit;
        let enrichment_changed = preferences.files.probe_budget
            != self.settings.preferences.files.probe_budget
            || preferences.files.enrichment != self.settings.preferences.files.enrichment;
        let preview_changed = preferences.files.preview != self.settings.preferences.files.preview;
        let watching_changed =
            preferences.files.watching != self.settings.preferences.files.watching;
        let tags_changed = preferences.tags != self.settings.preferences.tags;
        let roots_changed = preferences.files.volume_mount_points
            != self.settings.preferences.files.volume_mount_points;
        if !preferences.terminal_enabled() {
            self.terminal.visible = false;
            self.terminal.maximized = false;
        }
        self.settings.preferences = preferences.clone();
        if !preferences.tags.enabled
            && self.sidebar.regions.location == LocationTarget::Collection(Collection::Tags)
        {
            self.sidebar.regions.location = LocationTarget::Home;
        }
        self.apply_terminal_preferences(cx);
        if language_changed {
            // The shared language install succeeded; cached placeholder text and
            // the native window title are not read during render, so push them.
            self.sync_localized_fields(window, cx);
        }
        if preview_changed {
            let options = preferences.files.preview.cache_options();
            self.files
                .inspections
                .update(cx, |cache, cx| cache.set_options(options, cx));
        }
        if hidden_changed || child_limit_changed {
            self.refresh_current(cx);
        } else if enrichment_changed {
            let tabs: Vec<_> = self.tabs.items.iter().map(|tab| tab.id).collect();
            for tab in tabs {
                self.schedule_enrichment(tab, cx);
            }
        }
        if watching_changed {
            let watched: Vec<_> = self
                .tabs
                .items
                .iter()
                .filter_map(|tab| tab.watch().map(|watch| (tab.id, watch.path.clone())))
                .collect();
            for (id, path) in watched {
                if let Some(slot) = self.tab_slot_mut(id) {
                    slot.set_watch(None);
                }
                self.watch_directory(id, Some(&path), cx);
            }
        }
        if roots_changed {
            let tabs: Vec<_> = self
                .tabs
                .items
                .iter()
                .filter(|tab| tab.device().is_some())
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
                    slot.set_tags_load(None);
                    slot.navigation.cancel_pending();
                }
                self.navigate_location(id, location, cx);
            }
        }
        cx.notify();
    }
}
