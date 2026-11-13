use crate::widgets::settings::ui::View;
use gds::i18n::t;
use gds_uim::el;
use gpui_kit::*;

impl View {
    pub(super) fn render_extensions(&self, cx: &mut Context<Self>) -> Div {
        el! { .div [
            .div { text_lg; font_weight: FontWeight::SEMIBOLD; } [$(t!(@cx, "tags-title"))],
            $(self.render_tags_settings(cx)),
        ] }
    }
}
