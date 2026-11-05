use super::View;
use crate::pages::workspace::ui::*;
use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::ClickEvent;
use gpui_kit::component::menu::PopupMenuItem;

#[derive(Clone, Copy)]
enum SidebarCommand {
    Search,
    NewSession,
    AddWorkspace,
}

impl Workspace {
    pub(super) fn sidebar_controls(
        &self,
        presentation: gds::ui::sidebar::SidebarPresentation,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content = if self.navigation.search_expanded {
            let search_icon = Icon::new(IconName::Search).small();
            el! {
                .div {
                    # "thread-search";
                    test_support;
                    key_context: "BasSidebarSearch";
                    flex_1;
                    min_w_0;
                } [
                    $(crate::shared::ui::input::placeholder(&self.navigation.search, gds::ui::field::input(&self.navigation.search, cx).small().prefix(search_icon).cleanable(true))),
                ]
            }.into_any_element()
        } else {
            let actions = self.sidebar_actions(cx);
            el! {
                .div { flex; flex_1; min_w_0; items_center } [
                    $(self.navigation.view.label()).gds::ui::sidebar::SideBarSectionTitle::new {
                        # "sidebar-view-title";
                        flex_1;
                        min_w_0;
                        truncate;
                        opacity: $(presentation.label_opacity());
                    },
                    .div {
                        flex; flex_shrink_0;
                        when: $(!presentation.actions_visible()), { invisible; };
                    } [..actions],
                ]
            }
            .into_any_element()
        };
        el! {
            .div {
                # "sidebar-header";
                test_support;
                flex;
                flex_shrink_0;
                items_center;
                h: layout::HEADER_HEIGHT.px;
                pl: $(px(gds::appearance::tokens::SIDEBAR_PADDING + presentation.interpolate(0., (gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH - 2. * gds::appearance::tokens::SIDEBAR_PADDING - layout::ICON_BUTTON_SIZE) / 2.)));
                pr: gds::appearance::tokens::SIDEBAR_PADDING.px;
                overflow_hidden;
            } [
                .div { flex_shrink_0; } [$(self.sidebar_view_button(cx))],
                content,
            ]
        }
        .into_any_element()
    }

    fn sidebar_actions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![
            el! {
                    ("sidebar-search", IconName::Search).layout::icon_button {
                        accessibility_label: tr!("session-search");
                        tooltip_with_shortcut: tr!("session-search"), $(shortcuts::shortcut(FocusSearch));
                        on_click: $(cx.listener(Self::on_sidebar_search));
                    }
            }.into_any_element(),
            el! {
                    ("new-session", IconName::Plus).layout::icon_button {
                        accessibility_label: tr!("session-new");
                        tooltip_with_shortcut: tr!("session-new"), $(shortcuts::shortcut(NewSession));
                        on_click: $(cx.listener(Self::on_sidebar_new_session));
                    }
            }.into_any_element(),
            el! {
                    ("add-workspace-button", IconName::FolderPlus).layout::icon_button {
                        accessibility_label: tr!("workspace-add");
                        tooltip_with_shortcut: tr!("workspace-add"), $(shortcuts::shortcut(shortcuts::AddWorkspace));
                        on_click: $(cx.listener(Self::on_sidebar_add_workspace));
                    }
            }.into_any_element(),
        ]
    }

    fn sidebar_view_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        let selected = self.navigation.view;
        self.menu_button(
            layout::icon_button("sidebar-view-menu", selected.icon())
                .accessibility_label(tr!("sidebar-view"))
                .tooltip(selected.label()),
            View::ALL.len() + 3,
            false,
            move |mut menu, _, _| {
                for view in View::ALL {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(view.label())
                            .icon(view.icon())
                            .checked(selected == view)
                            .on_click(move |_, window, cx| {
                                menus::in_parent(parent, window, cx, |_, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.set_sidebar_view(view, cx);
                                    });
                                });
                            }),
                    );
                }
                for (label, icon, action) in [
                    (
                        tr!("session-search"),
                        IconName::Search,
                        SidebarCommand::Search,
                    ),
                    (
                        tr!("session-new"),
                        IconName::Plus,
                        SidebarCommand::NewSession,
                    ),
                    (
                        tr!("workspace-add"),
                        IconName::FolderPlus,
                        SidebarCommand::AddWorkspace,
                    ),
                ] {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(label).icon(icon).on_click(
                        move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ = owner.update(cx, |this, cx| match action {
                                    SidebarCommand::Search => this.open_sidebar_search(window, cx),
                                    SidebarCommand::NewSession => this.new_session(window, cx),
                                    SidebarCommand::AddWorkspace => {
                                        this.open_workspace_add_menu(window, cx)
                                    }
                                });
                            });
                        },
                    ));
                }
                menu
            },
            cx,
        )
    }

    pub(in crate::pages::workspace::ui) fn set_sidebar_view(
        &mut self,
        view: View,
        cx: &mut Context<Self>,
    ) {
        self.save_title(cx);
        self.navigation.view = view;
        cx.notify();
    }

    fn on_sidebar_search(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_sidebar_search(window, cx);
    }

    pub(in crate::pages::workspace) fn open_sidebar_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.window.compact {
            self.toggle_mode(window, cx);
        }
        self.window.sidebar_visible = true;
        self.navigation.search_expanded = true;
        self.navigation
            .search
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(in crate::pages::workspace) fn close_sidebar_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation.search_expanded {
            self.navigation.search_expanded = false;
            self.navigation
                .search
                .update(cx, |input, cx| input.set_value("", window, cx));
            cx.notify();
        }
    }

    pub(in crate::pages::workspace) fn on_sidebar_search_event(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => {
                let query = self.navigation.search.read(cx).value().to_lowercase();
                let first = self.sidebar_session_ids(&query).first().copied();
                if let Some(id) = first {
                    self.select(id, window, cx);
                    self.focus_input(window, cx);
                }
            }
            InputEvent::Blur => {
                self.close_sidebar_search(window, cx);
            }
            _ => {}
        }
    }
}
