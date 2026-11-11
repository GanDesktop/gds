use super::*;
use crate::entities::tags::{Commands, CommandsEditor};
use gds::ui::button::Button;
use gds::ui::settings::*;
use gpui_kit::component::switch::Switch;
use gpui_kit::{base::Disableable, component::Sizable};

pub(super) struct Controls {
    pub editor: Entity<CommandsEditor>,
    pub tasks: gds::service::task_manager::Scope,
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
        let work = gds::service::task_manager::Task::cancel_on_drop(
            self.tags
                .tasks
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
            .div { flex; items_start; gap_3; } [
                .div { flex_1; min_w_0; text_sm; whitespace_normal;
                    text_color: $(cx.theme().muted_foreground);
                } [status],
                "tags-detect".Button::new { label: $(text("tags-detect", cx)); small;
                    flex_shrink_0; disabled: $self.tags.pending;
                    tooltip: $(text("tags-detect", cx));
                    on_click: $(cx.listener(Self::on_detect_tags));
                },
            ],
            $(self.tags.editor.clone()),
        ] }
    }

    fn on_detect_tags(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.probe_tags(cx);
    }
}
