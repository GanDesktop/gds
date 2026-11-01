use crate::widgets::settings::ui::View;
use gds_uim::el;
use gpui_kit::*;

impl View {
    pub(super) fn render_general(&mut self, _: &mut Context<Self>) -> Div {
        el! { .div [$(self.general.clone())] }
    }
}
