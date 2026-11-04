//! Width-based workspace presentation, independent of saved location preferences.
use super::*;

// Approximate room for the expanded sidebar, details, a short title and full toolbar.
const COMPACT_WIDTH: f32 = 1080.;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Layout {
    pub compact: bool,
}

impl Layout {
    pub fn for_width(width: Pixels) -> Self {
        Self {
            compact: width < px(COMPACT_WIDTH),
        }
    }
}

impl Browser {
    pub(super) fn compact_page_header(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ui::page::PageHeader {
        use gds::ui::page::*;
        use gds_uim::el;
        let title = self.page_title(cx);
        let search = self
            .collection()
            .is_none()
            .then(|| self.directory_filter(window, cx));
        let menu = menu::control(
            menu::Kind::PageTools,
            self.menus.open,
            cx.entity().downgrade(),
            cx,
        );
        el! {
            .PageHeader::default [
                .div { # "page-title"; test_support; aria_label: $(title.clone()); flex_1; min_w_0; overflow_hidden } [
                    .PageTitle::default [title],
                ],
                .PageActions::default { min_w_0; flex_shrink_0 } [..search, menu],
            ]
        }
    }

    pub(super) fn sidebar_collapsed(&self) -> bool {
        self.sidebar.collapsed || self.window.layout.compact
    }

    pub(super) fn on_window_resize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layout = Layout::for_width(window.bounds().size.width);
        if layout == self.window.layout {
            return;
        }
        let page_tools_open = self.menus.open == Some(menu::Kind::PageTools);
        let menu_open = page_tools_open || self.menus.sort.is_some();
        if page_tools_open {
            self.menus.open = None;
        }
        self.menus.sort = None;
        if menu_open {
            self.window.focus.focus(window, cx);
        }
        if layout.compact {
            self.files
                .preview
                .update(cx, |preview, cx| preview.clear(cx));
        }
        self.window.layout = layout;
        cx.notify();
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
