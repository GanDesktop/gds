use crate::shared::i18n::tr;
use bas::model::Timestamp;
use gds_uim::el;
use gpui_kit::component::menu::PopupMenuItem;

use super::groups;
use crate::entities::session::status::Status;
use crate::pages::workspace::ui::*;

pub(in crate::pages::workspace) struct Metadata {
    pub title: Option<String>,
    pub updated_at: Timestamp,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            title: None,
            updated_at: Timestamp::now(),
        }
    }
}

impl Workspace {
    pub(in crate::pages::workspace) fn draft_title(&self) -> String {
        self.navigation
            .drafts
            .metadata
            .get(&self.navigation.selected_directory)
            .and_then(|draft| draft.title.clone())
            .unwrap_or_else(|| tr!("session-new").into())
    }

    pub(in crate::pages::workspace) fn discard_new_draft(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected.id.is_some() || self.selected.creating || self.composer.paste_pending {
            return;
        }
        let directory = self.navigation.selected_directory.clone();
        if self
            .renaming
            .edit
            .as_ref()
            .is_some_and(|edit| edit.target == rename::Target::Draft(directory.clone()))
        {
            self.renaming.edit = None;
        } else {
            self.save_title(cx);
        }
        self.selected.error = None;
        self.composer.switch_target(Draft::default(), window, cx);
        self.navigation.drafts.remove(&directory);
        let next = self
            .sessions
            .entries
            .iter()
            .filter(|session| !session.archived)
            .min_by_key(|session| session.cwd.as_deref().map(groups::normalize) != directory)
            .map(|session| session.id);
        if let Some(next) = next {
            self.select(next, window, cx);
            self.navigation.drafts.remove(&directory);
        } else {
            self.selected.waiting_for_agent_selection = true;
            self.focus_input(window, cx);
        }
        cx.notify();
    }

    pub(super) fn draft_thread_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.renaming_draft();
        let group = SharedString::from(format!("draft-thread-{index}"));
        let now = Timestamp::now();
        let updated_at = self
            .navigation
            .drafts
            .metadata
            .get(&self.navigation.selected_directory)
            .map_or(now, |draft| draft.updated_at);
        let disabled = self.selected.creating || self.composer.paste_pending;
        let directory = self.navigation.selected_directory.clone();
        let title = if editing {
            self.render_title_editor(cx)
        } else {
            let directory = directory.clone();
            let generation = self.composer.generation;
            el! {
                .div {
                    # format!("draft-thread-title-text-{index}");
                    test_support;
                    min_w_0;
                    truncate;
                    text_size: crate::shared::ui::SMALL;
                    on_click: $(cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                                if event.click_count() == 2 {
                                    cx.stop_propagation();
                                    if !this.selected.creating && !this.composer.paste_pending {
                                        this.on_draft_row_rename(&directory, generation, window, cx);
                                    }
                                }
                    }));
                } [
                    $(self.draft_title()),
                ]
            }
            .into_any_element()
        };
        let title_row = el! {
            .div {
                # $(SharedString::from(format!("draft-thread-title-{index}")));
                test_support;
                h: layout::ICON_BUTTON_SIZE.px;
                flex_1;
                flex;
                items_center;
                min_w_0;
            } [
                title,
            ]
        };
        let actions = (!editing).then(|| el! {
            .div {
                # $(SharedString::from(format!("draft-thread-actions-{index}")));
                test_support;
                flex;
                size: layout::ICON_BUTTON_SIZE.px;
            } [
                ($(SharedString::from(format!("discard-draft-{index}"))), IconName::Trash).layout::icon_button {
                    accessibility_label: tr!("draft-discard");
                    tooltip: tr!("draft-discard");
                    disabled: disabled;
                    on_click: $(cx.listener(Self::on_draft_row_discard));
                },
            ]
        }.into_any_element());
        let state = if self.selected.creating {
            Status::Starting
        } else {
            Status::Idle
        };
        let trailing = gds::ui::hover_action(
            group.clone(),
            state.render(
                format!("draft-thread-status-{index}"),
                format!("draft-thread-indicator-{index}"),
                cx,
            ),
            actions,
        );
        el! {
            .div {
                # $(group.clone());
                test_support;
                group: group;
                w_full;
                flex_shrink_0;
                h: gds::appearance::tokens::SIDEBAR_ITEM_HEIGHT.px;
                px_2;
                rounded: 6.0.px;
                tooltip_show_delay: gds::appearance::tokens::TOOLTIP_DELAY;
                when: $(self.window.menu.is_none()), {
                    hoverable_tooltip: $(move |window, cx| crate::shared::ui::tooltip::view(crate::shared::ui::time::recent_time(updated_at, now, cx), window, cx));
                };
                bg: $(cx.theme().sidebar_accent);
                text_color: $(cx.theme().sidebar_accent_foreground);
                flex;
                items_center;
                gap_2;
                on_mouse_down: gpui_kit::MouseButton::Right, $(cx.listener(move |this, event, window, cx| {
                            this.on_draft_row_context_menu(directory.clone(), disabled, event, window, cx);
                }));
            } [
                .div {
                    h: layout::ICON_BUTTON_SIZE.px;
                    flex;
                    items_center;
                    flex_shrink_0;
                } [
                    $(self.agent_icon(format!("draft-thread-agent-{index}"), &self.selected.agent, cx)
                        .size(px(gds::appearance::tokens::SIDEBAR_ICON_SIZE))
                        .text_color(cx.theme().sidebar_accent_foreground)),
                ],
                title_row,
                trailing,
            ]
        }.into_any_element()
    }

    fn on_draft_row_discard(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.discard_new_draft(window, cx);
    }

    pub(super) fn on_draft_row_context_menu(
        &mut self,
        directory: Option<PathBuf>,
        disabled: bool,
        event: &gpui_kit::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.stop_propagation();
        self.open_context_menu(
            event.position,
            2,
            self.draft_menu(directory, disabled, cx),
            window,
            cx,
        );
    }

    fn draft_menu(
        &self,
        directory: Option<PathBuf>,
        disabled: bool,
        cx: &Context<Self>,
    ) -> menus::Builder {
        let owner = cx.entity().downgrade();
        let generation = self.composer.generation;
        std::rc::Rc::new(move |menu, window, cx| {
            let parent = window.window_handle();
            let rename_owner = owner.clone();
            let rename_directory = directory.clone();
            let discard_owner = owner.clone();
            let discard_directory = directory.clone();
            menu.item(
                shortcuts::menu_item(
                    tr!("session-rename"),
                    Some(Box::new(shortcuts::RenameThread)),
                )
                .icon(IconName::Pencil)
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    menus::in_parent(parent, window, cx, |window, cx| {
                        let _ = rename_owner.update(cx, |this, cx| {
                            this.on_draft_row_rename(&rename_directory, generation, window, cx);
                        });
                    });
                }),
            )
            .item(
                PopupMenuItem::new(tr!("draft-discard"))
                    .icon(crate::shared::ui::delete_icon(cx))
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        menus::in_parent(parent, window, cx, |window, cx| {
                            let _ = discard_owner.update(cx, |this, cx| {
                                this.on_draft_row_menu_discard(
                                    &discard_directory,
                                    generation,
                                    window,
                                    cx,
                                );
                            });
                        });
                    }),
            )
        })
    }

    fn on_draft_row_rename(
        &mut self,
        directory: &Option<PathBuf>,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected.id.is_none()
            && &self.navigation.selected_directory == directory
            && self.composer.generation == generation
        {
            self.begin_draft_rename(window, cx);
        }
    }

    fn on_draft_row_menu_discard(
        &mut self,
        directory: &Option<PathBuf>,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if &self.navigation.selected_directory == directory
            && self.composer.generation == generation
        {
            self.discard_new_draft(window, cx);
        }
    }
}
