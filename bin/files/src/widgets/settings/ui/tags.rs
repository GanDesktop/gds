use super::*;
use crate::entities::tags::{Commands, CommandsEditor};
use gds::ui::button::Button;
use gds::ui::settings::form;
use gpui_kit::{base::Disableable, component::Sizable};

pub(super) enum CheckState {
    Checking { commands: Commands, _task: Task<()> },
    Ready(Commands),
    Failed { commands: Commands, error: String },
}

pub(super) struct Controls {
    pub editor: Entity<CommandsEditor>,
    pub state: CheckState,
}

impl View {
    pub fn tags_available(&self) -> bool {
        matches!(
            &self.tags.state,
            CheckState::Ready(checked) if *checked == self.preferences.tags.commands
        )
    }

    pub(super) fn probe_tags(&mut self, cx: &mut Context<Self>) {
        let commands = self.preferences.tags.commands.clone();
        let work_commands = commands.clone();
        let work = gds::service::task_manager::Task::cancel_on_drop(
            self.tasks.spawn_named("Check tag backend", async move {
                crate::entities::tags::connect(&work_commands).await
            }),
        );
        let probe_commands = commands.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.preferences.tags.commands != probe_commands {
                    return;
                }
                this.tags.state = match result {
                    Ok(Ok(_)) => CheckState::Ready(probe_commands),
                    Ok(Err(error)) => CheckState::Failed {
                        commands: probe_commands,
                        error: error.to_string(),
                    },
                    Err(error) => CheckState::Failed {
                        commands: probe_commands,
                        error: error.to_string(),
                    },
                };
                cx.notify();
            });
        });
        self.tags.state = CheckState::Checking {
            commands,
            _task: task,
        };
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
        self.change(window, cx, |preferences| {
            preferences.tags.commands = event.value.clone();
            true
        });
        self.probe_tags(cx);
    }

    pub(super) fn render_tags_settings(&self, cx: &mut Context<Self>) -> Div {
        let available = self.tags_available();
        let enabled = self.preferences.tags.enabled;
        let pending = matches!(
            &self.tags.state,
            CheckState::Checking { commands, .. } if *commands == self.preferences.tags.commands
        );
        let status: SharedString = match &self.tags.state {
            CheckState::Checking { commands, .. }
                if *commands == self.preferences.tags.commands =>
            {
                t!(@cx, "tags-checking")
            }
            CheckState::Checking { .. } => t!(@cx, "tags-unavailable"),
            CheckState::Ready(checked) if *checked == self.preferences.tags.commands => {
                t!(@cx, "tags-ready")
            }
            CheckState::Ready(_) => t!(@cx, "tags-unavailable"),
            CheckState::Failed { commands, error }
                if *commands == self.preferences.tags.commands =>
            {
                error.clone().into()
            }
            CheckState::Failed { .. } => t!(@cx, "tags-unavailable"),
        };
        let enabled_row = self.toggle_row(Field::TagsEnabled, !available && !enabled, cx);
        let detect_label = t!(@cx, "tags-detect");
        let on_detect = cx.listener(Self::on_detect_tags);
        el! { .div { flex; flex_col; gap_3; mt_4; } [
            enabled_row,
            .div { flex; items_start; gap_3; } [
                .div {
                    flex_1; min_w_0; text_sm; whitespace_normal;
                    text_color: $(cx.theme().muted_foreground);
                } [status],
                "tags-detect".Button::new {
                    label: $(detect_label.clone());
                    small;
                    flex_shrink_0;
                    disabled: pending;
                    tooltip: detect_label;
                    on_click: on_detect;
                },
            ],
            $(self.tags.editor.clone()),
        ] }
    }

    fn on_detect_tags(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.probe_tags(cx);
    }
}
