//! Filesystem label editing uses the shared UDisks2 operation service.

use super::*;
use gds::ui::DialogActions;
use gds_uim::el;

impl Browser {
    pub(in crate::pages::workspace) fn volume_can_rename(&self, id: &VolumeId, cx: &App) -> bool {
        self.devices
            .service
            .read(cx)
            .snapshot
            .volume(id)
            .is_some_and(|volume| {
                volume.visible && volume.can_set_label() && !self.device_is_busy(id, cx)
            })
    }

    pub(in crate::pages::workspace) fn request_volume_rename(
        &mut self,
        id: VolumeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.volume_can_rename(&id, cx) || window.has_active_dialog(cx) {
            return;
        }
        let volume = self
            .devices
            .service
            .read(cx)
            .snapshot
            .volume(&id)
            .expect("rename target was checked above");
        let label = volume.label.clone();
        let form = cx.new(|cx| {
            crate::pages::workspace::ui::name_form::NameForm::new(
                label.into(),
                i18n::text("volume-rename", cx),
                window,
                cx,
            )
        });
        let browser = cx.entity().downgrade();
        // Open after the context menu releases focus, then focus the text field.
        window.on_next_frame(move |window, cx| {
            let Some(view) = browser.upgrade() else {
                return;
            };
            let focus = view.read(cx).window.focus.clone();
            focus.focus(window, cx);
            let focus_form = form.clone();
            window.open_alert_dialog(cx, move |dialog, _, cx| {
                let browser = browser.clone();
                let id = id.clone();
                let form = form.clone();
                let content = form.clone();
                let footer = el! { .DialogActions::default {
                    primary_label: $(i18n::text("rename-confirm", cx));
                    primary_icon: IconName::Pencil;
                    cancel_label: $(i18n::text("operation-cancel", cx));
                } };
                el! {
                    dialog {
                        confirm; width: 480.0.px;
                        bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                        title: $(i18n::text("volume-rename", cx));
                        description: $(i18n::text("volume-rename-message", cx));
                        icon: IconName::TextCursor.Icon::new;
                        footer: footer;
                        on_ok: $(move |_, _, cx| {
                            let label = form.read(cx).chosen_name(cx).to_string_lossy().into_owned();
                            browser.update(cx, |browser, cx| {
                                if browser.volume_can_rename(&id, cx) {
                                    let unchanged = browser.devices.service.read(cx).snapshot.volume(&id)
                                        .is_some_and(|volume| volume.label == label);
                                    if !unchanged {
                                        browser.start_device_operation(DeviceAction::SetLabel { volume: id.clone(), label }, cx);
                                    }
                                } else {
                                    notifications::error(i18n::text("device-path-unavailable", cx), cx);
                                }
                                true
                            }).unwrap_or(true)
                        });
                    } [content]
                }
            });
            window.on_next_frame(move |window, cx| {
                focus_form.update(cx, |form, cx| form.focus_input(window, cx));
            });
        });
    }
}
