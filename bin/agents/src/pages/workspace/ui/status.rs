use super::*;
use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::component::spinner::Spinner;

const ACTIVITY_ICON_SIZE: f32 = crate::shared::ui::ICON;
const CONNECTION_LIGHT_SIZE: f32 = 6.;
const CONNECTION_BADGE_INSET: f32 = 2.;

/// Share the same motion and sizing across running activity indicators.
pub(super) fn activity_icon(
    id: impl Into<gpui_kit::ElementId>,
    icon: IconName,
    color: gpui_kit::Hsla,
    running: bool,
) -> AnyElement {
    let artwork = if running {
        el! {
            .Spinner::new {
                icon: IconName::LoaderCircle;
                with_size: ACTIVITY_ICON_SIZE.px;
                color: color;
            }
        }
        .into_any_element()
    } else {
        el! {
            (icon).Icon::new {
                with_size: ACTIVITY_ICON_SIZE.px;
                text_color: color;
            }
        }
        .into_any_element()
    };
    el! {
        .div {
            # id;
            test_support;
            size: ACTIVITY_ICON_SIZE.px;
            flex_shrink_0;
            flex;
            items_center;
            justify_center;
        } [
            artwork,
        ]
    }
    .into_any_element()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
}

impl Workspace {
    pub(super) fn awaiting_connection(&self) -> bool {
        !self.backend.connected && self.selected.id.is_none()
    }

    fn server_status_label(&self) -> &'static str {
        if self.backend.error.is_some() {
            tr!("status-reconnecting")
        } else if self.backend.connected {
            tr!("status-connected")
        } else {
            tr!("status-connecting")
        }
    }

    pub(super) fn render_connection_transition(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::features::navigation::{self as routing, presentation};

        let profiles = routing::shared(cx);
        let target = &self.backend.server.target;
        let endpoint = presentation::endpoint(target);
        let name = profiles
            .read(cx)
            .matching(&self.backend.server)
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| endpoint.clone());
        let label = self.server_status_label();
        el! {
            ().chat::content_panel {
                # "server-connection-transition";
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
                .div {
                    flex;
                    items_center;
                    gap_2;
                } [
                    $(activity_icon("server-connection-loading", IconName::LoaderCircle, cx.theme().warning, true)),
                    label,
                ],
                .div {
                    # "server-connection-target";
                    test_support;
                    role: gpui_kit::Role::Label;
                    aria_label: $(name.clone());
                    max_w_full;
                    truncate;
                } [
                    $(name.clone()),
                ],
                ..$((name != endpoint).then(|| el! {.div {
                                max_w_full;
                                truncate;
                            } [
                                endpoint,
                ]})),
                ..$(self.backend.error.as_ref().map(|error| crate::shared::ui::feedback(
                            "server-connection-error", error.presentation(), cx.theme().danger
                        ).compact())),
            ]
        }.into_any_element()
    }

    pub(super) fn connection_status(&self) -> ConnectionStatus {
        if self.backend.error.is_some() {
            return ConnectionStatus::Disconnected;
        }
        if !self.backend.connected {
            return ConnectionStatus::Connecting;
        }
        if self.selected.id.is_none() {
            return ConnectionStatus::Connected;
        }
        if self
            .selected
            .id
            .is_some_and(|id| !self.sessions.healthy(id))
        {
            return ConnectionStatus::Disconnected;
        }
        match self.selected.stream {
            StreamState::Loading | StreamState::Reconnecting(_) => ConnectionStatus::Connecting,
            StreamState::Live => ConnectionStatus::Connected,
            StreamState::Unavailable(_) | StreamState::Closed => ConnectionStatus::Disconnected,
        }
    }

    pub(super) fn session_indicator(&self, id: &'static str, cx: &App) -> AnyElement {
        use crate::entities::session::status::Status;
        let state = if self.selected.creating {
            Status::Starting
        } else if let Some(meta) = self.current() {
            if !self.sessions.healthy(meta.id) {
                Status::Failed
            } else {
                Status::for_session(
                    meta,
                    self.sessions
                        .seen_replies
                        .get(&meta.id)
                        .copied()
                        .unwrap_or(0),
                    self.has_unread_reply(),
                )
            }
        } else {
            Status::Idle
        };
        let indicator = if state == Status::Unread {
            "unread-reply".to_owned()
        } else {
            format!("{id}-indicator")
        };
        state.render(id, indicator, cx)
    }

    pub(super) fn server_status_footer(
        &self,
        sidebar: gds::ui::sidebar::SidebarPresentation,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::features::navigation::{self as routing, presentation};
        use gpui_kit::component::menu::PopupMenuItem;

        let label = self.server_status_label();
        let pending = !self.backend.connected || self.backend.error.is_some();
        let color = if self.backend.error.is_some() {
            cx.theme().danger
        } else if self.backend.connected {
            cx.theme().success
        } else {
            cx.theme().warning
        };
        let profiles = routing::shared(cx);
        let saved = profiles.read(cx).matching(&self.backend.server);
        let selected = saved.map(|profile| profile.id.clone());
        let target = &self.backend.server.target;
        let name = saved
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| presentation::endpoint(target));
        let hint = format!("{name} · {} · {label}", presentation::tooltip(target));
        let entries = profiles.read(cx).entries().to_vec();
        let owner = cx.entity().downgrade();
        let workspace_window = self.window.handle;
        let loading_size = ACTIVITY_ICON_SIZE;
        let indicator = el! {
            .div {
                # "server-status-indicator";
                test_support;
                role: gpui_kit::Role::Status;
                aria_label: label;
                size: CONNECTION_LIGHT_SIZE.px;
                rounded_full;
                flex_shrink_0;
                bg: color;
                when: pending, {
                    size: loading_size.px;
                    bg: $(gpui_kit::transparent_black());
                };
            } [
                @if (pending)
                    .div {
                        # "server-status-loading";
                        test_support;
                        size: loading_size.px;
                    } [
                        .Spinner::new {
                            icon: IconName::LoaderCircle;
                            with_size: loading_size.px;
                            color: color;
                        },
                    ],
                @end,
            ]
        };
        let icon = el! {
            $(presentation::icon(target)) {
                size: crate::shared::ui::ICON.px;
                flex_shrink_0;
            }
        };
        let inset = sidebar.interpolate(
            12.,
            (gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH
                - gds::appearance::tokens::ICON_BUTTON_SIZE)
                / 2.,
        );
        let button = el! {
            ("server-status").Button::new {
                small;
                ghost;
                w: $(px(sidebar.width() - inset - 12.));
                min_w_0;
                accessibility_label: $(hint.clone());
                tooltip: hint;
                overflow_hidden;
                justify_start;
                px_0;
            } [
                .div {
                    relative;
                    flex_shrink_0;
                    flex;
                    items_center;
                    justify_center;
                    size: gds::appearance::tokens::ICON_BUTTON_SIZE.px;
                } [
                    icon,
                    .div {
                        absolute;
                        bottom: CONNECTION_BADGE_INSET.px;
                        right: CONNECTION_BADGE_INSET.px;
                    } [indicator],
                ],
                .div {
                    # "server-status-name";
                    test_support;
                    role: gpui_kit::Role::Label;
                    aria_label: $(name.clone());
                    flex_1;
                    min_w_0;
                    truncate;
                    text_left;
                    opacity: $(sidebar.label_opacity());
                } [name],
                (IconName::ChevronDown).Icon::new {
                    size: crate::shared::ui::SMALL_ICON.px;
                    flex_shrink_0;
                    when: $(!sidebar.actions_visible()), { invisible; };
                },
            ]
        };
        let menu = self.menu_button(
            button,
            entries.len() * 2 + 2,
            false,
            move |menu, _, _| {
                let mut menu = menu.check_side(gpui_kit::component::Side::Right);
                for profile in &entries {
                    let id = profile.id.clone();
                    let owner = owner.clone();
                    menu = menu.item(
                        presentation::menu_item(profile)
                            .checked(selected.as_ref() == Some(&profile.id))
                            .on_click(move |_, _, cx| {
                                let id = id.clone();
                                let owner = owner.clone();
                                cx.defer(move |cx| {
                                    let result = routing::choose(&id, workspace_window, cx);
                                    let _ = owner.update(cx, |this, cx| {
                                        if let Err(error) = result {
                                            this.selected.error = Some(error);
                                        }
                                        cx.notify();
                                    });
                                });
                            }),
                    );
                }
                let owner = owner.clone();
                menu.separator().item(
                    PopupMenuItem::new(tr!("connections-manage"))
                        .icon(IconName::Settings)
                        .on_click(move |_, _, cx| {
                            let owner = owner.clone();
                            cx.defer(move |cx| {
                                if let Err(error) =
                                    crate::widgets::settings::window::open_connections(cx)
                                {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.selected.error = Some(error.to_string());
                                        cx.notify();
                                    });
                                }
                            });
                        }),
                )
            },
            cx,
        );
        el! {
            .div {
                h: gds::appearance::tokens::FOOTER_HEIGHT.px;
                w_full;
                flex_shrink_0;
                flex;
                items_center;
                pl: inset.px;
                pr_3;
                min_w_0;
                overflow_hidden;
            } [
                menu,
            ]
        }
        .into_any_element()
    }

    pub(super) fn activity_label(&self) -> Option<String> {
        let active = self.composer.paste_pending
            || self.selected.creating
            || self.working()
            || !self.selected.approval.panel.is_empty()
            || !self.selected.elicitations.is_empty()
            || self.selected.id.is_some_and(|id| {
                self.sending(id) || self.sessions.pending.configuring.contains(&id)
            });
        (active && self.connection_status() == ConnectionStatus::Connected)
            .then(|| self.stream_label())
    }
}
