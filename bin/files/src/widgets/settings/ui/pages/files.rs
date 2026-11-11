use gds_uim::el;
use gpui_kit::*;

use crate::widgets::settings::ui::{View, controls::Field};

impl View {
    pub(super) fn render_files(&mut self, cx: &mut Context<Self>) -> Div {
        let row = self.toggle_row(Field::ShowHidden, false, cx);
        let volumes_row = self.toggle_row(Field::SidebarVolumes, false, cx);
        let search_row = self.toggle_row(Field::AutoFocusSearch, false, cx);
        el! { .div [row, volumes_row, search_row] }
    }
}
