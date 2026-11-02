use crate::entities::session::status::Status as RowStatus;
use crate::shared::i18n::tr;
use bas::model::Timestamp;
use gds_uim::el;
use gpui_kit::component::menu::PopupMenuItem;

use crate::pages::workspace::ui::*;
use crate::shared::ui::time::recent_time;

impl Workspace {
    pub(super) fn session_row_status(&self, session: &SessionMeta) -> RowStatus {
        if !self.sessions.healthy(session.id) {
            return RowStatus::Failed;
        }
        RowStatus::for_session(
            session,
            self.sessions
                .seen_replies
                .get(&session.id)
                .copied()
                .unwrap_or(0),
            self.selected.id == Some(session.id) && self.has_unread_reply(),
        )
    }

    pub(super) fn thread_row(
        &self,
        session: &SessionMeta,
        cycle: &super::CycleTargets,
        presentation: gds::ui::sidebar::SidebarPresentation,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = session.id;
        let editing = self.renaming_thread(id);
        let pinned = self.navigation.pinned.contains(&id);
        let bookmarked = self.navigation.bookmarks.contains(&id);
        let selected = self.selected.id == Some(id);
        let state = self.session_row_status(session);
        let group = SharedString::from(format!("thread-row-{id}"));
        let menu_session = session.clone();
        let title_content = if editing {
            self.render_title_editor(cx)
        } else {
            el! {
                .div {
                    # format!("thread-title-text-{id}");
                    test_support;
                    min_w_0;
                    truncate;
                    text_size: crate::shared::ui::SMALL;
                    on_click: $(cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                                if event.click_count() == 2 {
                                    this.on_thread_row_rename(id, window, cx);
                                }
                    }));
                } [
                    $(title(session)),
                ]
            }
            .into_any_element()
        };
        let bookmark = (bookmarked && !editing).then(|| {
            el! {
                .div {
                    # format!("thread-bookmark-{id}");
                    test_support;
                    flex_shrink_0;
                } [
                    (IconName::Bookmark).Icon::new {
                        xsmall;
                        text_color: $(cx.theme().primary);
                    },
                ]
            }
        });
        let title_row = el! {
            .div {
                # $(SharedString::from(format!("thread-title-{id}")));
                test_support;
                h: layout::ICON_BUTTON_SIZE.px;
                flex_1;
                min_w_0;
                flex;
                items_center;
                gap_1;
            } [
                title_content,
                ..bookmark,
            ]
        };
        let actions = (!editing && presentation.actions_visible()).then(|| {
            el! {
                .div {
                    # $(SharedString::from(format!("thread-actions-{id}")));
                    test_support;
                    flex;
                    items_center;
                    size: layout::ICON_BUTTON_SIZE.px;
                } [
                    $(self.thread_visibility_button(session, cx)),
                ]
            }
            .into_any_element()
        });
        let trailing = if presentation.actions_visible() {
            gds::ui::hover_action(
                group.clone(),
                state.render(
                    format!("thread-status-{id}"),
                    format!("thread-indicator-{id}"),
                    cx,
                ),
                actions,
            )
        } else {
            div().size(px(layout::ICON_BUTTON_SIZE)).into_any_element()
        };
        let hint = format!(
            "{}\n{} · {} · {}\n{}",
            title(session),
            session.agent,
            super::groups::name(session.cwd.as_deref()),
            if session.archived {
                tr!("session-archived")
            } else {
                state.label()
            },
            recent_time(session.updated_at, Timestamp::now(), cx)
        );
        let agent = gds::ui::with_pin(
            self.agent_icon(format!("thread-agent-{id}"), session.agent.as_str(), cx)
                .size(px(gds::appearance::tokens::SIDEBAR_ICON_SIZE))
                .text_color(if selected {
                    cx.theme().sidebar_accent_foreground
                } else {
                    cx.theme().sidebar_foreground
                }),
            pinned,
            cx,
        );
        let badge = (!presentation.actions_visible())
            .then(|| state.badge(format!("thread-icon-status-{id}"), cx))
            .flatten()
            .map(|badge| {
                div()
                    .absolute()
                    .bottom_0()
                    .right_0()
                    .opacity(1.0 - presentation.progress())
                    .child(badge)
            });
        el! {
            .div {
                # $(SharedString::from(format!("session-{id}")));
                test_support;
                group: group;
                role: gpui_kit::Role::Button;
                aria_label: $(title(session));
                relative;
                w_full;
                flex_shrink_0;
                h: gds::appearance::tokens::SIDEBAR_ITEM_HEIGHT.px;
                pl: $(px(presentation.interpolate(8., 10.)));
                pr_2;
                overflow_hidden;
                rounded: 6.0.px;
                tooltip_show_delay: gds::appearance::tokens::TOOLTIP_DELAY;
                when: $(self.window.menu.is_none()), {
                    hoverable_tooltip: $(move |window, cx| crate::shared::ui::tooltip::view(hint.clone(), window, cx));
                };
                cursor_pointer;
                flex;
                items_center;
                gap_2;
                text_color: $(cx.theme().sidebar_foreground);
                when: selected, {
                    bg: $(cx.theme().sidebar_accent);
                    text_color: $(cx.theme().sidebar_accent_foreground);
                };
                when: $(!selected), {
                    hover: {
                        bg: $(cx.theme().list_hover);
                    };
                };
                on_click: $(cx.listener(move |this, _, window, cx| {
                    this.select(id, window, cx);
                    if this.navigation.search_expanded {
                        this.focus_input(window, cx);
                    }
                }));
                on_mouse_down: gpui_kit::MouseButton::Right, $(cx.listener(move |this, event, window, cx| {
                            this.on_thread_row_context_menu(&menu_session, event, window, cx);
                }));
            } [
                ..$(cycle.hints(id, presentation.progress() == 0.0)),
                .div {
                    # format!("thread-icon-{id}");
                    test_support;
                    relative;
                    h: layout::ICON_BUTTON_SIZE.px;
                    flex;
                    items_center;
                    flex_shrink_0;
                } [
                    agent,
                    ..badge,
                ],
                .div { flex_1; min_w_0; overflow_hidden; opacity: $(presentation.label_opacity()); } [title_row],
                .div { flex_shrink_0; when: $(!presentation.actions_visible()), { invisible; }; } [trailing],
            ]
        }.into_any_element()
    }

    fn on_thread_row_rename(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if !self.backend.connected
            || self.sessions.pending.archiving.contains(&id)
            || self.sessions.pending.deleting.contains(&id)
        {
            return;
        }
        self.begin_sidebar_rename(id, window, cx);
    }

    pub(super) fn on_thread_row_context_menu(
        &mut self,
        session: &SessionMeta,
        event: &gpui_kit::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.stop_propagation();
        self.open_thread_menu(session, event.position, window, cx);
    }

    fn thread_visibility_button(&self, session: &SessionMeta, cx: &mut Context<Self>) -> Button {
        let id = session.id;
        if session.archived || !self.sessions.is_draft(id) {
            return self.archive_button(session, "archive", cx);
        }
        el! {
            ($(SharedString::from(format!("discard-{id}"))), IconName::Trash).layout::icon_button {
                accessibility_label: tr!("draft-discard");
                tooltip: tr!("draft-discard");
                disabled: $(!self.backend.connected
                    || self.sessions.pending.archiving.contains(&id)
                    || self.sessions.pending.deleting.contains(&id));
                loading: $(self.sessions.pending.deleting.contains(&id));
                on_click: $(cx.listener(move |this, _, window, cx| {
                            this.on_thread_row_discard(id, window, cx);
                }));
            }
        }
    }

    fn on_thread_row_discard(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.discard_session_draft(id, window, cx);
    }

    fn open_thread_menu(
        &mut self,
        session: &SessionMeta,
        position: gpui_kit::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = session.id;
        let archived = session.archived;
        let draft = !archived && self.sessions.is_draft(id);
        let pinned = self.navigation.pinned.contains(&id);
        let bookmarked = self.navigation.bookmarks.contains(&id);
        let disabled = !self.backend.connected
            || self.sessions.pending.archiving.contains(&id)
            || self.sessions.pending.deleting.contains(&id);
        let state = self.session_row_status(session);
        let can_close = !archived
            && !matches!(
                session.status,
                SessionStatus::Stopped | SessionStatus::Failed
            );
        let stopped = !archived && session.status == SessionStatus::Stopped;
        let can_restart = self.can_restart_agent_session(id);
        let runtime_busy = self.sessions.pending.closing.contains(&id)
            || self.sessions.pending.restarting.contains(&id);
        let owner = cx.entity().downgrade();
        self.open_context_menu(
            position,
            (if state == RowStatus::Unread { 7 } else { 6 }) - usize::from(draft)
                + usize::from(can_close)
                + usize::from(stopped),
            std::rc::Rc::new(move |menu, window, cx| {
                let parent = window.window_handle();
                let pin_owner = owner.clone();
                let bookmark_owner = owner.clone();
                let rename_owner = owner.clone();
                let archive_owner = owner.clone();
                let delete_owner = owner.clone();
                let read_owner = owner.clone();
                let close_owner = owner.clone();
                let restart_owner = owner.clone();
                menu.item(
                    shortcuts::menu_item(
                        if pinned {
                            tr!("session-unpin")
                        } else {
                            tr!("session-pin")
                        },
                        Some(Box::new(shortcuts::ToggleThreadPin)),
                    )
                    .icon(IconName::Pin)
                    .on_click(move |_, window, cx| {
                        menus::in_parent(parent, window, cx, |_, cx| {
                            let _ = pin_owner.update(cx, |this, cx| this.toggle_pinned(id, cx));
                        });
                    }),
                )
                .item(
                    shortcuts::menu_item(
                        if bookmarked {
                            tr!("session-unbookmark")
                        } else {
                            tr!("session-bookmark")
                        },
                        Some(Box::new(shortcuts::ToggleThreadBookmark)),
                    )
                    .icon(IconName::Bookmark)
                    .on_click(move |_, window, cx| {
                        menus::in_parent(parent, window, cx, |_, cx| {
                            let _ =
                                bookmark_owner.update(cx, |this, cx| this.toggle_bookmark(id, cx));
                        });
                    }),
                )
                .item(
                    shortcuts::menu_item(
                        tr!("session-rename"),
                        Some(Box::new(shortcuts::RenameThread)),
                    )
                    .icon(IconName::Pencil)
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        menus::in_parent(parent, window, cx, |window, cx| {
                            let _ = rename_owner.update(cx, |this, cx| {
                                this.begin_sidebar_rename(id, window, cx);
                            });
                        });
                    }),
                )
                .item(
                    shortcuts::menu_item(
                        if draft {
                            tr!("draft-discard")
                        } else if archived {
                            tr!("session-restore")
                        } else {
                            tr!("session-archive")
                        },
                        (!draft).then(|| {
                            Box::new(shortcuts::ArchiveThread) as Box<dyn gpui_kit::Action>
                        }),
                    )
                    .icon(if draft {
                        crate::shared::ui::delete_icon(cx)
                    } else if archived {
                        Icon::new(IconName::ArchiveRestore)
                    } else {
                        Icon::new(IconName::Archive)
                    })
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        menus::in_parent(parent, window, cx, |window, cx| {
                            let _ = archive_owner.update(cx, |this, cx| {
                                this.on_thread_row_menu_visibility(id, draft, archived, window, cx);
                            });
                        });
                    }),
                )
                .when(state == RowStatus::Unread, |menu| {
                    menu.item(PopupMenuItem::new(tr!("session-mark-read")).on_click(
                        move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |_, cx| {
                                let _ =
                                    read_owner.update(cx, |this, cx| this.mark_thread_read(id, cx));
                            });
                        },
                    ))
                })
                .when(can_close, |menu| {
                    menu.item(
                        shortcuts::menu_item(
                            tr!("agent-session-close"),
                            Some(Box::new(shortcuts::ToggleSession)),
                        )
                        .icon(IconName::Square)
                        .disabled(disabled || runtime_busy)
                        .on_click(move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |_, cx| {
                                let _ = close_owner.update(cx, |this, cx| {
                                    this.request_close_agent_session(id, cx);
                                });
                            });
                        }),
                    )
                })
                .when(stopped, |menu| {
                    menu.item(
                        shortcuts::menu_item(
                            tr!("agent-session-start"),
                            Some(Box::new(shortcuts::ToggleSession)),
                        )
                        .icon(IconName::Play)
                        .disabled(!can_restart)
                        .on_click(move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |_, cx| {
                                let _ = restart_owner.update(cx, |this, cx| {
                                    this.request_restart_agent_session(id, cx);
                                });
                            });
                        }),
                    )
                })
                .when(!draft, |menu| {
                    menu.separator().item(
                        shortcuts::menu_item(
                            tr!("session-delete"),
                            Some(Box::new(shortcuts::DeleteThread)),
                        )
                        .icon(crate::shared::ui::delete_icon(cx))
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ = delete_owner
                                    .update(cx, |this, cx| this.request_delete(id, window, cx));
                            });
                        }),
                    )
                })
            }),
            window,
            cx,
        );
    }

    fn on_thread_row_menu_visibility(
        &mut self,
        id: SessionId,
        draft: bool,
        archived: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if draft {
            self.discard_session_draft(id, window, cx);
        } else {
            self.set_archived(id, !archived, cx);
        }
    }
}
