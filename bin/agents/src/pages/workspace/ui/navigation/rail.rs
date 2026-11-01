use super::{View, groups, history};
use crate::entities::session::status::Status;
use crate::pages::workspace::ui::*;
use crate::shared::i18n::tr;
use gds::ui::sidebar::*;
use gpui_kit::Stateful;

const BADGE_INSET: f32 = 2.;

impl Workspace {
    pub(in crate::pages::workspace) fn render_sidebar_rail(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cycle = self.thread_cycle_targets(cx);
        let actions = self.sidebar_actions(true, cx).into_iter().map(|action| {
            div()
                .w_full()
                .h(px(gds::appearance::tokens::SIDEBAR_ITEM_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(action)
        });
        let sessions = self.visible_sessions();
        let query = self.navigation.search.read(cx).value().to_lowercase();
        let sessions: Vec<_> = if self.navigation.view.flat() {
            history::sorted(&sessions)
                .into_iter()
                .filter(|session| matches_query(session, &query))
                .collect()
        } else {
            self.grouped_threads(&sessions, &query)
                .into_iter()
                .flat_map(|group| group.threads)
                .collect()
        };
        let draft = (self.navigation.view == View::Workspaces && self.selected.id.is_none())
            .then(|| self.rail_draft(cx));
        let empty = (sessions.is_empty() && draft.is_none()).then(|| {
            self.rail_item(
                "rail-empty".into(),
                false,
                tr!("session-no-results").into(),
                cx,
            )
            .test_support()
            .role(gpui_kit::Role::Status)
            .aria_label(tr!("session-no-results"))
            .cursor_default()
            .text_color(cx.theme().muted_foreground)
            .child(
                Icon::new(IconName::MessageSquare)
                    .size(px(gds::appearance::tokens::SIDEBAR_ICON_SIZE)),
            )
        });
        SideBar::default()
            .state(SidebarState::Collapsed)
            .gap_1()
            .p(px(gds::appearance::tokens::SIDEBAR_PADDING))
            .child(SideBarHeader::default().gap_1().children(actions))
            .child(
                div()
                    .h(px(1.))
                    .w_full()
                    .flex_shrink_0()
                    .my_1()
                    .bg(cx.theme().border),
            )
            .child(
                SideBarContent::default()
                    .id("rail-sessions")
                    .gap_1()
                    .children(draft)
                    .children(
                        sessions
                            .into_iter()
                            .map(|session| self.rail_session(session, &cycle, cx)),
                    )
                    .children(empty),
            )
            .into_any_element()
    }

    fn rail_item(&self, id: SharedString, selected: bool, hint: String, cx: &App) -> Stateful<Div> {
        div()
            .id(id)
            .relative()
            .w_full()
            .h(px(gds::appearance::tokens::SIDEBAR_ITEM_HEIGHT))
            .flex_shrink_0()
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .tooltip_show_delay(gds::appearance::tokens::TOOLTIP_DELAY)
            .when(self.window.menu.is_none(), |row| {
                row.hoverable_tooltip(move |window, cx| {
                    crate::shared::ui::tooltip::view(hint.clone(), window, cx)
                })
            })
            .when(selected, |row| {
                row.bg(cx.theme().sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .when(!selected, |row| {
                row.hover(|style| style.bg(cx.theme().list_hover))
            })
    }

    fn rail_session(
        &self,
        session: &SessionMeta,
        cycle: &super::CycleTargets,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = session.id;
        let selected = self.selected.id == Some(id);
        let state = self.session_row_status(session);
        let hint = format!(
            "{}\n{} · {} · {}",
            title(session),
            session.agent,
            groups::name(session.cwd.as_deref()),
            if session.archived {
                tr!("session-archived")
            } else {
                state.label()
            }
        );
        let context_session = session.clone();
        let badge = if session.archived {
            Some(
                Icon::new(IconName::Archive)
                    .size(px(crate::shared::ui::SMALL_ICON))
                    .into_any_element(),
            )
        } else {
            state.badge(format!("rail-status-{id}"), cx)
        };
        let pinned = self.navigation.pinned.contains(&id).then(|| {
            div()
                .absolute()
                .top(px(BADGE_INSET))
                .right(px(BADGE_INSET))
                .child(Icon::new(IconName::Pin).size(px(crate::shared::ui::SMALL_ICON)))
        });
        self.rail_item(format!("rail-session-{id}").into(), selected, hint, cx)
            .test_support()
            .role(gpui_kit::Role::Button)
            .aria_label(title(session))
            .child(
                self.agent_icon(format!("rail-agent-{id}"), session.agent.as_str(), cx)
                    .size(px(gds::appearance::tokens::SIDEBAR_ICON_SIZE))
                    .text_color(if selected {
                        cx.theme().sidebar_accent_foreground
                    } else {
                        cx.theme().foreground
                    }),
            )
            .children(badge.map(|badge| {
                div()
                    .absolute()
                    .bottom(px(BADGE_INSET))
                    .right(px(BADGE_INSET))
                    .child(badge)
            }))
            .children(pinned)
            .children(cycle.hints(id, true))
            .on_click(cx.listener(move |this, _, window, cx| this.select(id, window, cx)))
            .on_mouse_down(
                gpui_kit::MouseButton::Right,
                cx.listener(move |this, event, window, cx| {
                    this.on_thread_row_context_menu(&context_session, event, window, cx);
                }),
            )
            .into_any_element()
    }

    fn rail_draft(&self, cx: &mut Context<Self>) -> AnyElement {
        let directory = self.navigation.selected_directory.clone();
        let disabled = self.selected.creating || self.composer.paste_pending;
        let state = if self.selected.creating {
            Status::Starting
        } else {
            Status::Idle
        };
        let hint = format!(
            "{}\n{} · {}",
            self.draft_title(),
            self.selected.agent,
            groups::name(directory.as_deref())
        );
        self.rail_item("rail-draft".into(), true, hint, cx)
            .test_support()
            .role(gpui_kit::Role::Button)
            .aria_label(self.draft_title())
            .child(
                self.agent_icon("rail-draft-agent", &self.selected.agent, cx)
                    .size(px(gds::appearance::tokens::SIDEBAR_ICON_SIZE))
                    .text_color(cx.theme().sidebar_accent_foreground),
            )
            .children(state.badge("rail-draft-status", cx).map(|badge| {
                div()
                    .absolute()
                    .bottom(px(BADGE_INSET))
                    .right(px(BADGE_INSET))
                    .child(badge)
            }))
            .on_click(cx.listener(|this, _, window, cx| this.focus_input(window, cx)))
            .on_mouse_down(
                gpui_kit::MouseButton::Right,
                cx.listener(move |this, event, window, cx| {
                    this.on_draft_row_context_menu(directory.clone(), disabled, event, window, cx);
                }),
            )
            .into_any_element()
    }
}
