use super::groups;
use crate::pages::workspace::ui::*;
use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::{PathPromptOptions, component::menu::PopupMenuItem};

impl Workspace {
    pub(in crate::pages::workspace) fn workspace_button(
        &self,
        presentation: Option<gds::ui::sidebar::SidebarPresentation>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let directories: Vec<_> = self
            .backend
            .workspaces
            .iter()
            .filter(|workspace| {
                !workspace.archived || self.navigation.view == super::View::Archived
            })
            .map(|workspace| (Some(workspace.cwd.clone()), workspace.name.clone()))
            .collect();
        let selected = self.navigation.selected_directory.clone();
        let name = self
            .workspace_for_directory(&selected)
            .map(|workspace| workspace.name.clone())
            .unwrap_or_else(|| groups::name(selected.as_deref()));
        let hint = tr!("workspace-choose");
        let view = cx.entity().downgrade();
        let parent = self.window.handle;
        let label = (!self.window.compact).then(|| {
            el! {
                .div {
                    # "workspace-picker-label";
                    test_support;
                    role: gpui_kit::Role::Label;
                    aria_label: $(name.clone());
                    min_w_0;
                    flex_1;
                    opacity: $(presentation.map_or(1., |value| value.label_opacity()));
                    whitespace_nowrap;
                    text_ellipsis;
                    text_left;
                    text_size: crate::shared::ui::SMALL;
                } [
                    $(name.clone()),
                ]
            }
        });
        let button = el! {
            ("workspace-picker").Button::icon_button {
                accessibility_label: $(name.clone());
                @if (let Some(presentation) = presentation)
                    w: $(px(presentation.width() - 2. * gds::appearance::tokens::SIDEBAR_PADDING));
                @else
                    @if (!self.window.compact) w_auto; px_2; @end;
                @end;
            } [
                .div { size_full; min_w_0; flex; items_center; overflow_hidden;
                    pl: $(px(presentation.map_or(0., |value| value.interpolate(8., 10.))));
                    pr: $(px(presentation.map_or(0., |value| value.interpolate(8., 0.))));
                    gap: $(px(presentation.map_or(0., |value| value.interpolate(8., 0.))));
                    when: $(presentation.is_none()), { justify_center; };
                } [
                    (IconName::Folder).Icon::new { size: gds::appearance::tokens::SIDEBAR_ICON_SIZE.px; flex_shrink_0; },
                    ..label,
                    @if (presentation.is_some_and(|value| value.actions_visible()))
                        (IconName::ChevronDown).Icon::new { size: crate::shared::ui::SMALL_ICON.px; flex_shrink_0; },
                    @end,
                ],
            ]
        };
        let menu = self.menu_button(
            button,
            directories.len() + 2,
            false,
            move |mut menu, _, _| {
                for (directory, name) in &directories {
                    let directory = directory.clone();
                    let view = view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(directory == selected)
                            .on_click(move |_, window, cx| {
                                menus::in_parent(parent, window, cx, |window, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        this.switch_workspace(directory.clone(), window, cx)
                                    });
                                });
                            }),
                    );
                }
                menu.separator().item(
                    shortcuts::menu_item(
                        tr!("workspace-add"),
                        Some(Box::new(shortcuts::AddWorkspace)),
                    )
                    .icon(IconName::Plus)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.open_workspace_add_menu(window, cx)
                                });
                            });
                        }
                    }),
                )
            },
            cx,
        );
        let context_menu =
            self.workspace_menu(self.navigation.selected_directory.clone(), false, cx);
        div()
            .id("workspace-selector")
            .test_support()
            .on_mouse_down(
                gpui_kit::MouseButton::Right,
                cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.open_context_menu(event.position, 6, context_menu.clone(), window, cx);
                }),
            )
            .child(crate::shared::ui::tooltip::bounded(
                "workspace-picker-hint",
                menu,
                hint,
            ))
    }

    pub(in crate::pages::workspace) fn switch_workspace(
        &mut self,
        directory: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation.selected_directory == directory {
            return;
        }
        let Some(workspace) = self.workspace_for_directory(&directory) else {
            return;
        };
        let archived = workspace.archived;
        let workspace = workspace.id;
        let remembered = self.navigation.workspace_sessions.get(&directory).copied();
        let target = self
            .sessions
            .entries
            .iter()
            .filter(|session| session.workspace == workspace && session.archived == archived)
            .max_by_key(|session| (Some(session.id) == remembered, session.updated_at))
            .map(|session| session.id);
        self.navigation.view = super::View::Workspaces;
        if let Some(id) = target {
            self.select(id, window, cx);
        } else {
            self.detach_thread(directory.clone(), window, cx);
            self.navigation.session_requested = false;
            let draft = self
                .navigation
                .drafts
                .contents
                .get(&directory)
                .cloned()
                .unwrap_or_default();
            self.composer.switch_target(draft, window, cx);
            self.focus_input(window, cx);
        }
        if archived {
            self.navigation.view = super::View::Archived;
        }
        self.sync_terminal_panel(window, cx);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn open_workspace_add_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation.picker.pending
            || self
                .navigation
                .picker
                .launch
                .as_ref()
                .is_some_and(|launch| launch.read(cx).is_active())
        {
            return;
        }
        self.close_menu(window, cx);
        let target = self.backend.server.clone();
        let profiles = crate::features::navigation::shared(cx);
        let local = profiles
            .read(cx)
            .matching(&target)
            .is_some_and(|profile| profile.local);
        let tasks = cx
            .global::<crate::entities::agent::catalog::Service>()
            .tasks()
            .clone();
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        let expected = target.clone();
        let launch = crate::features::workspace_launch::open(
            target,
            local,
            tasks,
            move |workspace, cx| {
                let _ = parent.update(cx, |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if this.backend.server != expected {
                            return;
                        }
                        if this.window.presented && this.window.compact {
                            this.toggle_mode(window, cx);
                        }
                        this.workspace_completed(
                            connection::WorkspaceMutation::Create {
                                cwd: workspace.cwd.clone(),
                            },
                            Ok(Some(workspace)),
                            window,
                            cx,
                        );
                    });
                });
            },
            window,
            cx,
        );
        cx.subscribe_in(
            &launch,
            window,
            |this, _, _: &crate::features::workspace_launch::ChooseDirectory, window, cx| {
                this.choose_local_workspace(window, cx);
            },
        )
        .detach();
        self.navigation.picker.launch = Some(launch);
    }

    pub(in crate::pages::workspace) fn choose_local_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation.picker.pending {
            return;
        }
        self.navigation.picker.pending = true;
        self.navigation.picker.session = None;
        self.navigation.picker.waiting_activation = true;
        cx.defer_in(window, |this, window, cx| {
            this.close_menu(window, cx);
            if this.window.presented {
                window.activate_window();
                this.resume_workspace_picker(window, cx);
            }
        });
        cx.notify();
    }

    pub(in crate::pages::workspace) fn resume_workspace_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.window.presented
            || !self.navigation.picker.waiting_activation
            || !window.is_window_active()
        {
            return;
        }
        self.navigation.picker.waiting_activation = false;
        // Wait past the native activation event so Linux has recorded the dialog's parent.
        cx.on_next_frame(window, |this, window, cx| {
            if this.window.presented && window.is_window_active() {
                this.prompt_local_workspace(window, cx);
            } else {
                this.navigation.picker.waiting_activation = true;
            }
        });
    }

    fn prompt_local_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.navigation.picker.session.take();
        let launch = target
            .is_none()
            .then(|| self.navigation.picker.launch.clone())
            .flatten()
            .filter(|launch| launch.read(cx).choosing_directory());
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: target.is_some(),
            prompt: Some(
                if target.is_some() {
                    tr!("session-directory-add")
                } else {
                    tr!("workspace-choose-folder")
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = async { paths.await? }.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.navigation.picker.pending = false;
                if let Some(launch) = launch {
                    let result = result
                        .map(|paths| paths.and_then(|paths| paths.into_iter().next()))
                        .map_err(|error| format!("{error:#}"));
                    launch.update(cx, |launch, cx| launch.directory_chosen(result, window, cx));
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Some(paths)) => {
                        if let Some((session, epoch)) = target {
                            this.attach_directories(session, epoch, paths, cx);
                            cx.notify();
                            return;
                        }
                        if let Some(path) = paths.into_iter().next() {
                            if this.window.presented && this.window.compact {
                                this.toggle_mode(window, cx);
                            }
                            this.navigation.picker.pending = true;
                            this.command(
                                Command::Workspace(connection::WorkspaceMutation::Create {
                                    cwd: path,
                                }),
                                cx,
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(error) => this.operation_error(
                        tr!("workspace-select-failed"),
                        tr!("workspace-select-error", error = format!("{:#}", error)),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
