//! Application and selection menus, and the persisted column-layout editor.

mod actions;
mod columns;
mod entries;
mod pages;
mod surface;

use std::path::PathBuf;

use zgui::prelude::AnyView;
use zgui::vocab::KeyEvent;

use super::super::state::{context::Context, menus::Surface};
use crate::entities::location::{Location, volumes};

pub(in crate::pages::workspace) struct MenuHost<'a>(pub &'a Context);

impl MenuHost<'_> {
    pub(in crate::pages::workspace) fn menu_key(&self, event: &KeyEvent) -> bool {
        surface::menu_key(self.0, event)
    }

    pub(in crate::pages::workspace) fn app_menu(&self) -> AnyView {
        surface::app_menu(self.0)
    }

    pub(in crate::pages::workspace) fn show_app_menu(&self) {
        surface::show_app_menu(self.0);
    }

    pub(in crate::pages::workspace) fn page_menu(&self) -> AnyView {
        surface::page_menu(self.0)
    }

    pub(in crate::pages::workspace) fn special_page_actions(&self) -> Option<AnyView> {
        pages::special_page_actions(self.0)
    }

    pub(in crate::pages::workspace) fn show_context_menu(&self) {
        surface::show_context_menu(self.0);
    }

    pub(in crate::pages::workspace) fn row_menu(
        &self,
        target_view: AnyView,
        index: usize,
    ) -> AnyView {
        surface::row_menu(self.0, target_view, index)
    }

    pub(in crate::pages::workspace) fn sidebar_volume_menu(
        &self,
        target_view: AnyView,
        volume: &volumes::Volume,
    ) -> AnyView {
        let volume = volume.clone();
        surface::context_menu_region(self.0, true, target_view, move |x, y| Surface::Volume {
            volume: volume.clone(),
            x,
            y,
        })
    }

    pub(in crate::pages::workspace) fn sidebar_quick_access_section_menu(
        &self,
        target_view: AnyView,
    ) -> AnyView {
        surface::context_menu_region(self.0, true, target_view, |x, y| {
            Surface::QuickAccessSection { x, y }
        })
    }

    pub(in crate::pages::workspace) fn sidebar_quick_access_menu(
        &self,
        target_view: AnyView,
        path: PathBuf,
    ) -> AnyView {
        surface::context_menu_region(self.0, true, target_view, move |x, y| {
            Surface::QuickAccessItem {
                path: path.clone(),
                x,
                y,
            }
        })
    }

    pub(in crate::pages::workspace) fn history_menu(
        &self,
        target_view: AnyView,
        tab: u64,
        visits: Vec<(usize, Location)>,
    ) -> AnyView {
        surface::context_menu_region(self.0, false, target_view, move |x, y| Surface::History {
            tab,
            visits: visits.clone(),
            x,
            y,
        })
    }

    pub(in crate::pages::workspace) fn directory_menu(&self, target_view: AnyView) -> AnyView {
        surface::directory_menu(self.0, target_view)
    }

    pub(in crate::pages::workspace) fn tab_menu(&self, target_view: AnyView, id: u64) -> AnyView {
        surface::tab_menu(self.0, target_view, id)
    }

    pub(in crate::pages::workspace) fn open_selected_tab(&self) {
        actions::open_selected_tab(self.0);
    }

    pub(in crate::pages::workspace) fn copy_selected_paths(&self) {
        actions::copy_selected_paths(self.0);
    }

    pub(in crate::pages::workspace) fn change_volume(&self, action: volumes::DeviceAction) {
        actions::change_volume(self.0, action);
    }

    pub(in crate::pages::workspace) fn show_help(&self) {
        actions::show_help(self.0);
    }

    #[cfg(feature = "files-e2e")]
    pub(in crate::pages::workspace) fn show_about(&self) {
        actions::show_about(self.0);
    }

    pub(in crate::pages::workspace) fn column_menu(&self) -> AnyView {
        columns::column_menu(self.0)
    }

    pub(in crate::pages::workspace) fn show_sort_menu(&self) {
        columns::show_sort_menu(self.0);
    }

    pub(in crate::pages::workspace) fn menu_overlay(&self) -> AnyView {
        surface::menu_overlay(self.0)
    }
}
