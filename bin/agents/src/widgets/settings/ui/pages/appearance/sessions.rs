//! Session preferences share the local editor, not the appearance page's navigation.

use super::*;
use crate::shared::i18n::tr;
use crate::widgets::settings::ui::form::Row;
use gds_uim::el;

impl AppearancePage {
    pub(in crate::widgets::settings) fn matches_session_query(query: &str) -> bool {
        let query = query.trim().to_lowercase();
        [
            tr!("settings-sessions"),
            tr!("settings-sessions-help"),
            tr!("settings-confirm-delete"),
            tr!("settings-default-agent"),
            tr!("settings-auto-collapse-activity"),
            tr!("settings-auto-collapse-activity-help"),
            Field::ComposerEnter.label(),
            Field::ComposerEnter.help(),
        ]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
    }

    pub(in crate::widgets::settings) fn render_sessions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if self.reload {
            self.refresh(window, cx);
        }
        let agents = crate::widgets::settings::ui::pages::agents::shared(window, cx)
            .read(cx)
            .display_catalog();
        let selected = self
            .form
            .default_agent
            .read(cx)
            .selected_value()
            .cloned()
            .flatten();
        let mut choices: Vec<_> = std::iter::once(None)
            .chain(agents.iter().map(|spec| Some(spec.id.to_string())))
            .chain(selected.clone().map(Some))
            .collect();
        choices.sort();
        choices.dedup();
        if choices != self.form.default_agent_choices {
            self.form.default_agent_choices = choices.clone();
            self.form.default_agent.update(cx, |select, cx| {
                select.set_items(choices.into_iter().map(form::Choice).collect(), window, cx);
                select.set_selected_value(&selected, window, cx);
            });
        }
        let default_agent = Row {
            id: "settings-default-agent".into(),
            label: tr!("settings-default-agent"),
            help: tr!("settings-default-agent-help"),
            control: el! {
                .div {
                    # "settings-default-agent";
                    test_support;
                    flex_1;
                    min_w_0;
                } [
                    ($(&self.form.default_agent), cx).gds::ui::field::select {
                        small;
                        w_full;
                        cursor_pointer;
                        disabled: $(self.document.is_none());
                    },
                ]
            }
            .into_any_element(),
            reset: None,
        };
        let auto_collapse_activity = Row {
            id: "settings-auto-collapse-activity".into(),
            label: tr!("settings-auto-collapse-activity"),
            help: tr!("settings-auto-collapse-activity-help"),
            control: el! {
                ("settings-auto-collapse-activity").crate::shared::ui::checkbox {
                    accessibility_label: tr!("settings-auto-collapse-activity");
                    checked: $(self.form.auto_collapse_activity.unwrap_or(true));
                    disabled: $(self.document.is_none());
                    on_click: $(cx.listener(|this, checked: &bool, window, cx| {
                                this.form.auto_collapse_activity = Some(*checked);
                                this.flush_pending(window, cx);
                    }));
                }
            }
            .into_any_element(),
            reset: None,
        };
        let confirm_delete = Row {
            id: "settings-confirm-delete".into(),
            label: tr!("settings-confirm-delete"),
            help: tr!("settings-confirm-delete-help"),
            control: el! {
                ("settings-confirm-delete").crate::shared::ui::checkbox {
                    accessibility_label: tr!("settings-confirm-delete");
                    checked: $(!self.form.confirm_delete.unwrap_or(true));
                    disabled: $(self.document.is_none());
                    on_click: $(cx.listener(|this, checked: &bool, window, cx| {
                                this.form.confirm_delete = Some(!*checked);
                                this.flush_pending(window, cx);
                    }));
                }
            }
            .into_any_element(),
            reset: None,
        };
        el! {
            .div {
                # "session-settings";
                test_support;
                size_full;
                overflow_y_scroll;
                p_5;
                flex;
                flex_col;
                gap_2;
            } [
                .div {
                    flex;
                    flex_col;
                    gap_2;
                } [
                    .div {
                        text_size: crate::shared::ui::HEADING;
                    } [
                        tr!("settings-sessions"),
                    ],
                    .div {
                        text_size: crate::shared::ui::SMALL;
                        text_color: $(cx.theme().muted_foreground);
                    } [
                        tr!("settings-sessions-help"),
                    ],
                ],
                ..$(bool::then(Self::matches_session_query(&self.query), || el! {.div {
                                border_t_1;
                                border_color: $(cx.theme().border);
                                flex;
                                flex_col;
                            } [
                                default_agent,
                                $(self.row(Field::ComposerEnter, cx)),
                                auto_collapse_activity,
                                confirm_delete,
                ]})),
                ..$(bool::then(!Self::matches_session_query(&self.query), || el! {.div {
                                # "settings-no-results";
                                test_support;
                            } [
                                tr!("settings-no-results"),
                ]})),
            ]
        }
        .into_any_element()
    }
}
