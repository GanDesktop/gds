use super::*;
use crate::shared::i18n::tr;
use bas::model::Timestamp;
use bas::service::HistoryOverviewEntry;
use gds_uim::el;
use gds_uim::m;

const TICK_WIDTH: f32 = 14.;
const HOVER_TICK_WIDTH: f32 = 20.;
const ACTIVE_TICK_HEIGHT: f32 = 4.;
const MARKER_HEIGHT: f32 = 12.;
const RAIL_CHROME_HEIGHT: f32 = 24.;

fn marker_tick(
    id: SharedString,
    group: SharedString,
    active: bool,
    bookmarked: bool,
    cx: &App,
) -> AnyElement {
    el! {
        .div {
            size_full;
            flex;
            items_center;
            justify_center;
        } [
            .div {
                # id;
                test_support;
                w: TICK_WIDTH.px;
                h: $(if active { ACTIVE_TICK_HEIGHT } else { 2. }).px;
                rounded_full;
                bg: $(if active || bookmarked { cx.theme().primary } else { cx.theme().muted_foreground });
                group_hover: group, m! {w: HOVER_TICK_WIDTH.px;
                        h: $(if active { ACTIVE_TICK_HEIGHT } else { 3. }).px;
                        bg: $(cx.theme().primary);};
            },
        ]
    }.into_any_element()
}

fn marker_time(entry: &HistoryOverviewEntry) -> Timestamp {
    Timestamp::from_millis(u64::try_from(entry.timestamp_millis).unwrap_or_default())
}

fn marker_hint(entry: &HistoryOverviewEntry, bookmarked: bool, cx: &App) -> String {
    format!(
        "{}\n{}\nCtrl + {}: {}\nAlt + {}: {}",
        entry.preview,
        crate::shared::ui::time::recent_time(marker_time(entry), Timestamp::now(), cx),
        tr!("shortcut-click"),
        if bookmarked {
            tr!("turn-marker-remove-bookmark")
        } else {
            tr!("turn-marker-add-bookmark")
        },
        tr!("shortcut-scroll"),
        tr!("turn-marker-wheel-hint"),
    )
}

impl Workspace {
    pub(in crate::pages::workspace) fn render_turn_navigation(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.selected.id else {
            return el! {
                .div
            }
            .into_any_element();
        };
        let navigation = &self.selected.timeline.navigation;
        let active = self.active_marker();
        let at_end = !navigation.detached
            && active.is_none()
            && (self.selected.timeline.list.is_following_tail()
                || self
                    .selected
                    .timeline
                    .list
                    .is_scrolled_to_end()
                    .unwrap_or(false));
        let mut markers = el! {
            .div {
                # $(SharedString::from(format!("turn-prompts-{session}")));
                test_support;
                w_full;
                min_h_0;
                flex;
                flex_col;
                flex_shrink_1;
                overflow_y_scroll;
            }
        };
        let entries = navigation
            .overview
            .as_ref()
            .map(|overview| overview.entries.as_slice())
            .unwrap_or_default();
        let available = (f32::from(self.selected.timeline.list.viewport_bounds().size.height)
            - RAIL_CHROME_HEIGHT)
            .max(MARKER_HEIGHT);
        let marker_height = (available / entries.len().max(1) as f32).clamp(2., MARKER_HEIGHT);
        for entry in entries {
            let cursor = entry.cursor;
            let bookmarked = self
                .navigation
                .marker_bookmarks
                .get(&session)
                .is_some_and(|bookmarks| bookmarks.contains(&cursor));
            let marker_id = SharedString::from(format!("turn-marker-{cursor}"));
            let group = marker_id.clone();
            let hint: SharedString = marker_hint(entry, bookmarked, cx).into();
            let preview = crate::shared::ui::tooltip::wrap_tokens(&entry.preview);
            let at = marker_time(entry);
            let control_hint = if bookmarked {
                tr!("turn-marker-remove-bookmark")
            } else {
                tr!("turn-marker-add-bookmark")
            };
            let label = hint.clone();
            let tick = marker_tick(
                format!("turn-marker-line-{cursor}").into(),
                group.clone(),
                active == Some(cursor),
                bookmarked,
                cx,
            );
            let marker = el! {
                .div {
                    # marker_id;
                    test_support;
                    role: gpui_kit::Role::Button;
                    aria_label: hint;
                    group: group;
                    h: $(px(marker_height));
                    min_h: 2.0.px;
                    flex_shrink_0;
                    w_full;
                    cursor_pointer;
                    hoverable_tooltip: $(move |window, cx| {
                            let preview = preview.clone();
                            let at = crate::shared::ui::time::recent_time(at, Timestamp::now(), cx);
                            let previous = shortcuts::hint(tr!("turn-marker-previous"),
                                &shortcuts::PreviousConversationTurn, window);
                            let next = shortcuts::hint(tr!("turn-marker-next"),
                                &shortcuts::NextConversationTurn, window);
                            let label = format!("{label}\n{previous}\n{next}");
                            crate::shared::ui::tooltip::element("turn-marker-tooltip", label, move |_, cx| {
                                    el! {.div {
                                            flex;
                                            flex_col;
                                            gap_2;
                                        } [
                                            .div {
                                                # "turn-marker-preview";
                                                test_support;
                                                whitespace_normal;
                                                text_size: crate::shared::ui::SMALL;
                                                pb_2;
                                                border_b_1;
                                                border_color: $(cx.theme().border);
                                            } [
                                                $(preview.clone()),
                                            ],
                                            .div {
                                                # "turn-marker-metadata";
                                                test_support;
                                                flex;
                                                items_center;
                                                justify_between;
                                                gap_3;
                                                text_xs;
                                                text_color: $(cx.theme().muted_foreground);
                                            } [
                                                .div {
                                                    # "turn-marker-time";
                                                    test_support;
                                                    role: gpui_kit::Role::Label;
                                                    aria_label: $(at.clone());
                                                } [
                                                    $(at.clone()),
                                                ],
                                                ..$(bookmarked.then(|| el! {.div {
                                                                # "turn-marker-bookmark-label";
                                                                test_support;
                                                                role: gpui_kit::Role::Label;
                                                                aria_label: tr!("turn-marker-bookmarked");
                                                                flex;
                                                                items_center;
                                                                gap_1;
                                                                flex_shrink_0;
                                                                rounded_sm;
                                                                px_1;
                                                                whitespace_nowrap;
                                                                bg: $(cx.theme().accent);
                                                                text_color: $(cx.theme().accent_foreground);
                                                            } [
                                                                .div {
                                                                    # "turn-marker-bookmark-icon";
                                                                    test_support;
                                                                    flex;
                                                                    items_center;
                                                                    flex_shrink_0;
                                                                } [
                                                                    (IconName::Bookmark).Icon::new {
                                                                        xsmall;
                                                                    },
                                                                ],
                                                                tr!("turn-marker-bookmarked"),
                                                ]})),
                                            ],
                                            .div {
                                                # "turn-marker-shortcuts";
                                                test_support;
                                                flex;
                                                flex_col;
                                                gap_1;
                                                pt_2;
                                                border_t_1;
                                                border_color: $(cx.theme().border);
                                                text_xs;
                                                text_color: $(cx.theme().muted_foreground);
                                            } [
                                                $(gds::ui::ShortcutHint::gesture(control_hint,
                                                    ["Ctrl".into(), tr!("shortcut-click").into()])),
                                                .div {
                                                    # "turn-marker-previous-hint";
                                                    test_support;
                                                    role: gpui_kit::Role::Label;
                                                    aria_label: $(previous.clone());
                                                } [
                                                    $(gds::ui::ShortcutHint::new(tr!("turn-marker-previous"),
                                                        gds::ui::Shortcut::new(Box::new(shortcuts::PreviousConversationTurn), Some("BasWorkspace")))),
                                                ],
                                                .div {
                                                    # "turn-marker-next-hint";
                                                    test_support;
                                                    role: gpui_kit::Role::Label;
                                                    aria_label: $(next.clone());
                                                } [
                                                    $(gds::ui::ShortcutHint::new(tr!("turn-marker-next"),
                                                        gds::ui::Shortcut::new(Box::new(shortcuts::NextConversationTurn), Some("BasWorkspace")))),
                                                ],
                                                $(gds::ui::ShortcutHint::gesture(tr!("turn-marker-wheel-hint"),
                                                    ["Alt".into(), tr!("shortcut-scroll").into()])),
                                            ],
                                    ]}.into_any_element()
                                }, window, cx)
                    });
                    on_click: $(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                                if this.selected.id != Some(session) { return; }
                                if event.modifiers().control { this.toggle_marker_bookmark(session, cursor, cx); }
                                else { this.navigate_to_marker(cursor, cx); }
                    }));
                } [
                    tick,
                ]
            };
            markers = el! {
                markers [
                    marker,
                ]
            };
        }
        let group: SharedString = "turn-marker-end".into();
        let end = el! {
            .div {
                # $(group.clone());
                test_support;
                relative;
                role: gpui_kit::Role::Button;
                aria_label: tr!("conversation-end");
                group: $(group.clone());
                hoverable_tooltip: $(shortcuts::tooltip(tr!("conversation-end"), ScrollConversationToEnd));
                h: MARKER_HEIGHT.px;
                w_full;
                flex_shrink_0;
                cursor_pointer;
                on_click: $(cx.listener(|this, _, _, cx| this.scroll_conversation_to_end(cx)));
            } [
                ("turn-marker-end", $(shortcuts::shortcut(ScrollConversationToEnd))).gds::ui::keytips::Anchor::new,
                $(marker_tick("turn-marker-line-end".into(), group, at_end, false, cx)),
            ]
        };
        el! {
            .div {
                # "turn-navigation";
                test_support;
                absolute;
                top_0;
                bottom_0;
                right: $(px(-RAIL_WIDTH));
                flex;
                flex_col;
                justify_center;
                py_1;
                w: RAIL_WIDTH.px;
                flex_shrink_0;
            } [
                .div {
                    # "turn-shortcuts";
                    test_support;
                    flex;
                    flex_col;
                    w_full;
                    min_h_0;
                    max_h_full;
                    rounded_md;
                    hover: {
                        bg: $(cx.theme().list_hover);
                    };
                    on_scroll_wheel: $(cx.listener(|this, event, window, cx| {
                                this.on_navigation_wheel(event, window, cx);
                                cx.stop_propagation();
                    }));
                } [
                    ..$(navigation.overview_request.is_some().then(|| status::activity_icon(
                                "turn-overview-loading", IconName::LoaderCircle, cx.theme().muted_foreground, true))),
                    ..$(navigation.overview_error.as_ref().map(|error| el! {("turn-overview-error", el! {.div {
                                                text_color: $(cx.theme().danger);
                                            } [
                                                "!",
                                    ]}, $(error.presentation())).crate::shared::ui::tooltip::bounded {
                                    test_support;
                                    cursor_pointer;
                                    on_click: $(cx.listener(|this, _, _, cx| this.request_history_overview(0, cx)));
                    }})),
                    markers,
                    end,
                ],
            ]
        }.into_any_element()
    }

    pub(in crate::pages::workspace) fn render_history_location_status(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let navigation = &self.selected.timeline.navigation;
        let loading = navigation.location.is_some();
        let error = navigation.location_error.as_ref();
        if !loading && error.is_none() && !navigation.detached {
            return None;
        }
        Some(el! {
            .div {
                # "history-location-status";
                test_support;
                absolute;
                bottom_0;
                left_0;
                right_0;
                flex;
                justify_center;
                px_3;
            } [
                .div {
                    flex;
                    items_center;
                    gap_2;
                    px_3;
                    py_1;
                    max_w_full;
                    rounded_md;
                    bg: $(cx.theme().popover);
                    text_size: crate::shared::ui::SMALL;
                } [
                    ..$(loading.then(|| status::activity_icon("history-location-loading",
                                IconName::LoaderCircle, cx.theme().muted_foreground, true))),
                    ..$(error.map(|error| crate::shared::ui::feedback("history-location-error", error.presentation(), cx.theme().danger).compact())),
                    ..$((!loading && navigation.after.is_some()).then(|| el! {("history-location-newer").crate::shared::ui::action_button {
                                    label: tr!("turn-marker-load-newer");
                                    on_click: $(cx.listener(|this, _, _, cx| this.load_next_history(cx)));
                    }})),
                    ..$((!loading).then(|| el! {("history-location-latest").crate::shared::ui::action_button {
                                    label: tr!("conversation-end");
                                    on_click: $(cx.listener(|this, _, _, cx| this.scroll_conversation_to_end(cx)));
                    }})),
                ],
            ]
        }.into_any_element())
    }
}
