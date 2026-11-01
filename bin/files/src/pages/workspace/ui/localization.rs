//! Locale refresh for text a control caches at construction.
//!
//! Most interface text is looked up while rendering, so installing a new locale
//! is enough. Input placeholders and the native window title are copied into
//! their owners once, so changing the language has to push the new values
//! explicitly. Input values and selections are deliberately left untouched.

use gds::i18n;
use gpui_kit::*;

use super::Browser;

impl Browser {
    /// Re-apply the active locale to cached interface text.
    pub(super) fn sync_localized_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_title(i18n::text("app-title", cx).as_ref());
        let path_placeholder = i18n::text("path-placeholder", cx);
        let filter_placeholder = i18n::text("filter-placeholder", cx);
        self.path_input.update(cx, |input, cx| {
            input.set_placeholder(path_placeholder, window, cx);
        });
        self.filter_input.update(cx, |input, cx| {
            input.set_placeholder(filter_placeholder, window, cx);
        });
        cx.notify();
    }
}
