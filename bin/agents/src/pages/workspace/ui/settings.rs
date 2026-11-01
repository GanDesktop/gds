//! Routes management pages to the workspace's existing native window.

use super::*;
use crate::shared::i18n::tr;

impl Workspace {
    pub(super) fn open_agent_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_reply_preview(window, cx);
        self.close_menu(window, cx);
        self.record_settings_target(cx);
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        cx.defer(move |cx| {
            if !crate::features::navigation::target_page(parent, cx) {
                return;
            }
            if let Err(error) = crate::widgets::settings::window::open_catalog(cx) {
                let _ = owner.update(cx, |this, cx| {
                    this.selected.error =
                        Some(tr!("catalog-open-error", error = error.to_string()));
                    cx.notify();
                });
            }
        });
        cx.notify();
    }

    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_reply_preview(window, cx);
        self.close_menu(window, cx);
        self.record_settings_target(cx);
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        cx.defer(move |cx| {
            if !crate::features::navigation::target_page(parent, cx) {
                return;
            }
            if let Err(error) = crate::widgets::settings::window::open(cx) {
                let _ = owner.update(cx, |this, cx| {
                    this.selected.error =
                        Some(tr!("settings-open-error", error = error.to_string()));
                    cx.notify();
                });
            }
        });
        cx.notify();
    }

    pub(super) fn record_settings_target(&self, cx: &mut Context<Self>) {
        crate::features::navigation::manage(self.backend.server.clone(), cx);
        cx.update_global::<crate::entities::agent::Changes, _>(|changes, _| {
            changes.target = Some(self.window.handle.window_id());
            changes.select = None;
            changes.ready = None;
            changes.server = None;
        });
    }
}
