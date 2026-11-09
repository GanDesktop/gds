use super::*;
use crate::entities::tags::{Commands, CommandsEditor};
use gds::ui::settings::*;
use gpui_kit::component::{button::Button, switch::Switch};
use gpui_kit::{base::Disableable, component::Sizable};

pub(super) struct Controls {
    pub editor: Entity<CommandsEditor>,
    pub runtime: tokio::runtime::Handle,
    pub checked: Option<Commands>,
    pub error: Option<String>,
    pub pending: bool,
    pub task: Option<Task<()>>,
}

impl View {
    pub fn tags_available(&self) -> bool {
        !self.tags.pending
            && self.tags.error.is_none()
            && self.tags.checked.as_ref() == Some(&self.preferences.tags.commands)
    }

    pub(super) fn probe_tags(&mut self, cx: &mut Context<Self>) {
        self.tags.pending = true;
        self.tags.error = None;
        let commands = self.preferences.tags.commands.clone();
        let work_commands = commands.clone();
        let work = tokio_util::task::AbortOnDropHandle::new(
            self.tags
                .runtime
                .spawn(async move { crate::entities::tags::connect(&work_commands).await }),
        );
        self.tags.task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.preferences.tags.commands != commands {
                    return;
                }
                this.tags.checked = Some(commands);
                this.tags.pending = false;
                this.tags.error = match result {
                    Ok(Ok(_)) => None,
                    Ok(Err(e)) => Some(e),
                    Err(e) => Some(e.to_string()),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn on_tags_edited(
        &mut self,
        _: &Entity<CommandsEditor>,
        event: &form::Changed<Commands>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.value == self.preferences.tags.commands {
            return;
        }
        self.change(window, cx, |p| {
            p.tags.commands = event.value.clone();
            true
        });
        self.probe_tags(cx);
    }

    pub(super) fn render_tags_settings(&self, cx: &mut Context<Self>) -> Div {
        let available = self.tags_available();
        let enabled = self.preferences.tags.enabled;
        let status = if self.tags.pending {
            text("tags-checking", cx)
        } else if available {
            text("tags-ready", cx)
        } else {
            self.tags
                .error
                .clone()
                .unwrap_or_else(|| text("tags-unavailable", cx).to_string())
                .into()
        };
        el! { .div { flex; flex_col; gap_3; mt_4; } [
            .SettingRow::default [
                .SettingLabel::default [$(text("tags-enabled", cx))],
                .SettingActions::default [
                    .SettingControl::default [
                        "tags-enabled".Switch::new {
                            checked: enabled; disabled: $(!available && !enabled);
                            accessibility_label: $(text("tags-enabled", cx));
                            on_change: $(cx.listener(|this, value: &bool, window, cx| {
                                if *value && !this.tags_available() { return; }
                                this.change(window, cx, |p| { p.tags.enabled = *value; true });
                            }));
                        },
                    ],
                ],
            ],
            $(self.tags.editor.clone()),
            .div { text_sm; text_color: $(cx.theme().muted_foreground); } [status],
            "tags-detect".Button::new { label: $(text("tags-detect", cx)); small;
                on_click: $(cx.listener(|this, _, _, cx| this.probe_tags(cx)));
            },
        ] }
    }
}
