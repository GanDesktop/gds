//! Quick Access pinned-folder queries, mutations, and navigation actions.

use std::path::{Path, PathBuf};

use zgui::prelude::*;

use super::super::state::{
    context::Context,
    model::Pane,
    ui_flags::{AccordionSelection, CollapsibleSection, QuickAccessAddState},
};
use super::tabs::Navigator;
use crate::{entities::location::Location, shared::i18n};

pub(in crate::pages::workspace) struct QuickAccess<'a>(pub &'a Context);

impl QuickAccess<'_> {
    pub(in crate::pages::workspace) fn quick_access_paths(&self) -> Vec<PathBuf> {
        self.0.read(|state| {
            state
                .places
                .quick_access
                .iter()
                .filter_map(|loc| loc.local_path().map(Path::to_path_buf))
                .collect()
        })
    }

    pub(in crate::pages::workspace) fn quick_access_paths_untracked(&self) -> Vec<PathBuf> {
        self.0.read_untracked(|state| {
            state
                .places
                .quick_access
                .iter()
                .filter_map(|loc| loc.local_path().map(Path::to_path_buf))
                .collect()
        })
    }

    pub(in crate::pages::workspace) fn tab_local_path(&self, id: u64) -> Option<PathBuf> {
        self.0.read_untracked(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .and_then(|tab| tab.navigation.path().map(Path::to_path_buf))
        })
    }

    pub(in crate::pages::workspace) fn add_quick_access_path(&self, path: PathBuf) -> bool {
        let location = Location::from(path.as_path());
        let added = self.0.update(|state| {
            if state.places.contains_quick_access(&location) {
                return false;
            }
            state.places.add_quick_access(location.clone());
            true
        });
        if added {
            self.0.save_places();
        }
        let prefs = self.0.0.preferences.snapshot();
        if !prefs.files.sidebar.show_quick_access {
            self.0.0.sidebar_reveal_quick_access.set(true);
        }
        if self.0.0.sidebar_in_accordion.get_untracked() {
            self.0.0.sidebar_accordion.set(AccordionSelection::Expanded(
                CollapsibleSection::QuickAccess,
            ));
        } else if prefs.files.sidebar.quick_access_collapsed {
            self.0
                .edit_preferences(|p| p.files.sidebar.quick_access_collapsed = false);
        }
        added
    }

    pub(in crate::pages::workspace) fn add_current_to_quick_access(&self) {
        if self.0.pane_untracked() != Pane::Files {
            self.0
                .report_notice(i18n::text_untracked("quick-access-local-only"));
            return;
        }
        let path = self
            .0
            .read_untracked(|state| state.tab().navigation.path().map(Path::to_path_buf));
        let Some(path) = path else {
            self.0
                .report_notice(i18n::text_untracked("quick-access-local-only"));
            return;
        };
        let _ = self.add_quick_access_path(path);
    }

    pub(in crate::pages::workspace) fn add_quick_access_from_tab(&self, tab_id: u64) {
        let Some(path) = self.tab_local_path(tab_id) else {
            self.0
                .report_notice(i18n::text_untracked("quick-access-local-only"));
            return;
        };
        let _ = self.add_quick_access_path(path);
    }

    pub(in crate::pages::workspace) fn remove_quick_access(&self, path: &Path) {
        let location = Location::from(path);
        let removed = self.0.update(|state| {
            if !state.places.contains_quick_access(&location) {
                return false;
            }
            state.places.remove_quick_access(location.clone());
            true
        });
        if removed {
            self.0.save_places();
        }
    }

    pub(in crate::pages::workspace) fn reorder_quick_access_item(&self, from: &Path, to: &Path) {
        let from_loc = Location::from(from);
        let to_loc = Location::from(to);
        let changed = self.0.update(|state| {
            let mut order = state.places.quick_access.clone();
            let Some(from_idx) = order.iter().position(|loc| loc == &from_loc) else {
                return false;
            };
            let Some(to_idx) = order.iter().position(|loc| loc == &to_loc) else {
                return false;
            };
            if from_idx == to_idx {
                return false;
            }
            let item = order.remove(from_idx);
            order.insert(to_idx, item);
            state.places.reorder_quick_access(order);
            true
        });
        if changed {
            self.0.save_places();
        }
    }

    pub(in crate::pages::workspace) fn quick_access_add_state_untracked(
        &self,
    ) -> QuickAccessAddState {
        if self.0.pane_untracked() != Pane::Files {
            return QuickAccessAddState::NotLocal;
        }
        self.0
            .read_untracked(|state| match state.tab().navigation.path() {
                Some(path) if state.places.contains_quick_access(Location::from(path)) => {
                    QuickAccessAddState::AlreadyAdded
                }
                Some(_) => QuickAccessAddState::CanAdd,
                None => QuickAccessAddState::NotLocal,
            })
    }

    pub(in crate::pages::workspace) fn open_quick_access(&self, path: &Path, in_new_tab: bool) {
        if !path.is_dir() {
            self.0.report_notice(gds::t!(
                "quick-access-missing",
                path = path.display().to_string()
            ));
            return;
        }
        let navigator = Navigator(self.0);
        if in_new_tab {
            navigator.new_tab(Some(Location::from(path)));
        } else {
            navigator.open(Location::from(path));
        }
        self.0.focus_content_after_mount();
    }
}
