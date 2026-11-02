use super::*;
use gds_uim::el;

impl Frame {
    pub fn workspace(&self, sidebar: AnyElement, content: AnyElement, cx: &App) -> Div {
        let surface = div()
            .size_full()
            .bg(theme::content_background(cx))
            .child(content);
        self.workspace_with_content_surface(sidebar, surface.into_any_element(), cx)
    }

    /// For composite content whose panes paint their own material. Do not add a
    /// shared underlay: translucent pane backgrounds would accumulate its alpha.
    pub fn workspace_with_content_surface(
        &self,
        sidebar: AnyElement,
        content: AnyElement,
        cx: &App,
    ) -> Div {
        let width = self.sidebar_width;
        let divider = self.vertical_divider(cx);
        el! {
            .div { relative; flex; flex_1; min_h_0; w_full } [
                @if (let Some(width) = width)
                    .div { # "frame-workspace-sidebar"; test_support; w: width.px; h_full; flex_shrink_0; bg: $(theme::sidebar_background(cx)) } [sidebar],
                @end,
                .div { flex_1; min_w_0; h_full } [content],
                ..divider,
            ]
        }
    }
}
