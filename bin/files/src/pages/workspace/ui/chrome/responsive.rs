//! Width-based workspace presentation, independent of saved location preferences.
use super::menu::MenuControl;
use super::*;
use gds::ui::page::PageHeader;

// Approximate room for the expanded sidebar, details, a short title and full toolbar.
const COMPACT_WIDTH: f32 = 1080.;

#[derive(Clone, Copy, PartialEq)]
pub(in crate::pages::workspace) struct Layout {
    pub width: f32,
    pub compact: bool,
}

impl Layout {
    pub fn for_width(width: Pixels) -> Self {
        let width = f32::from(width);
        Self {
            width,
            compact: width < COMPACT_WIDTH,
        }
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn compact_page_header(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PageHeader {
        use gds::ui::page::*;
        use gds_uim::el;
        let title = self.page_title(cx);
        let search = self
            .collection()
            .is_none()
            .then(|| self.directory_filter(window, cx));
        let menu = el! { .MenuControl::default {
            kind: menu::Kind::PageTools; open: $(self.menus.open);
            view: $(cx.entity().downgrade());
        } };
        el! {
            .PageHeader::default [
                .div { # "page-title"; test_support; aria_label: $(title.clone()); flex_1; min_w_0; overflow_hidden } [
                    .PageTitle::default [title],
                ],
                .PageActions::default { min_w_0; flex_shrink_0 } [..search, menu],
            ]
        }
    }

    pub(in crate::pages::workspace) fn sidebar_collapsed(&self) -> bool {
        self.sidebar.collapsed || self.window.layout.compact
    }

    pub(in crate::pages::workspace) fn on_window_resize(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let layout = Layout::for_width(window.bounds().size.width);
        if layout == self.window.layout {
            return;
        }
        let compact_changed = layout.compact != self.window.layout.compact;
        self.window.layout = layout;
        if compact_changed {
            if self.menus.close_for_resize() {
                self.window.focus.focus(window, cx);
            }
            if layout.compact {
                self.files
                    .preview
                    .update(cx, |preview, cx| preview.clear(cx));
            }
        }
        cx.notify();
    }
}
