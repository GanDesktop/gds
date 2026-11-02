use super::*;
use crate::shared::i18n::tr;
use gds_uim::el;

const POPUP_WIDTH: f32 = 640.;
pub(super) const INPUT_HEIGHT: f32 = 98.;
const COMPACT_HEADER_HEIGHT: f32 = 28.;
const PANEL_BORDER_WIDTH: f32 = 1.;
const COMMAND_ROW_HEIGHT: f32 = 44.;
const COMMAND_HINT_HEIGHT: f32 = 28.;
const OUTPUT_HEIGHT: f32 = 440.;
pub(super) const HEADER_HEIGHT: f32 = crate::shared::ui::window_controls::HEIGHT;
pub(super) const ICON_BUTTON_SIZE: f32 = crate::shared::ui::COMPACT_CONTROL;
pub(super) const MACOS_WINDOW_CONTROLS_WIDTH: f32 =
    crate::shared::ui::window_controls::MACOS_CONTROLS_WIDTH;

pub(super) fn icon_button(id: impl Into<gpui_kit::ElementId>, icon: IconName) -> Button {
    crate::shared::ui::icon_button(id, icon)
}

pub(crate) fn initial_size(compact: bool) -> Size<Pixels> {
    if compact {
        size(px(POPUP_WIDTH), px(INPUT_HEIGHT))
    } else {
        size(px(1280.), px(820.))
    }
}

impl Workspace {
    pub(super) fn prepare_to_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.audio.update(cx, |player, cx| player.reset(cx));
        self.save_title(cx);
        self.remember_thread(cx);
        self.close_reply_preview(window, cx);
        self.close_menu(window, cx);
    }

    pub(super) fn close_window(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        // The Host owns all backend workspaces and the single close/confirm boundary.
        cx.emit(Event::CloseWindow);
    }

    pub(super) fn resize_window(&mut self, window: &mut Window, cx: &App) {
        if !self.window.presented {
            return;
        }
        let command_count = self
            .command_candidates(cx)
            .len()
            .min(commands::VISIBLE_COMMANDS);
        let command_height = command_count as f32 * COMMAND_ROW_HEIGHT
            + if command_count > 0 || self.command_hint(cx).is_some() {
                COMMAND_HINT_HEIGHT
            } else {
                0.
            };
        let menu_height = self
            .window
            .menu
            .as_ref()
            .filter(|menu| menu.window.is_none())
            .map_or(0., |menu| f32::from(menu.size.height));
        let menu_height = menu_height
            + self
                .window
                .reply_preview
                .as_ref()
                .filter(|preview| preview.window.is_none())
                .map_or(0., |preview| f32::from(preview.size.height));
        let size = if self.window.compact {
            size(
                px(POPUP_WIDTH),
                px(if self.window.output_open {
                    OUTPUT_HEIGHT.max(
                        COMPACT_HEADER_HEIGHT
                            + self.composer_height(cx)
                            + self.feedback_height(cx)
                            + composer::COMPOSER_TOOLBAR_HEIGHT
                            + composer::COMPOSER_BOTTOM_PADDING
                            + PANEL_BORDER_WIDTH * 2.
                            + 100.,
                    ) + command_height
                        + menu_height
                } else {
                    COMPACT_HEADER_HEIGHT
                        + self.composer_height(cx)
                        + self.feedback_height(cx)
                        + composer::COMPOSER_TOOLBAR_HEIGHT
                        + composer::COMPOSER_BOTTOM_PADDING
                        + PANEL_BORDER_WIDTH * 2.
                        + command_height
                        + menu_height
                }),
            )
        } else {
            self.window.size
        };
        if self.window.compact && self.window.popup_size == Some(size) {
            return;
        }
        self.window.popup_size = self.window.compact.then_some(size);
        if window.bounds().size != size {
            window.resize(size);
        }
        let title = if self.window.compact {
            tr!("app-quick-input")
        } else {
            "Agents"
        };
        window.set_window_title(&format!("{title} — {}", self.backend.server));
    }

    pub(super) fn toggle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_title(cx);
        self.close_reply_preview(window, cx);
        self.close_menu(window, cx);
        self.window.usage_details = None;
        if !self.window.compact {
            if window.is_maximized() {
                window.zoom_window();
            } else {
                self.window.size = window.bounds().size;
            }
        }
        self.window.compact = !self.window.compact;
        if !self.window.compact {
            self.window.sidebar_visible = true;
            if !self.thread_has_user_prompt() {
                self.composer.expanded = true;
                self.sync_composer_mode(cx);
            }
        }
        self.resize_window(window, cx);
        self.selected.timeline.list.remeasure_items(
            0..self
                .selected
                .timeline
                .conversation
                .as_ref()
                .map_or(0, |state| state.rows.len()),
        );
        self.focus_input(window, cx);
        cx.notify();
    }

    pub(super) fn toggle_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window.compact {
            self.close_reply_preview(window, cx);
            self.window.output_open = !self.window.output_open;
            if self.window.output_open {
                self.selected
                    .timeline
                    .list
                    .set_follow_mode(FollowMode::Tail);
            }
            self.resize_window(window, cx);
            cx.notify();
        }
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation.search_expanded {
            self.close_sidebar_search(window, cx);
            self.focus_input(window, cx);
            return;
        }
        if self.cancel_queue_edit(window, cx) {
            return;
        }
        if self.window.reply_preview.is_some() {
            self.close_reply_preview(window, cx);
        } else if self.window.menu.is_some() {
            self.close_menu(window, cx);
        } else if !self.command_candidates(cx).is_empty() {
            self.composer.commands.dismissed = true;
            if self.window.compact {
                self.resize_window(window, cx);
            }
            cx.notify();
        } else if self.renaming.edit.is_some() {
            self.cancel_rename(window, cx);
        } else if !self.window.compact {
            self.toggle_mode(window, cx);
        } else if self.window.output_open {
            self.toggle_output(window, cx);
        } else {
            self.close_window(window, cx);
        }
    }

    pub(super) fn mode_button(&self, cx: &mut Context<Self>) -> Button {
        el! {
            ("toggle-mode", $(if self.window.compact {
                        IconName::Maximize2
                    } else {
                        IconName::Minimize2
            })).icon_button {
                tooltip_with_shortcut: $(if self.window.compact {
                            tr!("window-expand")
                        } else {
                            tr!("window-compact")
                        }), $(shortcuts::shortcut(ToggleMode));
                on_click: $(cx.listener(Self::on_layout_toggle_mode));
            }
        }
    }

    pub(super) fn render_layout(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(left) = crate::shared::ui::input_geometry::content_left(
            &self.composer.input,
            gpui_kit::component::Size::Medium.input_px(),
            window,
            cx,
        ) {
            self.composer.content_left.set(left);
        }
        self.composer
            .available_height
            .set((f32::from(window.viewport_size().height) - 180.).max(80.));
        el! {
            .div {
                # "bas-workspace";
                key_context: "BasWorkspace";
                size_full;
                flex;
                text_color: $(cx.theme().foreground);
                capture_any_mouse_down: $(cx.listener(Self::on_layout_mouse_down));
                on_action: $(cx.listener(|this, _: &gpui_kit::component::input::Enter, window, cx| {
                    if this.terminal_focused(window, cx) { cx.propagate(); }
                }));
                on_action: $(cx.listener(Self::on_layout_new_session_action));
                on_action: $(cx.listener(Self::on_layout_next_thread));
                on_action: $(cx.listener(Self::on_layout_previous_thread));
                on_action: $(cx.listener(Self::on_layout_focus_composer));
                on_action: $(cx.listener(Self::on_terminal_focus_composer));
                on_action: $(cx.listener(Self::on_composer_expand_action));
                on_action: $(cx.listener(Self::on_layout_focus_search));
                on_action: $(cx.listener(Self::on_layout_toggle_sidebar_action));
                on_action: $(cx.listener(Self::on_layout_toggle_mode_action));
                on_action: $(cx.listener(Self::on_layout_toggle_output_action));
                on_action: $(cx.listener(Self::on_layout_dismiss));
                on_action: $(cx.listener(Self::on_layout_refresh));
                on_action: $(cx.listener(Self::on_layout_scroll_end));
                on_action: $(cx.listener(Self::on_layout_open_settings));
                on_action: $(cx.listener(Self::on_rename_thread));
                on_action: $(cx.listener(Self::on_archive_thread));
                on_action: $(cx.listener(Self::on_toggle_thread_pin));
                on_action: $(cx.listener(Self::on_toggle_thread_bookmark));
                on_action: $(cx.listener(Self::on_delete_thread));
                on_action: $(cx.listener(Self::on_add_workspace));
            } [
                $(if self.window.compact {
                        self.render_popup(cx)
                    } else {
                        self.render_window(window, cx)
                }),
                ..$(self.inline_context_menu(window)),
                ..$(self.inline_preview()),
            ]
        }
        .on_action(cx.listener(Self::on_conversation_top))
        .on_action(cx.listener(Self::on_conversation_page_up))
        .on_action(cx.listener(Self::on_conversation_page_down))
        .on_action(cx.listener(Self::on_conversation_previous_turn))
        .on_action(cx.listener(Self::on_conversation_next_turn))
        .on_action(cx.listener(Self::on_toggle_session))
        .on_action(cx.listener(Self::on_open_session_manager))
        .into_any_element()
    }

    fn render_popup(&self, cx: &mut Context<Self>) -> AnyElement {
        let preview = self
            .window
            .reply_preview
            .as_ref()
            .filter(|preview| preview.window.is_none())
            .map(|preview| {
                el! {
                    .div {
                        flex_shrink_0;
                        h: $(preview.size.height);
                    } [
                        $(preview.content.clone()),
                    ]
                }
            });
        let menu = self
            .window
            .menu
            .as_ref()
            .filter(|menu| menu.window.is_none())
            .map(|menu| {
                el! {
                    .div {
                        flex_shrink_0;
                        h: $(menu.size.height);
                    } [
                        $(menu.content.clone()),
                    ]
                }
            });
        let output = self
            .window
            .output_open
            .then(|| self.render_layout_compact_output(cx));
        el! {
            .div {
                # "minimal-mode";
                size_full;
                flex;
                flex_col;
                rounded_xl;
                overflow_hidden;
                border_1;
                border_color: $(cx.theme().border);
                bg: $(theme::content_background(cx));
            } [
                ..preview,
                ..menu,
                ..output,
                $(self.render_layout_compact_header(cx)),
                $(self.render_composer(cx)),
            ]
        }
        .into_any_element()
    }

    fn render_layout_compact_output(&self, cx: &mut Context<Self>) -> AnyElement {
        let feedback = self
            .selected
            .timeline
            .conversation
            .as_ref()
            .is_some_and(|conversation| !conversation.notices.is_empty())
            .then(|| {
                el! {
                    .div {
                        # "compact-feedback";
                        test_support;
                        flex_shrink_0;
                        max_h: $(px(OUTPUT_HEIGHT / 2.));
                        overflow_y_scroll;
                        pl: chat::THREAD_BODY_PADDING.px;
                        pr: $(px(turn_navigation::RAIL_WIDTH + chat::THREAD_BODY_PADDING));
                    } [
                        ..$(self.render_notices(cx)),
                    ]
                }
            });
        el! {
            .div {
                # "agent-output";
                test_support;
                flex;
                flex_col;
                flex_1;
                min_h_0;
                border_b_1;
                border_color: $(cx.theme().border);
            } [
                ..feedback,
                $(self.render_timeline(cx)),
            ]
        }
        .into_any_element()
    }

    fn render_layout_compact_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let archived = self
            .current()
            .is_some_and(|session| session.archived)
            .then(|| caption(tr!("session-archived"), cx));
        let activity = self.activity_label().map(|label| {
            el! {
                (label, cx).caption {
                    flex_1;
                    min_w_0;
                    truncate;
                }
            }
        });
        let summary = self
            .activity_label()
            .is_none()
            .then(|| self.render_reply_summary(cx))
            .flatten();
        el! {
            .div {
                # "compact-status";
                test_support;
                flex_1;
                cursor_grab;
                min_w_0;
                flex;
                items_center;
                gap_1;
                text_size: crate::shared::ui::SMALL;
                on_mouse_down: gpui_kit::MouseButton::Left, Self::on_layout_window_move;
            } [
                $(self.session_indicator("compact-connection", cx)),
                ..archived,
                ..activity,
                ..summary,
            ]
        }
        .into_any_element()
    }

    fn render_layout_compact_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let navigation = (self.selected.id.is_some() && self.window.output_open)
            .then(|| self.render_conversation_navigation(cx));
        let restore = self
            .current()
            .filter(|session| session.archived)
            .map(|session| self.archive_button(session, "compact-restore", cx));
        let authentication = self
            .selected
            .authentication
            .required
            .is_some()
            .then(|| self.authentication_button(cx));
        el! {
            .div {
                # "compact-header";
                test_support;
                flex;
                items_center;
                gap_1;
                mx: composer::COMPOSER_SIDE_PADDING.px;
                pl: composer::COMPOSER_CONTROL_INSET.px;
                pr: composer::COMPOSER_CONTROL_INSET.px;
                when: cfg!(target_os = "macos"), {
                    pl: MACOS_WINDOW_CONTROLS_WIDTH.px;
                };
                h: COMPACT_HEADER_HEIGHT.px;
                flex_shrink_0;
            } [
                ("toggle-output", $(if self.window.output_open { IconName::ChevronDown } else { IconName::ChevronRight })).icon_button {
                    tooltip_with_shortcut: tr!("conversation-toggle"), $(shortcuts::shortcut(ToggleOutput));
                    on_click: $(cx.listener(Self::on_layout_toggle_output));
                },
                $(self.agent_button(cx)),
                ..$(self.render_plan_indicator(cx)),
                $(self.render_layout_compact_status(cx)),
                ..navigation,
                ..restore,
                ..authentication,
                $(self.application_button(cx)),
                $(self.workspace_button(cx)),
                $(self.terminal_toggle_button(cx)),
                ("new-session", IconName::Plus).icon_button {
                    tooltip_with_shortcut: tr!("session-new"), $(shortcuts::shortcut(NewSession));
                    on_click: $(cx.listener(Self::on_layout_new_session));
                },
                $(self.mode_button(cx)),
            ]
        }.into_any_element()
    }

    fn render_window(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use gds::ui::sidebar::{SideBar, SidebarState};
        let sidebar = SideBar::default().state(if self.window.sidebar_visible {
            SidebarState::Expanded
        } else {
            SidebarState::Collapsed
        });
        let presentation = sidebar.sample("workspace-sidebar", window, cx);
        let frame = gds::ui::Frame::default().sidebar_width(presentation.width());
        let sidebar = self.render_sidebar(presentation, cx).into_any_element();
        let menu = self.application_button(cx);
        let controls = self.header_row(window, cx).into_any_element();
        let toolbar =
            frame.toolbar_with_window_controls(menu, controls, self.window_controls(cx), cx);
        let content = div()
            .flex()
            .flex_col()
            .size_full()
            .children(self.archive_notice(cx))
            .child(self.render_chat(cx))
            .into_any_element();
        let content = self.render_terminal_dock(content, cx);
        let workspace = if self.terminals.visible {
            frame.workspace_with_content_surface(sidebar, content, cx)
        } else {
            frame.workspace(sidebar, content, cx)
        };
        let server = self.server_status_footer(presentation, cx);
        let status = div()
            .children(self.activity_label().map(|label| caption(label, cx)))
            .into_any_element();
        let footer = frame.footer(
            server,
            status,
            self.terminal_toggle_button(cx).into_any_element(),
            cx,
        );
        div()
            .id("window-mode")
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(workspace)
            .child(footer)
            .into_any_element()
    }

    fn on_layout_toggle_mode(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_mode(window, cx);
    }

    fn on_layout_toggle_output(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_output(window, cx);
    }

    fn on_layout_new_session(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_session(window, cx);
    }

    fn on_layout_window_move(_: &gpui_kit::MouseDownEvent, window: &mut Window, _: &mut App) {
        window.start_window_move();
    }

    fn on_layout_mouse_down(
        &mut self,
        event: &gpui_kit::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu_outside(event.position, window, cx);
    }

    fn on_layout_new_session_action(
        &mut self,
        _: &NewSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.new_session(window, cx);
    }

    fn on_layout_next_thread(
        &mut self,
        _: &NextThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.cycle_thread(false, window, cx);
    }

    fn on_layout_previous_thread(
        &mut self,
        _: &PreviousThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.cycle_thread(true, window, cx);
    }

    fn on_layout_focus_composer(
        &mut self,
        _: &FocusComposer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.focus_input(window, cx);
    }

    fn on_layout_focus_search(
        &mut self,
        _: &FocusSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.open_sidebar_search(window, cx);
    }

    fn on_layout_toggle_sidebar_action(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.toggle_sidebar(window, cx);
    }

    fn on_layout_toggle_mode_action(
        &mut self,
        _: &ToggleMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.toggle_mode(window, cx);
    }

    fn on_layout_toggle_output_action(
        &mut self,
        _: &ToggleOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.toggle_output(window, cx);
    }

    fn on_layout_dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.dismiss(window, cx);
    }

    fn on_layout_refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.refresh(cx);
    }

    fn on_layout_scroll_end(
        &mut self,
        _: &ScrollConversationToEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.scroll_conversation_to_end(cx);
    }

    fn on_layout_open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.open_settings(window, cx);
    }
}
