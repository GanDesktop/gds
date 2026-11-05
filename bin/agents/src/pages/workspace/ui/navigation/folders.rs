use super::groups;
use crate::pages::workspace::ui::*;
use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::{PathPromptOptions, component::menu::PopupMenuItem};

impl Workspace {
    pub(in crate::pages::workspace) fn workspace_button(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sessions = self.visible_sessions();
        let groups = self.grouped_threads(&sessions, "");
        let directories: Vec<_> = groups.into_iter().map(|group| group.directory).collect();
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
                    whitespace_nowrap;
                    text_ellipsis;
                    text_size: crate::shared::ui::SMALL;
                } [
                    $(name.clone()),
                ]
            }
        });
        let button = el! {
            ("workspace-picker").Button::new {
                ghost;
                xsmall;
                h: layout::ICON_BUTTON_SIZE.px;
                control_icon: IconName::Folder;
                accessibility_label: $(name.clone());
                when: $(self.window.compact), {
                    w: layout::ICON_BUTTON_SIZE.px;
                    h: layout::ICON_BUTTON_SIZE.px;
                    p_0;
                };
            } [
                ..label,
            ]
        };
        let menu = self.menu_button(
            button,
            directories.len() + 2,
            false,
            move |mut menu, _, _| {
                for directory in &directories {
                    let directory = directory.clone();
                    let view = view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(groups::path_label(directory.as_deref()))
                            .checked(directory == selected)
                            .on_click(move |_, window, cx| {
                                menus::in_parent(parent, window, cx, |window, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        this.new_thread(directory.clone(), window, cx)
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
        crate::shared::ui::tooltip::bounded("workspace-picker-hint", menu, hint)
    }

    pub(in crate::pages::workspace) fn open_workspace_add_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = window.mouse_position();
        // The previous popup dismisses after its item callback. Open the next menu afterward.
        cx.defer_in(window, move |this, window, cx| {
            let owner = cx.entity().downgrade();
            let parent = this.window.handle;
            let pending = this.navigation.picker.pending;
            let build: menus::Builder = std::rc::Rc::new(move |menu, _, _| {
                let owner = owner.clone();
                menu.item(
                    PopupMenuItem::new(tr!("workspace-local-folder"))
                        .icon(IconName::Folder)
                        .disabled(pending)
                        .on_click(move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.choose_local_workspace(window, cx);
                                });
                            });
                        }),
                )
            });
            this.open_context_menu(position, 1, build, window, cx);
        });
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
