//! Pure UI-signal state transitions (address bar edit, filter availability, sidebar focus, pages).

use gds::ui::window::{HeightTier, WidthTier};
use zgui::prelude::*;

use super::super::state::{
    context::Context,
    model::{AddressPresentation, Contents, Pane},
    ui_flags::{FilterFocusMode, SidebarRegion},
};
use super::{geometry::Geometry, rows::Rows};
use crate::entities::location::Location;

pub(in crate::pages::workspace) struct UiState<'a>(pub &'a Context);

impl UiState<'_> {
    pub(in crate::pages::workspace) fn details_available_structure(&self) -> bool {
        self.0.read_structure(|state| {
            let tab = state.tab();
            match tab.contents {
                Contents::Directory | Contents::Places(_) | Contents::Recent(_) => true,
                Contents::Tags { .. } => !tab.is_pure_tag_catalogue(),
                Contents::LoadingCollection
                | Contents::Unavailable(_)
                | Contents::Network { .. }
                | Contents::Trash(_)
                | Contents::Remote { .. }
                | Contents::Volumes => false,
            }
        })
    }

    pub(in crate::pages::workspace) fn start_address_edit(&self) {
        let value = self.0.read_untracked(|state| {
            let navigation = &state.tab().navigation;
            if let Some(pending) = navigation.pending_location() {
                Rows::location_address(pending, navigation.pending_path())
            } else {
                Rows::location_address(navigation.location(), navigation.path())
            }
        });
        self.0.0.address.set(value.clone());
        self.0
            .0
            .address_presentation
            .set(AddressPresentation::Editing {
                original: value.clone(),
            });
        let node = self.0.0.address_node;
        if node.is_bound() {
            node.set_value(&value);
            node.focus();
            node.select_all();
        }
    }

    pub(in crate::pages::workspace) fn finish_address_edit(&self) {
        let AddressPresentation::Editing { original } =
            self.0.0.address_presentation.get_untracked()
        else {
            return;
        };
        self.0
            .0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        self.0.0.address.set(original);
    }

    pub(in crate::pages::workspace) fn activate_filter(&self) {
        if !self.filter_available_untracked() {
            return;
        }
        if Geometry(self.0).height_tier_untracked() == HeightTier::Micro {
            self.0.0.footer_filter_open.set(true);
        }
        let node = self.0.0.filter_node;
        if node.is_bound() {
            node.focus();
            node.select_all();
        } else {
            self.0
                .0
                .filter_focus_request
                .set(Some(FilterFocusMode::SelectAll));
        }
    }

    pub(in crate::pages::workspace) fn filter_available(&self) -> bool {
        let geometry = Geometry(self.0);
        self.0.pane() == Pane::Files
            && self
                .0
                .read(|state| match state.tab().navigation.location() {
                    Location::Recent | Location::Favorites | Location::Tags(_) => {
                        geometry.height_tier() == HeightTier::Micro || !geometry.actions_collapsed()
                    }
                    Location::Directory(_) | Location::Volume { .. } | Location::Remote(_) => true,
                    Location::Network | Location::Volumes | Location::Trash => false,
                })
    }

    pub(in crate::pages::workspace) fn filter_available_untracked(&self) -> bool {
        let geometry = Geometry(self.0);
        self.0.pane_untracked() == Pane::Files
            && self
                .0
                .read_untracked(|state| match state.tab().navigation.location() {
                    Location::Recent | Location::Favorites | Location::Tags(_) => {
                        geometry.height_tier_untracked() == HeightTier::Micro
                            || !geometry.actions_collapsed_untracked()
                    }
                    Location::Directory(_) | Location::Volume { .. } | Location::Remote(_) => true,
                    Location::Network | Location::Volumes | Location::Trash => false,
                })
    }

    pub(in crate::pages::workspace) fn sidebar_focus_changed(
        &self,
        region: SidebarRegion,
        focused: bool,
    ) {
        if focused {
            self.0.0.sidebar_region.set(Some(region));
        } else if self.0.0.sidebar_region.get_untracked() == Some(region) {
            self.0.0.sidebar_region.set(None);
        }
    }

    pub(in crate::pages::workspace) fn focused_sidebar_region(&self) -> Option<SidebarRegion> {
        let focused = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten());
        if focused.is_some_and(|node| self.0.0.tabs_node.contains(node)) {
            Some(SidebarRegion::Tabs)
        } else if focused.is_some_and(|node| self.0.0.quick_access_node.contains(node)) {
            Some(SidebarRegion::QuickAccess)
        } else if focused.is_some_and(|node| self.0.0.locations_node.contains(node)) {
            Some(SidebarRegion::Locations)
        } else if focused.is_some_and(|node| self.0.0.volumes_node.contains(node)) {
            Some(SidebarRegion::Volumes)
        } else {
            self.0
                .0
                .sidebar_region
                .get_untracked()
                .filter(|_| !self.0.0.list_focused.get())
        }
    }

    pub(in crate::pages::workspace) fn open_page(&self, pane: Pane) {
        self.finish_address_edit();
        self.0.0.pages.update(|pages| pages.open(pane));
        self.0.focus_page_after_mount();
    }

    fn sidebar_contains_focus(&self) -> bool {
        if self.focused_sidebar_region().is_some() {
            return true;
        }
        let Some(focused) = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten())
        else {
            return false;
        };
        [
            self.0.0.sidebar_content_node,
            self.0.0.locations_region_node,
            self.0.0.tabs_node,
            self.0.0.quick_access_node,
            self.0.0.volumes_node,
            self.0.0.locations_node,
        ]
        .into_iter()
        .any(|node| node.get_untracked() == Some(focused) || node.contains(focused))
    }

    pub(in crate::pages::workspace) fn toggle_sidebar(&self) {
        if Geometry(self.0).width_tier_untracked() == WidthTier::Micro {
            return;
        }
        let next = !self.0.0.sidebar.get_untracked();
        let restore_focus = !next && self.sidebar_contains_focus();
        self.0.0.sidebar.set(next);
        if restore_focus {
            self.0.0.sidebar_region.set(None);
            if self.0.pane_untracked() == Pane::Files {
                self.0.focus_content();
            } else if self.0.0.settings_node.is_bound() {
                self.0.0.settings_node.focus();
            } else {
                self.0.focus_content();
            }
        }
    }

    pub(in crate::pages::workspace) fn toggle_settings(&self) {
        if self.0.pane_untracked() == Pane::Settings {
            self.0.back_page();
        } else {
            self.show_settings();
        }
    }

    pub(in crate::pages::workspace) fn show_settings(&self) {
        self.open_page(Pane::Settings);
    }
}
