use crate::shared::i18n::tr;
use bas::model::{Role, ToolCallStatus};
use gds_uim::el;
use gpui_kit::ClickEvent;

use super::composer::COMPOSER_CONTROL_INSET;
use super::*;

pub(super) mod content;
mod resource;

pub(super) const CONTENT_WIDTH: f32 = 840.;
pub(super) const THREAD_BODY_PADDING: f32 = 12.;
const INITIALIZATION_MESSAGES_MAX_HEIGHT: f32 = 160.;
const APPROVAL_PANEL_HEIGHT: f32 = 240.;
const CONTROL_PANEL_PADDING: f32 = 12.;
const CONTROL_PANEL_BOTTOM_MARGIN: f32 = 16.;

pub(super) fn content_panel() -> Div {
    el! {
        .div {
            w_full;
            min_w_0;
            max_w: $(px(CONTENT_WIDTH));
            mx_auto;
        }
    }
}

impl Workspace {
    fn work_status(&self) -> String {
        if let Some(conversation) = &self.selected.timeline.conversation {
            if matches!(
                conversation.foreground_state,
                Some(bas::acp_v2::StateUpdate::RequiresAction(_))
            ) {
                return tr!("status-agent-confirming").into();
            }
            for item in conversation.items.iter().rev() {
                match item {
                    Item::Message {
                        message,
                        streaming: true,
                    } if message.role == Role::Thought => {
                        return tr!("status-thinking").into();
                    }
                    Item::Message {
                        message,
                        streaming: true,
                    } if message.role == Role::Agent => {
                        return tr!("status-responding").into();
                    }
                    Item::Tool(tool)
                        if matches!(
                            tool.status,
                            ToolCallStatus::Pending | ToolCallStatus::InProgress
                        ) =>
                    {
                        return tr!("status-tool", tool = (tool.title).to_string());
                    }
                    _ => {}
                }
            }
        }
        tr!("status-thinking").into()
    }

    pub(super) fn render_chat(&self, cx: &mut Context<Self>) -> AnyElement {
        el! {
            .div {
                # "thread-body";
                test_support;
                flex;
                flex_col;
                flex_1;
                min_h_0;
                w_full;
                when: $(!self.window.compact), {
                    pl: $(px(THREAD_BODY_PADDING));
                    pr: $(px(turn_navigation::RAIL_WIDTH + THREAD_BODY_PADDING));
                };
            } [
                ..$(self.render_notices(cx)),
                ..$((!self.composer_fills_thread()).then(|| self.render_timeline(cx))),
                ..$(self.composer_fills_thread().then(|| self.render_initialization_messages(cx)).flatten()),
                $(self.render_control_panel(cx)),
            ]
        }.into_any_element()
    }

    fn render_control_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let fills_thread = self.composer_fills_thread();
        let surface =
            gds::ui::surface::glass_panel(cx.global::<theme::Appearance>().content_opacity, cx);
        el! {
            ().content_panel {
                # "thread-composition";
                test_support;
                flex;
                flex_col;
                flex_shrink_0;
                pt: CONTROL_PANEL_PADDING.px;
                pb: CONTROL_PANEL_BOTTOM_MARGIN.px;
                when: fills_thread, {
                    flex_1;
                    min_h_0;
                };
            } [
                surface {
                    # "conversation-control-panel";
                    test_support;
                    w_full;
                    min_w_0;
                    flex;
                    flex_col;
                    p: CONTROL_PANEL_PADDING.px;
                    when: fills_thread, {
                        flex_1;
                        min_h_0;
                    };
                } [
                    $(self.render_control_header(cx)),
                    $(self.render_composer(cx)),
                ],
            ]
        }
        .into_any_element()
    }

    fn render_control_header(&self, cx: &mut Context<Self>) -> AnyElement {
        el! {
            .div {
                # "conversation-status";
                test_support;
                w_full;
                min_w_0;
                flex;
                flex_shrink_0;
                items_center;
                gap_2;
                px: COMPOSER_CONTROL_INSET.px;
                mb_2;
            } [
                $(self.agent_button(cx)),
                ..$(self.render_plan_indicator(cx)),
                $(self.session_indicator("conversation-connection", cx)),
                .div {
                    flex_1;
                    min_w_0;
                } [
                    ..$(self.activity_label().map(|label| el! {$(caption(label, cx)) {
                        truncate;
                    }})),
                ],
                .div {
                    flex;
                    items_center;
                    flex_shrink_0;
                    gap_1;
                } [
                    ..$((self.selected.id.is_some() && !self.composer_fills_thread())
                        .then(|| self.render_conversation_navigation(cx))),
                    $(self.composer_expansion_button(cx)),
                ],
            ]
        }
        .into_any_element()
    }

    pub(super) fn render_conversation_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        el! {
            .div {
                # "conversation-navigation";
                test_support;
                flex;
                items_center;
                gap_1;
            } [
                $(message_actions::action_button("scroll-to-top", IconName::ArrowUp, cx)) {
                    accessibility_label: tr!("conversation-top");
                    tooltip_with_shortcut: tr!("conversation-top"), $(shortcuts::shortcut(shortcuts::ScrollConversationToTop));
                    on_click: $(cx.listener(Self::on_chat_top));
                },
                $(message_actions::action_button("scroll-to-bottom", IconName::ArrowDown, cx)) {
                    accessibility_label: tr!("conversation-latest");
                    tooltip_with_shortcut: tr!("conversation-latest"), $(shortcuts::shortcut(ScrollConversationToEnd));
                    on_click: $(cx.listener(Self::on_chat_bottom));
                },
            ]
        }
        .into_any_element()
    }

    fn on_chat_top(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_conversation_to_top(cx);
    }

    fn on_chat_bottom(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_conversation_to_end(cx);
    }

    fn render_initialization_messages(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let conversation = self
            .selected
            .timeline
            .conversation
            .as_ref()
            .filter(|state| !state.rows.is_empty())?;
        Some(
            el! {
                ().content_panel {
                    # "agent-initialization-messages";
                    test_support;
                    flex_shrink_0;
                    max_h: $(px(INITIALIZATION_MESSAGES_MAX_HEIGHT));
                    overflow_y_scroll;
                    border_b_1;
                    border_color: $(cx.theme().border);
                } [
                    $(caption(tr!("conversation-startup"), cx)) {
                        px_3;
                        pt_2;
                    },
                    ..$((0..conversation.rows.len()).map(|index| self.render_item(index, cx))),
                ]
            }
            .into_any_element(),
        )
    }

    pub(super) fn render_timeline(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.awaiting_connection() {
            return self.render_connection_transition(cx);
        }
        let awaiting_history =
            self.selected.awaiting_history() && self.selected.authentication.panel.is_none();
        let empty = self
            .selected
            .timeline
            .conversation
            .as_ref()
            .is_none_or(|conversation| conversation.items.is_empty());
        el! {
            .div {
                # "conversation-timeline";
                test_support;
                flex;
                flex_col;
                flex_1;
                min_h_0;
                w_full;
            } [
                ..$(awaiting_history.then(|| self.render_history_transition(cx))),
                ..$((empty && !awaiting_history && self.selected.authentication.panel.is_none()).then(|| self.render_empty_timeline(cx))),
                ..$((!empty).then(|| self.render_timeline_list(cx))),
                ..$((!self.selected.approval.panel.is_empty()).then(|| self.render_timeline_approval(cx))),
                ..$(self.render_queue_panel(cx)),
                ..$(self.selected.authentication.panel.as_ref().map(|panel| {
                            el! {().content_panel {
                                    # "thread-authentication";
                                    test_support;
                                    p_4;
                                    my_3;
                                    flex_shrink_0;
                                    min_w_0;
                                    when: empty, {
                                        my_auto;
                                    };
                                    border_1;
                                    rounded_md;
                                    border_color: $(cx.theme().border);
                                    bg: $(cx.theme().secondary);
                                    text_color: $(cx.theme().secondary_foreground);
                                    max_h: $(px(360.));
                                    overflow_y_scroll;
                                } [
                                    $(panel.entity.clone()),
                            ]}
                })),
                ..$((!self.selected.elicitations.is_empty()).then(|| {
                            el! {.div {
                                    # "thread-elicitations";
                                    test_support;
                                    when: $(self.window.compact), {
                                        px_3;
                                    };
                                    py_2;
                                    max_h: $(px(360.));
                                    overflow_y_scroll;
                                } [
                                    $(content_panel()) [
                                        ..$(self.selected.elicitations.values().map(|panel| panel.entity.clone())),
                                    ],
                            ]}
                })),
            ]
        }.into_any_element()
    }

    fn render_history_transition(&self, cx: &mut Context<Self>) -> AnyElement {
        let (label, loading) = match &self.selected.stream {
            StreamState::Loading | StreamState::Live => (tr!("status-loading"), true),
            StreamState::Reconnecting(_) => (tr!("status-reconnecting"), true),
            StreamState::Unavailable(_) => (tr!("status-session-unavailable"), false),
            StreamState::Closed => (tr!("status-session-ended"), false),
        };
        let failure = match &self.selected.stream {
            StreamState::Unavailable(error) | StreamState::Reconnecting(error) => {
                Some(error.presentation())
            }
            _ => None,
        };
        let retry = !loading && self.selected.id.is_some();
        el! {
            ().content_panel {
                # "conversation-history-transition";
                test_support;
                role: gpui_kit::Role::Status;
                aria_label: label;
                flex_1;
                min_h_0;
                flex;
                flex_col;
                items_center;
                justify_center;
                gap_3;
                p_5;
                text_size: crate::shared::ui::SMALL;
                text_color: $(cx.theme().muted_foreground);
            } [
                ..$(loading.then(|| status::activity_icon(
                            "conversation-history-loading", IconName::LoaderCircle, cx.theme().muted_foreground, true
                ))),
                label,
                ..$(failure.map(|message| crate::shared::ui::feedback(
                            "history-load-error", message, cx.theme().danger
                ))),
                ..$(retry.then(|| el! {("history-load-retry").crate::shared::ui::action_button {
                                label: $(self.history_retry_label());
                                disabled: $(self.history_retry_disabled());
                                on_click: $(cx.listener(|this, _, _, cx| this.retry_history(cx)));
                }})),
            ]
        }.into_any_element()
    }

    fn render_empty_timeline(&self, cx: &mut Context<Self>) -> AnyElement {
        let label = if self.selected.creating {
            tr!("status-creating")
        } else if self.selected.waiting_for_agent_selection {
            tr!("conversation-choose-agent")
        } else if self.selected.id.is_some() || self.window.compact {
            tr!("conversation-empty")
        } else {
            tr!("conversation-start")
        };
        el! {
            .div {
                # "empty-conversation-message";
                test_support;
                w_full;
                flex_1;
                min_h_0;
                flex;
                flex_col;
                justify_center;
                items_center;
                gap_2;
                p_5;
            } [
                .gpui_kit::component::empty::Empty::default { p_0; gap_2 } [
                .gpui_kit::component::empty::EmptyDescription::default { text_size: crate::shared::ui::SMALL } [label],
                ..$((!self.window.compact && self.selected.id.is_none()).then(|| {
                            caption(navigation::groups::path_label(self.navigation.selected_directory.as_deref()), cx)
                })),
                ],
            ]
        }
        .into_any_element()
    }

    fn render_timeline_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        el! {
            .div {
                relative;
                flex_1;
                min_h_0;
                w_full;
                on_scroll: $(cx.listener(Self::on_navigation_wheel));
            } [
                $(list(self.selected.timeline.list.clone(), move |index, _, cx| {
                            view.update(cx, |this, cx| this.render_item(index, cx))
                })) {
                    size_full;
                },
                .div {
                    absolute;
                    inset_0;
                    when: $(self.window.compact), {
                        pl: THREAD_BODY_PADDING.px;
                        pr: $(turn_navigation::RAIL_WIDTH + THREAD_BODY_PADDING).px;
                    };
                } [
                    ().content_panel {
                        # "timeline-navigation-anchor";
                        test_support;
                        relative;
                        h_full;
                    } [
                        $(self.render_turn_navigation(cx)),
                    ],
                ],
                .div {
                    # "timeline-scrollbar";
                    test_support;
                    absolute;
                    top_0;
                    bottom_0;
                    w: THREAD_BODY_PADDING.px;
                    right: $(if self.window.compact { 0. } else { -turn_navigation::RAIL_WIDTH - THREAD_BODY_PADDING }).px;
                } [
                    $(gpui_kit::base::Scrollbar::vertical(&self.selected.timeline.list)
                        .viewport_from_layout()
                        .mode(gpui_kit::base::ScrollbarMode::Always)),
                ],
                ..$(self.render_older_history(cx)),
                ..$(self.render_history_location_status(cx)),
            ]
        }
        .into_any_element()
    }

    fn render_timeline_approval(&self, cx: &mut Context<Self>) -> AnyElement {
        let session = self.selected.id;
        let epoch = self.selected.epoch;
        el! {
            .div {
                # "approval-panel";
                test_support;
                when: $(self.window.compact), {
                    px_3;
                };
                py_2;
                flex_shrink_0;
                max_h: $(px(APPROVAL_PANEL_HEIGHT));
                overflow_y_scroll;
            } [
                $(content_panel()) [
                    $(self.selected.approval.panel.render(cx, move |this, request_id, outcome, _, cx| {
                                if let Some(session) = session {
                                    this.respond_permission(session, epoch, request_id, outcome, cx);
                                }
                    })),
                ],
            ]
        }.into_any_element()
    }

    pub(super) fn stream_label(&self) -> String {
        if self.composer.paste_pending {
            return tr!("status-pasting").into();
        }
        if !self.selected.approval.panel.is_empty() {
            return tr!("status-approval").into();
        }
        if self.selected.approval.error.is_some() {
            return tr!("status-approval-unavailable").into();
        }
        if !self.selected.elicitations.is_empty() {
            return tr!("status-input").into();
        }
        if !self.backend.connected {
            return if self.backend.error.is_some() {
                tr!("status-reconnecting").into()
            } else {
                tr!("status-connecting").into()
            };
        }
        if self.selected.id.is_none() {
            return if self.selected.creating {
                tr!("status-creating").into()
            } else if self.selected.waiting_for_agent_selection {
                tr!("status-select-agent").into()
            } else if self.selected.authentication.required.is_some() {
                tr!("status-auth").into()
            } else if self.backend.agents.is_empty() {
                tr!("status-no-agents").into()
            } else {
                tr!(
                    "composer-shortcuts-custom",
                    send = self.composer_send_shortcut(),
                    newline = self.composer_newline_shortcut()
                )
            };
        }
        if self
            .selected
            .id
            .is_some_and(|id| !self.sessions.healthy(id))
        {
            return tr!("status-failed").into();
        }
        if self
            .selected
            .id
            .is_some_and(|id| self.sessions.pending.configuring.contains(&id))
        {
            return tr!("status-applying").into();
        }
        if self
            .selected
            .id
            .and_then(|id| self.sessions.outboxes.get(&id))
            .is_some_and(|outbox| outbox.unknown().is_some())
        {
            return tr!("status-submission-unknown").into();
        }
        if self.selected.id.is_some_and(|id| self.sending(id)) {
            return tr!("status-sending").into();
        }
        match &self.selected.stream {
            StreamState::Loading => tr!("status-loading").into(),
            StreamState::Live if self.working() => self.work_status(),
            StreamState::Live => tr!("status-synced").into(),
            StreamState::Reconnecting(_) => tr!("status-reconnecting").into(),
            StreamState::Unavailable(_) => tr!("status-session-unavailable").into(),
            StreamState::Closed => tr!("status-session-ended").into(),
        }
    }

    fn render_item(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(conversation) = self.selected.timeline.conversation.as_ref() else {
            return el! {
                .div
            }
            .into_any_element();
        };
        let Some(row) = conversation.rows.get(index) else {
            return el! {
                .div
            }
            .into_any_element();
        };
        let items = &conversation.items[row.items.clone()];
        let item = &items[0];
        let grouped = row.activity && items.len() > 1;
        let id = if grouped {
            format!("activity-{}", item.id())
        } else {
            item.id()
        };
        let content = if grouped {
            self.render_activity(&id, items, index, cx)
        } else {
            self.render_entry(item, index, cx)
        };
        let panel_id = SharedString::from(format!("message-content-{id}"));
        el! {
            .div {
                # $(SharedString::from(id));
                test_support;
                when: $(self.window.compact), {
                    pl: $(px(THREAD_BODY_PADDING));
                    pr: $(px(turn_navigation::RAIL_WIDTH + THREAD_BODY_PADDING));
                };
                py_1;
                w_full;
                min_w_0;
                text_size: $(theme::content_font_size(cx));
            } [
                $(content_panel()) {
                    # panel_id;
                    test_support;
                } [
                    content,
                    ..$(self.render_message_actions(
                            items
                            .last()
                            .expect("timeline rows contain at least one item"),
                            cx,
                    )),
                ],
            ]
        }
        .into_any_element()
    }
}
