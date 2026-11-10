//! Deleting a virtual tag changes associations through its backend, never file targets.
use super::*;
use gds::ui::DialogActions;
use gpui_kit::assets::IconName;

const CONFIRMATION_WIDTH: f32 = 480.;

impl Browser {
    pub(in super::super) fn selected_deletable_tag(&self) -> Option<TagAddress> {
        if !self.settings.preferences.tags.enabled
            || self.operations.in_progress
            || !self.pages.is_empty()
            || self.collection() != Some(Collection::Tags)
            || !self.browse_settled()
        {
            return None;
        }
        match self.selected_place()? {
            Location::Tags(address) if !address.tag().is_empty() => Some(address),
            _ => None,
        }
    }

    pub(in super::super) fn request_delete_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(address) = self.selected_deletable_tag() else {
            return;
        };
        let browser = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let Some(owner) = browser.upgrade() else {
                return;
            };
            let focus = owner.read(cx).window.focus.clone();
            focus.focus(window, cx);
            window.open_alert_dialog(cx, move |dialog, _, cx| {
                let browser = browser.clone();
                let target = address.clone();
                let hint = address.tag().to_owned();
                let footer = el! { .DialogActions::default {
                    primary_label: $(i18n::text("tags-delete", cx));
                    primary_icon: IconName::Trash;
                    cancel_label: $(i18n::text("operation-cancel", cx)); danger;
                } };
                el! { dialog {
                    confirm; width: CONFIRMATION_WIDTH.px;
                    title: $(i18n::text("tags-delete-title", cx));
                    description: $(i18n::text("tags-delete-message", cx));
                    footer: footer;
                    on_ok: $(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = browser.update(cx, |browser, cx| browser.delete_tag(target.clone(), cx));
                        true
                    });
                } [
                    .div { # "delete-tag-name"; min_w_0; text_sm; truncate;
                        tooltip: $(move |window, cx| gds::ui::tooltip::view(hint.clone(), window, cx));
                    } [$(address.tag().to_owned())],
                ] }
            });
        });
    }

    fn delete_tag(&mut self, address: TagAddress, cx: &mut Context<Self>) {
        if self.selected_deletable_tag().as_ref() != Some(&address) {
            return;
        }
        self.operations.in_progress = true;
        let commands = self.settings.preferences.tags.commands.clone();
        let work = gds::ui::task_manager::scope(cx)
            .spawn(&self.window.runtime, async move {
                crate::entities::tags::connect(&commands)
                    .await?
                    .delete_tag(address.tag())
                    .await
            })
            .expect("tag mutation accepted");
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(|error| error.to_string())
                .and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.operations.in_progress = false;
                this.collections.preview_task = None;
                this.collections.preview_location = None;
                this.files
                    .preview
                    .update(cx, |preview, cx| preview.clear(cx));
                // Refresh even on failure: a backend operation may have partly completed.
                let tabs: Vec<_> = this
                    .tabs
                    .items
                    .iter()
                    .filter(|tab| matches!(tab.location(), Location::Tags(_)))
                    .map(|tab| tab.id)
                    .collect();
                for id in tabs {
                    this.refresh_tab(id, cx);
                }
                match result {
                    Ok(()) => notifications::success(i18n::text("tags-deleted", cx), cx),
                    Err(error) => notifications::error(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
