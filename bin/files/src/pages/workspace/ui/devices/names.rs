//! Filesystem label editing uses the shared UDisks2 operation service.

use super::*;
use gds_uim::el;

impl Browser {
    pub(in super::super) fn volume_can_rename(&self, id: &VolumeId, cx: &App) -> bool {
        self.devices
            .service
            .read(cx)
            .snapshot
            .volume(id)
            .is_some_and(|volume| {
                volume.visible && volume.can_set_label() && !self.device_is_busy(id, cx)
            })
    }

    pub(in super::super) fn request_volume_rename(
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
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(label, window, cx);
            input.select_all(window, cx);
            input
        });
        let browser = cx.entity().downgrade();
        // Open after the context menu releases focus, then focus the text field.
        window.on_next_frame(move |window, cx| {
            let Some(view) = browser.upgrade() else {
                return;
            };
            let focus = view.read(cx).window.focus.clone();
            focus.focus(window, cx);
            let focus_input = input.clone();
            window.open_alert_dialog(cx, move |dialog, _, cx| {
                let browser = browser.clone();
                let id = id.clone();
                let input = input.clone();
                let field = gds::ui::field::input(&input, cx).w_full();
                let footer = ui::DialogActions::default().primary_label(i18n::text("rename-confirm", cx)).primary_icon(IconName::Pencil).cancel_label(i18n::text("operation-cancel", cx));
                el! {
                    dialog {
                        confirm; width: 480.0.px;
                        bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                        title: $(i18n::text("volume-rename", cx));
                        description: $(i18n::text("volume-rename-message", cx));
                        icon: IconName::TextCursor.Icon::new;
                        footer: footer;
                        on_ok: $(move |_, _, cx| {
                            let label = input.read(cx).value().to_string();
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
                    } [field]
                }
            });
            window.on_next_frame(move |window, cx| {
                focus_input.update(cx, |input, cx| input.focus(window, cx));
            });
        });
    }
}
