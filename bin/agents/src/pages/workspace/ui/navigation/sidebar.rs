use super::{View, groups, history};
use crate::pages::workspace::ui::*;
use crate::shared::i18n::tr;
use gds::ui::{disclosure::DisclosureIcon, sidebar::*};
use gds_uim::el;
use gds_uim::m;
use gpui_kit::component::menu::PopupMenuItem;

const WORKSPACE_MENU_ROWS: usize = 6;

impl Workspace {
    pub(in crate::pages::workspace) fn render_sidebar(
        &self,
        presentation: SidebarPresentation,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let query = self.navigation.search.read(cx).value().to_lowercase();
        let cycle = self.thread_cycle_targets(cx);
        let sessions = self.sidebar_sessions();
        let groups = if self.navigation.view.flat() {
            Vec::new()
        } else {
            self.grouped_threads(&sessions, &query)
                .into_iter()
                .filter(|group| group.directory == self.navigation.selected_directory)
                .collect()
        };
        let header = self.sidebar_controls(presentation, cx);
        let empty = (if self.navigation.view == View::Workspaces {
            false
        } else if self.navigation.view.flat() {
            !sessions
                .iter()
                .any(|session| matches_query(session, &query))
        } else {
            groups.is_empty()
        })
        .then(|| {
            el! {
                .div {
                    p_3;
                    whitespace_nowrap;
                    overflow_hidden;
                    opacity: $(presentation.label_opacity());
                } [
                    (tr!("session-no-results"), cx).caption,
                ]
            }
        });
        let columns = if self.navigation.view == View::Workspaces {
            let mut rows = Vec::new();
            if self.selected.id.is_none() && query.is_empty() {
                rows.push(self.draft_thread_row(0, presentation, cx));
            }
            rows.extend(
                history::sorted(&sessions)
                    .into_iter()
                    .filter(|session| matches_query(session, &query))
                    .map(|session| self.thread_row(session, &cycle, presentation, cx)),
            );
            if rows.is_empty() {
                rows.push(
                    div()
                        .p_3()
                        .opacity(presentation.label_opacity())
                        .child(caption(tr!("session-no-results"), cx))
                        .into_any_element(),
                );
            }
            rows
        } else if self.navigation.view == View::History {
            let now = jiff::Zoned::now();
            history::groups(&sessions, &query, &now)
                .into_iter()
                .map(|group| {
                    let label = group.period.label(now.date());
                    el! {
                        .SideBarSection::default [
                            .SideBarSectionHeader::default [
                                (~label).SideBarSectionTitle::new {
                                    # $(group.period.id());
                                    role: gpui_kit::Role::Label;
                                    aria_label: ~label;
                                    px_3;
                                    pt_3;
                                    pb_1;
                                    whitespace_nowrap;
                                    overflow_hidden;
                                    opacity: $(presentation.label_opacity());
                                },
                            ],
                            .SideBarSectionContent::default [
                                ..$(group.sessions.into_iter().map(|session| self.thread_row(session, &cycle, presentation, cx))),
                            ],
                        ]
                    }
                    .into_any_element()
                })
                .collect::<Vec<_>>()
        } else if self.navigation.view == View::Bookmarks {
            history::sorted(&sessions)
                .into_iter()
                .filter(|session| matches_query(session, &query))
                .map(|session| self.thread_row(session, &cycle, presentation, cx))
                .collect()
        } else {
            groups
                .into_iter()
                .enumerate()
                .map(|(index, group)| {
                    let collapsed =
                        query.is_empty() && self.navigation.collapsed.contains(&group.directory);
                    let directory = group.directory.clone();
                    let draft_selected = self.navigation.view == View::Workspaces
                        && self.selected.id.is_none()
                        && self.navigation.selected_directory == directory;
                    let header = self.workspace_header(index, directory, collapsed, presentation, cx);
                    let draft =
                        (!collapsed && draft_selected).then(|| self.draft_thread_row(index, presentation, cx));
                    let empty =
                        (!collapsed && group.threads.is_empty() && !draft_selected).then(|| {
                            el! {
                                (tr!("session-empty"), cx).caption {
                                    ml_5;
                                    px_3;
                                    py_2;
                                    whitespace_nowrap;
                                    overflow_hidden;
                                    opacity: $(presentation.label_opacity());
                                }
                            }
                        });
                    let threads = group
                        .threads
                        .into_iter()
                        .filter(|_| !collapsed)
                        .map(|session| self.thread_row(session, &cycle, presentation, cx));
                    el! {
                        .SideBarSection::default [
                            .SideBarSectionHeader::default [header],
                            .SideBarSectionContent::default { pl: $(px(presentation.interpolate(8., 0.))); } [
                                ..draft,
                                ..empty,
                                ..threads,
                            ],
                        ]
                    }
                    .into_any_element()
                })
                .collect::<Vec<_>>()
        };
        let pinned = self.pinned_sessions();
        let pinned = history::sorted(&pinned)
            .into_iter()
            .filter(|session| matches_query(session, &query))
            .map(|session| self.thread_row(session, &cycle, presentation, cx))
            .collect::<Vec<_>>();
        let picker = self
            .workspace_button(Some(presentation), cx)
            .into_any_element();
        el! {
            .SideBar::default { presentation: presentation; } [
                .SideBarHeader::default [
                    @if (!pinned.is_empty())
                        .div {
                            # "pinned-sessions"; test_support; flex; flex_col; gap_1;
                            max_h: 180.0.px; overflow_y_scroll;
                            px: gds::appearance::tokens::SIDEBAR_PADDING.px;
                        } [..pinned],
                    @end,
                    .div { px: gds::appearance::tokens::SIDEBAR_PADDING.px; py_1; } [picker],
                    header,
                ],
                .SideBarContent::default { # "agents-sidebar-content"; overflow_hidden } [
                    .div {
                        # "session-list";
                        test_support;
                        flex_1;
                        min_h_0;
                        overflow_y_scroll;
                        track_scroll: $(&self.navigation.scroll);
                        px: gds::appearance::tokens::SIDEBAR_PADDING.px;
                        flex;
                        flex_col;
                        gap_1;
                    } [
                        ..columns,
                        ..empty,
                    ],
                ],
            ]
        }
    }

    pub(super) fn on_sidebar_add_workspace(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_add_menu(window, cx);
    }

    pub(super) fn on_sidebar_new_session(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_session(window, cx);
    }

    fn workspace_header(
        &self,
        index: usize,
        directory: Option<PathBuf>,
        collapsed: bool,
        presentation: SidebarPresentation,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = SharedString::from(format!("workspace-header-{index}"));
        let disclosure_id = SharedString::from(format!("workspace-disclosure-{directory:?}"));
        let name = self
            .workspace_for_directory(&directory)
            .map(|workspace| workspace.name.clone())
            .unwrap_or_else(|| groups::name(directory.as_deref()));
        let active = self.navigation.selected_directory == directory;
        let can_create = self.can_remove_workspace(&directory)
            && self
                .workspace_for_directory(&directory)
                .is_some_and(|workspace| !workspace.archived);
        let new_directory = directory.clone();
        let menu = self.workspace_menu(directory.clone(), collapsed, cx);
        let button_menu = menu.clone();
        let more_button = el! {
            ($(SharedString::from(format!("workspace-menu-{index}"))), IconName::Ellipsis).layout::icon_button {
                accessibility_label: tr!("workspace-more");
                tooltip: tr!("workspace-more");
                on_click: $(|_, _, cx| cx.stop_propagation());
            }
        };
        let more = self.menu_button(
            more_button,
            WORKSPACE_MENU_ROWS,
            false,
            move |menu, window, cx| button_menu(menu, window, cx),
            cx,
        );
        let actions = el! {
            .div {
                # $(SharedString::from(format!("workspace-actions-{index}")));
                test_support;
                flex;
                items_center;
                flex_shrink_0;
                invisible;
                when: $(presentation.actions_visible()), {
                    group_hover: $(group.clone()), m! {visible;};
                };
            } [
                ($(SharedString::from(format!("new-thread-{index}"))), IconName::Plus).layout::icon_button {
                    accessibility_label: tr!("workspace-new-session");
                    tooltip: tr!("workspace-new-session");
                    disabled: $(!can_create);
                    on_click: $(cx.listener(move |this, _, window, cx| {
                                this.on_sidebar_new_thread(new_directory.clone(), window, cx);
                    }));
                },
                more,
            ]
        };
        el! {
            .div {
                # $(group.clone());
                test_support;
                group: group;
                role: gpui_kit::Role::Button;
                aria_label: $(name.clone());
                aria_expanded: $(!collapsed);
                w_full;
                h: gds::appearance::tokens::SIDEBAR_ITEM_HEIGHT.px;
                rounded: 6.0.px;
                flex;
                items_center;
                gap_2;
                pl: $(px(presentation.interpolate(8., 10.)));
                pr_2;
                overflow_hidden;
                flex_shrink_0;
                text_color: $(cx.theme().muted_foreground);
                when: active, {
                    text_color: $(cx.theme().sidebar_foreground);
                };
                cursor_pointer;
                hover: {
                    bg: $(gds::appearance::theme::Colors::current(cx).sidebar_hover());
                };
                on_click: $(cx.listener(move |this, _, _, cx| {
                            this.toggle_workspace_collapsed(directory.clone(), cx)
                }));
                on_mouse_down: gpui_kit::MouseButton::Right, $(cx.listener(move |this, event, window, cx| {
                            this.on_sidebar_context_menu(menu.clone(), event, window, cx);
                }));
            } [
                .div {
                    # $(SharedString::from(format!("workspace-{index}")));
                    test_support;
                    flex_1;
                    min_w_0;
                    flex;
                    items_center;
                    gap_2;
                    hoverable_tooltip: $(move |window, cx| {
                            crate::shared::ui::tooltip::view(
                                if collapsed {
                                    tr!("workspace-expand")
                                } else {
                                    tr!("workspace-collapse")
                                },
                                window,
                                cx,
                            )
                    });
                } [
                    .DisclosureIcon::default {
                        # disclosure_id;
                        expanded: $(!collapsed);
                    },
                    .div {
                        text_size: crate::shared::ui::SMALL;
                        truncate;
                        opacity: $(presentation.label_opacity());
                    } [
                        name,
                    ],
                ],
                actions,
            ]
        }.into_any_element()
    }

    fn on_sidebar_new_thread(
        &mut self,
        directory: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.new_thread(directory, window, cx);
    }

    fn on_sidebar_context_menu(
        &mut self,
        menu: menus::Builder,
        event: &gpui_kit::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.stop_propagation();
        self.open_context_menu(event.position, WORKSPACE_MENU_ROWS, menu, window, cx);
    }

    fn toggle_workspace_collapsed(&mut self, directory: Option<PathBuf>, cx: &mut Context<Self>) {
        if !self.navigation.collapsed.remove(&directory) {
            self.navigation.collapsed.insert(directory);
        }
        cx.notify();
    }

    pub(super) fn workspace_menu(
        &self,
        directory: Option<PathBuf>,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> menus::Builder {
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        let removable = self.can_remove_workspace(&directory);
        let archived = self
            .workspace_for_directory(&directory)
            .is_some_and(|workspace| workspace.archived);
        std::rc::Rc::new(move |menu, _, _| {
            let new_owner = owner.clone();
            let new_directory = directory.clone();
            let collapse_owner = owner.clone();
            let collapse_directory = directory.clone();
            let add_owner = owner.clone();
            let archive_owner = owner.clone();
            let archive_directory = directory.clone();
            let remove_owner = owner.clone();
            let remove_directory = directory.clone();
            let path = directory
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            menu.item(
                shortcuts::menu_item(tr!("session-new"), Some(Box::new(NewSession)))
                    .icon(IconName::Plus)
                    .disabled(archived || !removable)
                    .on_click(move |_, window, cx| {
                        menus::in_parent(parent, window, cx, |window, cx| {
                            let _ = new_owner.update(cx, |this, cx| {
                                this.new_thread(new_directory.clone(), window, cx)
                            });
                        });
                    }),
            )
            .item(
                PopupMenuItem::new(if collapsed {
                    tr!("workspace-expand")
                } else {
                    tr!("workspace-collapse")
                })
                .icon(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .on_click(move |_, window, cx| {
                    menus::in_parent(parent, window, cx, |_, cx| {
                        let _ = collapse_owner.update(cx, |this, cx| {
                            this.toggle_workspace_collapsed(collapse_directory.clone(), cx)
                        });
                    });
                }),
            )
            .item(
                PopupMenuItem::new(tr!("workspace-copy-path"))
                    .icon(IconName::Copy)
                    .disabled(path.is_none())
                    .on_click(move |_, _, cx| {
                        Self::on_sidebar_copy_path(path.as_deref(), cx);
                    }),
            )
            .item(
                shortcuts::menu_item(
                    tr!("workspace-add"),
                    Some(Box::new(shortcuts::AddWorkspace)),
                )
                .icon(IconName::FolderPlus)
                .on_click(move |_, window, cx| {
                    menus::in_parent(parent, window, cx, |window, cx| {
                        let _ = add_owner
                            .update(cx, |this, cx| this.open_workspace_add_menu(window, cx));
                    });
                }),
            )
            .item(
                PopupMenuItem::new(if archived {
                    tr!("workspace-restore")
                } else {
                    tr!("workspace-archive")
                })
                .icon(if archived {
                    IconName::ArchiveRestore
                } else {
                    IconName::Archive
                })
                .disabled(!removable)
                .on_click(move |_, window, cx| {
                    menus::in_parent(parent, window, cx, |_, cx| {
                        let _ = archive_owner.update(cx, |this, cx| {
                            this.request_archive_workspace(archive_directory.clone(), cx)
                        });
                    });
                }),
            )
            .item(
                PopupMenuItem::new(tr!("workspace-delete"))
                    .icon(IconName::Trash)
                    .disabled(!removable)
                    .on_click(move |_, window, cx| {
                        menus::in_parent(parent, window, cx, |_, cx| {
                            let _ = remove_owner.update(cx, |this, cx| {
                                this.request_remove_workspace(remove_directory.clone(), cx);
                            });
                        });
                    }),
            )
        })
    }

    fn on_sidebar_copy_path(path: Option<&str>, cx: &mut App) {
        if let Some(path) = path {
            cx.write_to_clipboard(ClipboardItem::new_string(path.to_owned()));
        }
    }
}
