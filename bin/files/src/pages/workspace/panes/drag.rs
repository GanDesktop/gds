//! Native tab reordering, pointer/OS drag-and-drop, spring-loading, and lazy file drag snapshots.

mod coordinator;
mod files;
mod sidebar;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use gds::ui::IconName;
use zgui::prelude::AnyView;
use zgui::view::{EventCx, events};

pub(in crate::pages::workspace) use coordinator::{FileDrag, take_row_pointer_down};

use super::super::{
    projection::geometry::Grid,
    state::{context::Context, model::Row},
};
use coordinator::{WindowDragPayload, coordinator_for};

pub(in crate::pages::workspace) struct DragHost<'a>(pub &'a Context);

impl DragHost<'_> {
    /// Window-level floating preview portal for in-window file and tab drags.
    ///
    /// Mounted once on the Files workspace pane so tab previews stay visible on non-directory views
    /// (Recent, Favorites, Trash, Network) and survive spring-loaded folder switches.
    pub(in crate::pages::workspace) fn drag_overlay(&self) -> AnyView {
        files::install_drag_preview_stylesheet();
        let sidebar_presentation = self.0.sidebar_presentation();
        coordinator_for(self.0).render_overlay(move |payload, position, copy_mode| match payload {
            WindowDragPayload::Files(drag) => {
                files::render_file_drag_card(drag, position, copy_mode)
            }
            WindowDragPayload::Tab(preview) => {
                sidebar::render_tab_ghost(preview, position, sidebar_presentation)
            }
            WindowDragPayload::QuickAccess(preview) => {
                sidebar::render_quick_access_ghost(preview, position, sidebar_presentation)
            }
        })
    }

    pub(in crate::pages::workspace) fn background_drop_target(&self, view: AnyView) -> AnyView {
        files::background_drop_target(self.0, view)
    }

    pub(in crate::pages::workspace) fn file_drag_target(
        &self,
        view: AnyView,
        destination: PathBuf,
    ) -> AnyView {
        files::file_drag_target(self.0, view, destination)
    }

    pub(in crate::pages::workspace) fn unsupported_file_drop_target(
        &self,
        view: AnyView,
    ) -> AnyView {
        files::unsupported_file_drop_target(self.0, view)
    }

    pub(in crate::pages::workspace) fn file_row_drag(
        &self,
        view: AnyView,
        row: &Row,
        grid: Option<Grid>,
    ) -> AnyView {
        files::file_row_drag(self.0, view, row, grid)
    }

    pub(in crate::pages::workspace) fn mouse_navigation(
        &self,
        event: &mut EventCx<'_, events::PointerDown>,
    ) {
        files::mouse_navigation(self.0, event);
    }

    pub(in crate::pages::workspace) fn draggable_tab(
        &self,
        view: AnyView,
        id: u64,
        pinned: bool,
    ) -> AnyView {
        sidebar::draggable_tab(self.0, view, id, pinned)
    }

    pub(in crate::pages::workspace) fn quick_access_drop_zone(&self, view: AnyView) -> AnyView {
        sidebar::quick_access_drop_zone(self.0, view)
    }

    pub(in crate::pages::workspace) fn draggable_quick_access(
        &self,
        view: AnyView,
        path: PathBuf,
        label: String,
        icon: IconName,
    ) -> AnyView {
        sidebar::draggable_quick_access(self.0, view, path, label, icon)
    }
}
