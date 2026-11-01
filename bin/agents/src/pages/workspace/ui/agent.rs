use super::*;
use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::{Bounds, canvas, component::badge::Badge};
use std::{cell::Cell, rc::Rc};

mod options;

pub(super) const TOOLBAR_AGENT_ICON_SIZE: f32 = crate::shared::ui::SMALL_ICON;

impl Workspace {
    pub(super) fn new_thread_agent(&self, cx: &App) -> String {
        if let Some(agent) = &cx.global::<theme::Appearance>().default_agent {
            return agent.clone();
        }
        if !self.selected.agent.is_empty() {
            return self.selected.agent.clone();
        }
        self.sessions
            .entries
            .iter()
            .max_by_key(|session| session.updated_at)
            .map(|session| session.agent.to_string())
            .or_else(|| self.backend.agents.first().map(|agent| agent.id.clone()))
            .unwrap_or_default()
    }

    pub(super) fn agent_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.activity_count();
        let agents: Vec<_> = self
            .backend
            .agents
            .iter()
            .map(|agent| agent.id.clone())
            .collect();
        let selected = self
            .current()
            .map(|session| session.agent.to_string())
            .unwrap_or_else(|| self.selected.agent.clone());
        let disabled = !self.can_select_agent();
        let hint = if selected.is_empty() {
            tr!("agent-choose").to_owned()
        } else {
            tr!("agent-current", agent = selected.as_str())
        };
        let view = cx.entity().downgrade();
        let metadata = self.backend.metadata.clone();
        let server = self.backend.server.clone();
        let parent = self.window.handle;
        let icon = el! {
            $(self.agent_icon("agent-picker-icon", &selected, cx)) {
                size: TOOLBAR_AGENT_ICON_SIZE.px;
                text_color: $(if disabled {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().foreground
                });
            }
        };
        let button = el! {
            ("agent-picker").Button::new {
                ghost;
                xsmall;
                h: layout::ICON_BUTTON_SIZE.px;
                p_0;
                when: $(!self.selected.waiting_for_agent_selection), {
                    w: layout::ICON_BUTTON_SIZE.px;
                };
                flex_shrink_0;
                disabled: disabled;
                accessibility_label: $(if selected.is_empty() {
                        hint.to_owned()
                    } else {
                        tr!("agent-current", agent = selected.as_str())
                });
                when: $(self.selected.waiting_for_agent_selection), {
                    label: tr!("agent-choose");
                };
            } [
                icon,
            ]
        };
        let menu = if disabled {
            button.into_any_element()
        } else {
            self.menu_button(
                button,
                agents.len() + 2,
                false,
                move |menu, _, _| {
                    let mut menu = menu.check_side(Side::Right);
                    for id in &agents {
                        let id = id.clone();
                        let label = id.clone();
                        let metadata = metadata.clone();
                        let view = view.clone();
                        let server = server.clone();
                        menu = menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                let artwork = metadata
                                    .as_ref()
                                    .and_then(|metadata| metadata.read(cx).icon(&server, &label))
                                    .unwrap_or_default();
                                el! {
                                    .div {
                                        # format!("agent-menu-label-{label}");
                                        test_support;
                                        role: gpui_kit::Role::Label;
                                        aria_label: $(label.clone());
                                        flex;
                                        flex_1;
                                        min_w_0;
                                        items_center;
                                        gap_2;
                                    } [
                                        (format!("agent-menu-icon-{label}"), artwork).crate::entities::agent::icon::AgentIcon::new {
                                            size: AGENT_ICON_SIZE.px;
                                        },
                                        .div {
                                            flex_1;
                                            truncate;
                                        } [
                                            $(label.clone()),
                                        ],
                                    ]
                                }
                            })
                            .checked(id == selected)
                            .on_click(move |_, window, cx| {
                                menus::in_parent(parent, window, cx, |window, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        this.select_agent(id.clone(), window, cx);
                                    });
                                });
                            }),
                        );
                    }
                    let view = view.clone();
                    menu.separator()
                    .item(
                        PopupMenuItem::element(|_, _| {
                            el! {
                                .div {
                                    # "agent-menu-install-label";
                                    test_support;
                                    role: gpui_kit::Role::Label;
                                    aria_label: tr!("agent-install-action");
                                    flex;
                                    flex_1;
                                    items_center;
                                    gap_2;
                                } [
                                    (IconName::Plus).Icon::new {
                                        size: AGENT_ICON_SIZE.px;
                                    },
                                    tr!("agent-install-action"),
                                ]
                            }
                        })
                        .on_click(move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ =
                                    view.update(cx, |this, cx| this.open_agent_catalog(window, cx));
                            });
                        }),
                    )
                },
                cx,
            )
        };
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let measured = bounds.clone();
        let activity_open = self
            .window
            .reply_preview
            .as_ref()
            .is_some_and(|preview| preview.key == reply::PreviewKey::Activity);
        el! {
            .div {
                # "agent-picker-hint";
                test_support;
                relative;
                flex;
                flex_shrink_0;
                on_hover: $(cx.listener(move |this, hovered: &bool, window, cx| {
                            this.hover_activity(*hovered, bounds.get(), window, cx);
                }));
                on_mouse_down: gpui_kit::MouseButton::Left, $(cx.listener(|this, _, window, cx| {
                            this.close_reply_preview(window, cx);
                }));
                when: $(running > 0), {
                    pr_3;
                    role: gpui_kit::Role::Status;
                    aria_label: tr!("agent-activity-count", count = running);
                };
                when: $(running == 0 && !activity_open), {
                    hoverable_tooltip: $(move |window, cx| crate::shared::ui::tooltip::view(hint.clone(), window, cx));
                };
            } [
                .Badge::new {
                    count: running;
                    small;
                    max: 99;
                    color: $(cx.theme().primary);
                } [
                    menu,
                ],
                ($(move |bounds, _, _| measured.set(bounds)), $(|_, _, _, _| {})).canvas {
                    absolute;
                    size_full;
                },
            ]
        }
    }
}

impl Workspace {
    fn configure(
        &mut self,
        request: bas::acp::SetSessionConfigOptionRequest,
        cx: &mut Context<Self>,
    ) {
        let session = request
            .session_id
            .to_string()
            .parse()
            .expect("GUI uses daemon session ids");
        if self.sessions.healthy(session) && self.sessions.pending.configuring.insert(session) {
            self.command(Command::SetConfig(request), cx);
            cx.notify();
        }
    }

    pub(super) fn render_config_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use bas::acp;
        let mut row = el! {
            .div {
                # "session-options";
                flex;
                items_center;
                w_full;
                min_w_0;
                gap_1;
                overflow_x_scroll;
            } [
                .div {
                    flex_1;
                },
            ]
        };
        if self.selected.creating {
            return el! {
                row [
                    ("options-loading").Button::new {
                        ghost;
                        xsmall;
                        cursor_default;
                        icon: IconName::Loader;
                        tooltip: tr!("agent-initializing");
                    },
                ]
            };
        }
        let Some(session) = self.current() else {
            return row;
        };
        if !self.sessions.healthy(session.id) {
            return row;
        }
        let Some(options) = session
            .acp
            .as_ref()
            .and_then(|info| info.config_options.as_ref())
        else {
            return row;
        };
        for option in options {
            let session = session.id;
            let config_id = option.id.clone();
            let view = cx.entity().downgrade();
            let id = SharedString::from(format!("config-{}", option.id));
            match &option.kind {
                acp::SessionConfigKind::Select(select) => {
                    let choices: Vec<_> = match &select.options {
                        acp::SessionConfigSelectOptions::Ungrouped(options) => options.clone(),
                        acp::SessionConfigSelectOptions::Grouped(groups) => groups
                            .iter()
                            .flat_map(|group| group.options.clone())
                            .collect(),
                        _ => continue,
                    };
                    let current = select.current_value.clone();
                    let name = choices
                        .iter()
                        .find(|value| value.value == current)
                        .map(|value| value.name.clone())
                        .unwrap_or_else(|| current.to_string());
                    let hint = tr!("option-choose", name = option.name.as_str());
                    let hint = options::description(option, &hint);
                    let button = match options::icon(option) {
                        Some(icon) => Button::new(id).control_icon(icon),
                        None => Button::new(id),
                    };
                    // A first-party label keeps text size independent of button geometry.
                    let control = crate::shared::ui::tooltip::bounded(
                        SharedString::from(format!("config-hint-{}", option.id)),
                        self.menu_button(
                            el! {
                                button {
                                    flex_shrink_0;
                                    disabled: $(self.sessions.pending.configuring.contains(&session));
                                    accessibility_label: $(name.clone());
                                    ghost;
                                    small;
                                    h: $(px(layout::ICON_BUTTON_SIZE));
                                    px_2;
                                    gap_1;
                                    max_w: 160.0.px;
                                    dropdown_caret: true;
                                } [
                                    .div {
                                        # $(SharedString::from(format!("config-label-{}", option.id)));
                                        test_support;
                                        min_w_0;
                                        whitespace_nowrap;
                                        text_ellipsis;
                                        line_height: $(gpui_kit::relative(1.));
                                        text_size: crate::shared::ui::SMALL;
                                    } [
                                        name,
                                    ],
                                ]
                            },
                            choices.len(),
                            self.sessions.pending.configuring.contains(&session),
                            move |mut menu, _, _| {
                                for choice in &choices {
                                    let view = view.clone();
                                    let request = acp::SetSessionConfigOptionRequest::new(
                                        session.to_string(),
                                        config_id.clone(),
                                        choice.value.clone(),
                                    );
                                    menu = menu.item(
                                        PopupMenuItem::new(choice.name.clone())
                                            .checked(choice.value == current)
                                            .on_click(move |_, _, cx| {
                                                let _ = view.update(cx, |this, cx| {
                                                    this.configure(request.clone(), cx);
                                                });
                                            }),
                                    );
                                }
                                menu
                            },
                            cx,
                        ),
                        hint,
                    );
                    row = el! {
                        row [
                            control,
                        ]
                    };
                }
                acp::SessionConfigKind::Boolean(boolean) => {
                    let hint = options::description(option, &config_option_hint(option));
                    if let Some(icon) = options::toggle_icon(option) {
                        let checked = boolean.current_value;
                        let control = crate::shared::ui::icon_button(id, icon)
                            .selected(checked)
                            .toggled(checked)
                            .accessibility_label(option.name.clone())
                            .disabled(self.sessions.pending.configuring.contains(&session))
                            .tooltip(hint)
                            .on_click(move |_, window, cx| {
                                let request = acp::SetSessionConfigOptionRequest::new(
                                    session.to_string(),
                                    config_id.clone(),
                                    !checked,
                                );
                                let _ = view.update(cx, |this, cx| {
                                    this.on_agent_boolean_config(request, window, cx);
                                });
                            });
                        row = row.child(control);
                        continue;
                    }
                    let label = el! {
                        .div {
                            # $(SharedString::from(format!("config-label-{}", option.id)));
                            test_support;
                            text_size: crate::shared::ui::SMALL;
                            text_color: $(if self.sessions.pending.configuring.contains(&session) {
                                    cx.theme().muted_foreground
                                } else {
                                    cx.theme().foreground
                            });
                        } [
                            $(option.name.clone()),
                        ]
                    };
                    let switch = el! {
                        (id).gpui_kit::component::switch::Switch::new {
                            flex_shrink_0;
                            checked: $(boolean.current_value);
                            disabled: $(self.sessions.pending.configuring.contains(&session));
                            when: $(!self.sessions.pending.configuring.contains(&session)), {
                                cursor_pointer;
                            };
                            xsmall;
                            accessibility_label: $(option.name.clone());
                            on_change: $(move |checked, window, cx| {
                                    let request = acp::SetSessionConfigOptionRequest::new(
                                        session.to_string(), config_id.clone(), *checked,
                                    );
                                    let _ = view.update(cx, |this, cx| {
                                            this.on_agent_boolean_config(request, window, cx);
                                    });
                            });
                        }
                    };
                    row = el! {
                        row [
                            .div {
                                # $(SharedString::from(format!("config-field-{}", option.id)));
                                test_support;
                                flex_shrink_0;
                                flex;
                                items_center;
                                gap_2;
                                px_1;
                                hoverable_tooltip: $(move |window, cx| {
                                        crate::shared::ui::tooltip::view(hint.clone(), window, cx)
                                });
                            } [
                                label,
                                switch,
                            ],
                        ]
                    };
                }
                _ => {}
            }
        }
        row
    }

    fn on_agent_boolean_config(
        &mut self,
        request: bas::acp::SetSessionConfigOptionRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.configure(request, cx);
        self.focus_input(window, cx);
    }
}

pub(super) fn config_option_hint(option: &bas::acp::SessionConfigOption) -> String {
    match &option.kind {
        bas::acp::SessionConfigKind::Boolean(boolean) if boolean.current_value => {
            tr!("option-disable", name = option.name.as_str())
        }
        bas::acp::SessionConfigKind::Boolean(_) => {
            tr!("option-enable", name = option.name.as_str())
        }
        _ => tr!("option-choose", name = option.name.as_str()),
    }
}
