use super::*;
use gds::appearance::tokens;

use gds::ui::page::*;
use gds::{
    appearance::tokens::DataListStyle,
    ui::{ContentPage, Frame, IconButton, NavItem, button::Button},
};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::base::motion::Interpolate as _;
use gpui_kit::component::empty::{Empty as EmptyState, EmptyDescription};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, button::ButtonVariants, menu::ContextMenuExt, table::DataTable,
    tooltip::Tooltip,
};
use gpui_kit::prelude::*;

const SMALL_GAP: f32 = 4.;
const ITEM_GAP: f32 = 8.;
const CONTENT_PADDING: f32 = 24.;
pub(super) const TABLE_INSET: f32 = ContentPage::TABLE_INSET;
const TAB_HEIGHT: f32 = 44.;
const ITEM_RADIUS: f32 = 6.;
const CAPTION_PADDING: f32 = 6.;
const FILTER_WIDTH: f32 = 200.;
/// A collapsed tab row puts its close control in its own corner: a small X
/// must not widen the icon-only row.
const TAB_CORNER_CLOSE_SIZE: f32 = 16.;
const TAB_CORNER_CLOSE_ICON: f32 = 11.;
/// Space a tab row keeps for its close control at its trailing edge: the row's
/// inner padding, the button and the gap in front of it. A page-cycle hint ends
/// before that space, so the two keys never cover each other.
const TAB_CLOSE_RESERVE: f32 = ITEM_GAP * 2. + tokens::ICON_BUTTON_SIZE;
/// Height reserved by Kit's overlay scrollbar at the table's bottom edge.
pub(super) const SCROLLBAR_LANE: f32 = DataListStyle::SCROLLBAR_LANE;
const PREVIEW_WIDTH: f32 = 280.;
/// Whether the footer's left slot renders the current location scope label.
const SHOW_LOCATION_SCOPE: bool = false;

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_picker_name(window, cx);
        if std::mem::take(&mut self.toolbar.filter.sync_pending) {
            self.sync_filter_input(window, cx);
        }
        // Take the press state from the window on every frame: a release that
        // happened while a dialog held focus never reaches this view's listener,
        // and must still hide the close controls.
        self.window.held_modifier = Self::current_held_modifier(window);
        self.drain_device_removals(cx);
        self.sync_terminal_cwd(window, cx);
        if !self.pages.is_empty()
            || self.collection().is_some()
            || self.menus.application_open
            || self.menus.context.is_some()
            || self.menus.sort.is_some()
            || self.picker.is_some()
            || window.has_active_dialog(cx)
        {
            self.discard_marquee();
        }
        // Settings replaces the toolbar; every location page keeps the menu, so
        // only entering Settings drops a menu that was open over the workspace.
        if !self.pages.is_empty() {
            self.menus.application_open = false;
        }
        if !self.pages.is_empty() || self.collection().is_some() {
            self.menus.sort = None;
        }
        let state = if self.sidebar.collapsed {
            ui::sidebar::SidebarState::Collapsed
        } else {
            ui::sidebar::SidebarState::Expanded
        };
        self.sidebar.presentation =
            ui::sidebar::SideBar::default()
                .state(state)
                .sample("files-sidebar-motion", window, cx);
        let width = self.width();
        self.settings.view.update(cx, |view, cx| {
            view.set_sidebar_presentation(self.sidebar.presentation, cx)
        });
        let frame = if self.pages.current() == Some(&pages::Page::Help) {
            Frame::without_sidebar()
        } else {
            Frame::default().sidebar_width(width)
        };
        let toolbar = self.render_toolbar(&frame, window, cx);
        let body = if self.pages.current() == Some(&pages::Page::Help) {
            el! { .div { flex_1; min_h_0; } [$(crate::widgets::help::render(cx))] }
                .into_any_element()
        } else if !self.pages.is_empty() {
            el! { .div { flex_1; min_h_0 } [$(self.settings.view.clone())] }.into_any_element()
        } else {
            let opacity = motion::tab_content(self.tabs.active_id, window, cx);
            let content = self.render_content(window, cx);
            let content = ui::region::surface(
                "content-region",
                &self.window.focus,
                Command::FocusContent,
                window,
                cx,
            )
            .opacity(opacity)
            .aria_label(self.current_location().label(cx))
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(content)
            .into_any_element();
            let workspace = frame
                .workspace(self.render_sidebar(window, cx), content, cx)
                .into_any_element();
            self.terminal_workspace(workspace, window, cx)
        };
        let extras = if !self.pages.is_empty() {
            Vec::new()
        } else {
            self.render_picker_extras(self.picker_actions_available(window, cx), cx)
        };
        let extras = (!extras.is_empty()).then(|| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px_4()
                .py_2()
                .children(extras)
        });
        let footer_status = self
            .render_picker_name(self.picker_actions_available(window, cx))
            .unwrap_or_else(|| self.render_status(cx));
        let footer = frame.footer(
            self.render_location_scope(cx),
            footer_status,
            self.render_footer_controls(window, cx),
            cx,
        );
        let mut context = KeyContext::parse(if self.pages.current() == Some(&pages::Page::Help) {
            "Help"
        } else if !self.pages.is_empty() {
            SETTINGS_CONTEXT
        } else {
            WORKSPACE_CONTEXT
        })
        .expect("bundled key contexts are valid");
        if self.pages.is_empty()
            && (self.collection() == Some(Collection::Volumes)
                || self.current_tab().is_some_and(|slot| slot.device.is_some()))
        {
            context.add(VOLUME_CONTEXT);
        }
        // The picker decisions exist only on the picker page itself. Settings
        // replaces the workspace context, and a dialog owns its own context, so
        // neither can pick up `Alt+A` / `Alt+C` from here.
        if self.picker.is_some() && self.pages.is_empty() {
            context.add(picker::PICKER_CONTEXT);
        }
        let notification_overlay = notifications::overlay_for(window, cx);
        let selection_menu = self.render_selection_menu(window);
        el! {
            .div {
                # "gds-files"; key_context: $context; track_focus: $&self.window.root_focus;
                size_full; flex; flex_col; overflow_hidden;
                text_color: $cx.theme().foreground; text_size: tokens::SMALL;
                on_action: (cx, Self::on_application_menu).Context::listener;
                on_action: (cx, Self::on_focus_locations).Context::listener;
                on_action: (cx, Self::on_focus_volumes).Context::listener;
                on_action: (cx, Self::on_focus_tabs).Context::listener;
                on_action: (cx, Self::on_focus_content).Context::listener;
                on_action: (cx, Self::on_focus_notifications).Context::listener;
                on_action: (cx, Self::on_close_notification).Context::listener;
                on_action: (cx, Self::on_close_all_notifications).Context::listener;
                on_action: (cx, Self::on_settings).Context::listener;
                on_action: (cx, Self::on_help).Context::listener;
                on_action: (cx, Self::on_new_folder).Context::listener;
                on_action: (cx, Self::on_new_tab).Context::listener;
                on_action: (cx, Self::on_close_tab).Context::listener;
                on_action: (cx, Self::on_next_tab).Context::listener;
                on_action: (cx, Self::on_previous_tab).Context::listener;
                on_action: (cx, Self::on_sidebar).Context::listener;
                on_action: (cx, Self::on_terminal).Context::listener;
                on_action: (cx, Self::on_return_from_terminal).Context::listener;
                on_action: (cx, Self::on_edit_location).Context::listener;
                on_action: (cx, Self::on_back).Context::listener;
                on_action: (cx, Self::on_forward).Context::listener;
                on_action: (cx, Self::on_parent).Context::listener;
                on_action: (cx, Self::on_refresh).Context::listener;
                on_action: (cx, Self::on_filter).Context::listener;
                on_action: (cx, Self::on_hidden).Context::listener;
                on_action: (cx, Self::on_open).Context::listener;
                on_action: (cx, Self::on_open_in_new_tabs).Context::listener;
                on_action: (cx, Self::on_copy_files).Context::listener;
                on_action: (cx, Self::on_cut_files).Context::listener;
                on_action: (cx, Self::on_paste_files).Context::listener;
                on_action: (cx, Self::on_undo_files).Context::listener;
                on_action: (cx, Self::on_redo_files).Context::listener;
                on_action: (cx, Self::on_copy_paths).Context::listener;
                on_action: (cx, Self::on_select_all).Context::listener;
                on_action: (cx, Self::on_deselect_all).Context::listener;
                on_action: (cx, Self::on_selection_menu).Context::listener;
                on_action: (cx, Self::on_rename).Context::listener;
                on_action: (cx, Self::on_details).Context::listener;
                on_action: (cx, Self::on_configure_columns).Context::listener;
                on_action: (cx, Self::on_view_mode).Context::listener;
                on_action: (cx, Self::on_sort).Context::listener;
                on_action: (cx, Self::on_favorite_current_folder).Context::listener;
                on_action: (cx, Self::on_trash).Context::listener;
                on_action: (cx, Self::on_delete_permanently).Context::listener;
                on_action: (cx, Self::on_restore_selection).Context::listener;
                on_action: (cx, Self::on_purge_selection).Context::listener;
                on_action: (cx, Self::on_empty_trash).Context::listener;
                on_action: (cx, Self::on_clear_recent_action).Context::listener;
                on_action: (cx, Self::on_mount_volume).Context::listener;
                on_action: (cx, Self::on_unmount_volume).Context::listener;
                on_action: (cx, Self::on_picker_confirm).Context::listener;
                on_action: (cx, Self::on_picker_confirm_with_control).Context::listener;
                on_action: (cx, Self::on_picker_read_only).Context::listener;
                on_action: (cx, Self::on_picker_next_filter).Context::listener;
                on_action: (cx, Self::on_picker_cancel).Context::listener;






                on_action: (cx, Self::on_tab_1).Context::listener;
                on_action: (cx, Self::on_tab_2).Context::listener;
                on_action: (cx, Self::on_tab_3).Context::listener;
                on_action: (cx, Self::on_tab_4).Context::listener;
                on_action: (cx, Self::on_tab_5).Context::listener;
                on_action: (cx, Self::on_tab_6).Context::listener;
                on_action: (cx, Self::on_tab_7).Context::listener;
                on_action: (cx, Self::on_tab_8).Context::listener;
                on_action: (cx, Self::on_tab_9).Context::listener;
                capture_key_down: (cx, Self::on_region_key_down).Context::listener;
                capture_key_down: (cx, Self::on_file_key_down).Context::listener;
                capture_key_down: (cx, Self::on_trash_key_down).Context::listener;
                capture_key_down: (cx, Self::on_place_key_down).Context::listener;
                capture_key_down: (cx, Self::on_volume_key_down).Context::listener;
                on_action: (cx, Self::on_select_up).Context::listener;
                on_action: (cx, Self::on_select_down).Context::listener;
                on_action: (cx, Self::on_select_first).Context::listener;
                on_action: (cx, Self::on_select_last).Context::listener;
                on_mouse_down: $(MouseButton::Navigate(NavigationDirection::Back)), (cx, Self::on_mouse_back).Context::listener;
                on_mouse_down: $(MouseButton::Navigate(NavigationDirection::Forward)), (cx, Self::on_mouse_forward).Context::listener;
                on_action: (cx, Self::on_cancel).Context::listener;

                on_modifiers_changed: (cx, Self::on_modifiers_changed).Context::listener;
            } [toolbar, body, ..extras, footer,
                ..selection_menu,
                @if (let Some(overlay) = notification_overlay)
                    overlay,
                @end,
            ]
        }
    }
}

impl Browser {
    fn command_button(
        &self,
        id: &'static str,
        icon: IconName,
        command: Command,
        cx: &mut Context<Self>,
    ) -> IconButton {
        let label = i18n::text(command.label_id(), cx);
        let on_click = cx.listener(Self::click_command(command));
        el! { id.IconButton::new { icon: icon; label: label; command: command; on_click: on_click } }
    }

    fn render_toolbar(&self, frame: &Frame, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // Settings owns its return control; location navigation stays in the workspace.
        let secondary = !self.pages.is_empty();
        let menu = if secondary {
            let on_click = cx.listener(Self::click_command(Command::Cancel));
            el! {
                "return-to-files".IconButton::new {
                    icon: IconName::ArrowLeft; label: $i18n::text("page-return", cx);
                    command: Command::Cancel; on_click: on_click;
                }
            }
            .into_any_element()
        } else {
            menu::application_menu(self.menus.application_open, cx.entity().downgrade(), cx)
        };
        let sidebar = self.command_button("sidebar", IconName::PanelLeft, Command::Sidebar, cx);
        let content = if !self.pages.is_empty() {
            el! { .div { flex_1; px: ITEM_GAP.px } [$i18n::text(if self.pages.current() == Some(&pages::Page::Help) { "help" } else { "settings" }, cx)] }
                .into_any_element()
        } else {
            self.render_navigation(window, cx)
        };
        let controls = el! {
            .div { flex; items_center; gap: SMALL_GAP.px; w_full; min_w_0 } [@if (self.pages.current() != Some(&pages::Page::Help)) sidebar, @end, content]
        };
        let window_controls = ui::window::WindowControls::default()
            .on_close_window(cx.listener(Self::on_close_window));
        frame.toolbar_with_window_controls(menu, controls.into_any_element(), window_controls, cx)
    }

    fn render_navigation(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let has_parent = matches!(self.current_location(), Location::Remote(ref address) if address.parent().is_some())
            || self
                .active()
                .and_then(OpenTab::path)
                .and_then(Path::parent)
                .is_some();
        let back = self.history_button(NavigationDirection::Back, cx);
        let forward = self.history_button(NavigationDirection::Forward, cx);
        let parent = self
            .command_button("parent", IconName::ArrowUp, Command::Parent, cx)
            .disabled(!has_parent);
        let path = self.render_path(window, cx);
        el! {
            .div { flex; flex_1; min_w_0; items_center; gap: SMALL_GAP.px } [back, forward, parent, path]
        }.into_any_element()
    }

    fn render_path(&self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.toolbar.path.editing {
            return div()
                .flex_1()
                .min_w_0()
                .block_mouse_except_scroll()
                .child(ui::field::input(&self.toolbar.path.input, cx))
                .into_any_element();
        }
        if let Some(location) = self
            .current_tab()
            .and_then(|s| s.navigation.pending_location())
        {
            let destination = match location {
                Location::Remote(a) => a.to_string(),
                Location::Directory(p) => Path::new(p).display().to_string(),
                _ => location.label(cx).to_string(),
            };
            return div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(destination)
                .into_any_element();
        }
        let content = if let Location::Remote(address) = self.current_location() {
            el! { .div { flex_1; min_w_0; px: CAPTION_PADDING.px; truncate } [$(address.to_string())] }
                .into_any_element()
        } else if self.current_path().is_none() {
            el! { .div { flex_1; min_w_0; px: CAPTION_PADDING.px; truncate } [$(self.current_location().display_address())] }
                .into_any_element()
        } else {
            let mut crumbs = Vec::new();
            for (index, segment) in self
                .current_path()
                .map(|path| breadcrumb::segments(&path))
                .unwrap_or_default()
                .into_iter()
                .enumerate()
            {
                let label: SharedString = segment.label.into();
                let color = if segment.current {
                    cx.theme().foreground
                } else {
                    cx.theme().muted_foreground
                };
                let on_click = cx.listener(Self::click_path(segment.path));
                crumbs.push(
                    el! {
                        ($("crumb", index)).Button::new { ghost; small; flex_shrink_0;
                            h: tokens::ICON_BUTTON_SIZE.px; px_1; text_color: color;
                            label: label; on_click: on_click;
                        }
                    }
                    .into_any_element(),
                );
            }
            el! {
                .div { # "breadcrumbs"; flex_1; min_w_0; flex; items_center; gap: SMALL_GAP.px; overflow_x_scroll } [..crumbs]
            }.into_any_element()
        };
        let edit = self.command_button("edit-path", IconName::Pencil, Command::EditLocation, cx);
        el! {
            .div {
                # "location-display";
                flex_1; min_w_0; flex; items_center; mx: CAPTION_PADDING.px;
                rounded: ITEM_RADIUS.px; border_1; border_color: $cx.theme().border;
            } [
                content,
                edit,
            ]
        }
        .into_any_element()
    }

    pub(super) fn render_tab(
        &self,
        slot: &OpenTab,
        position: usize,
        cycle_targets: (Option<u64>, Option<u64>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = slot.id;
        let presentation = self.sidebar.presentation;
        let compact = presentation.progress() == 0.;
        let row_height = gds::appearance::typography::scaled(TAB_HEIGHT, cx);
        let selected = id == self.tabs.active_id;
        let selection =
            motion::state_progress(format!("files-tab-selection-{id}"), selected, window, cx);
        let cursor = ui::region::is_focus_visible(&self.sidebar.regions.tabs, window)
            && self.sidebar.regions.tab == Some(id);
        let title: SharedString = self.tab_title(slot, cx).into();
        let path: SharedString = slot
            .path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| slot.location().label(cx))
            .into();
        let icon = Icon::new(slot.navigation.location().icon()).size(px(tokens::SIDEBAR_ICON_SIZE));
        let on_click = cx.listener(Self::click_tab(id));
        let on_hover = cx.listener(Self::hover_tab(id));
        let menu = Self::tab_menu(cx.entity().downgrade(), id, slot.pinned);
        // Pinned tabs keep their unpin control in the ordinary close slot.
        let action_icon = if slot.pinned {
            IconName::Pin
        } else {
            IconName::X
        };
        let action_label = i18n::text(
            if slot.pinned {
                "unpin-tab"
            } else {
                "close-this-tab"
            },
            cx,
        );
        let action_id = if slot.pinned {
            "unpin-tab"
        } else {
            "close-tab"
        };
        let corner_id = if slot.pinned {
            "unpin-tab-corner"
        } else {
            "close-tab-corner"
        };
        let show_action =
            (slot.pinned || self.revealing_tab_close() || self.tabs.hovered == Some(id))
                && (presentation.actions_visible() || compact);
        // A row with a pending press loses on_hover on mouse movement. Keep
        // the action button's press off that row until its click completes.
        let on_close_down = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
            cx.stop_propagation();
        };
        let action = match (show_action, compact) {
            (true, false) => {
                let on_close = cx.listener(Self::click_tab_action(id, slot.pinned));
                Some(
                    el! {
                        .div { # $(if slot.pinned { "unpin-tab-press" } else { "close-tab-press" }, id); absolute; right: ITEM_GAP.px;
                            on_mouse_down: MouseButton::Left, on_close_down;
                        } [
                            ($(action_id, id)).IconButton::new {
                                icon: action_icon; label: action_label;
                                @if (selected && !slot.pinned) command: Command::CloseTab; @end;
                                on_click: on_close;
                            },
                        ]
                    }
                    .into_any_element(),
                )
            }
            // A collapsed row has no free trailing space: the action sits in its own
            // top trailing corner at icon size instead of widening the row.
            (true, true) => {
                let on_close = cx.listener(Self::click_tab_action(id, slot.pinned));
                Some(
                    el! {
                        .div {
                            # $(action_id, id); absolute; top: 1.0.px; right: 1.0.px;
                            on_mouse_down: MouseButton::Left, on_close_down;
                        } [
                            ($(corner_id, id)).Button::new {
                                ghost; small; size: TAB_CORNER_CLOSE_SIZE.px;
                                icon: action_icon.Icon::new { size: TAB_CORNER_CLOSE_ICON.px };
                                accessibility_label: $action_label.clone();
                                tooltip: action_label;
                                on_click: on_close;
                            },
                        ]
                    }
                    .into_any_element(),
                )
            }
            (false, _) => None,
        };
        let mut hints =
            self.render_tab_cycle_hints(id, cycle_targets, compact, show_action, window, cx);
        hints.extend(self.render_tab_ordinal_hint(id, position, compact, show_action, window, cx));
        let muted = cx.theme().muted_foreground;
        let tooltip = Self::tab_tooltip(title.clone(), path.clone(), muted);
        let tab = el! {
            .div {
                # $("tab", id); relative; flex; items_center; gap: ITEM_GAP.px; w_full; flex_shrink_0;
                h: row_height; min_h: row_height;
                overflow_hidden;
                pl: $(px(presentation.interpolate(ITEM_GAP, (tokens::COLLAPSED_SIDEBAR_WIDTH - 2. * tokens::SIDEBAR_PADDING - 1. - tokens::SIDEBAR_ICON_SIZE) / 2.)));
                pr: ITEM_GAP.px; rounded: ITEM_RADIUS.px; cursor_pointer;
                bg: $(cx.theme().sidebar_accent.opacity(selection));
                text_color: $(cx.theme().sidebar_foreground.interpolate(&cx.theme().sidebar_accent_foreground, selection));
                @if (!selected && selection == 0.) hover: { bg: $(gds::appearance::theme::Colors::current(cx).sidebar_hover()) }; @end;
                on_click: on_click;
                on_hover: on_hover;
                @if (!self.revealing_tab_close()) tooltip_show_delay: tokens::TOOLTIP_DELAY; tooltip: tooltip; @end;
            } [
                @if (cursor) $(ui::region::focus_ring(cx)), @end,
                .div { # $("tab-icon", id); test_support; flex_shrink_0; w: tokens::SIDEBAR_ICON_SIZE.px; } [icon],
                .div { flex_1; min_w_0; flex; flex_col; overflow_hidden; opacity: $(presentation.label_opacity()); } [
                    .div { pr: $(px(tokens::ICON_BUTTON_SIZE + ITEM_GAP)); text_ellipsis } [title],
                    .div { pr: $(px(tokens::ICON_BUTTON_SIZE + ITEM_GAP)); text_xs; text_color: $(cx.theme().muted_foreground.interpolate(&cx.theme().sidebar_accent_foreground, selection)); text_ellipsis } [path],
                ],
                ..action,
                ..hints,
            ]
        };
        let tab = if let Some(path) = slot.path() {
            drag_drop::tab_drop_target(
                tab,
                path.to_owned(),
                id,
                cx.entity().downgrade(),
                window,
                cx,
            )
        } else {
            tab
        };
        el! { tab { test_support; context_menu: menu; into_any_element } }
    }

    /// A row's folder name and its whole path.
    fn tab_tooltip(
        title: SharedString,
        path: SharedString,
        muted: Hsla,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView {
        move |window, cx| {
            let title = title.clone();
            let path = path.clone();
            Tooltip::element(move |_, _| {
                el! {
                    .div { flex; flex_col; gap: 2.0.px; max_w: 420.0.px } [
                        .div { font_weight: FontWeight::SEMIBOLD; whitespace_normal } [$title.clone()],
                        .div { text_xs; text_color: muted; whitespace_normal } [$path.clone()],
                    ]
                }
            })
            .build(window, cx)
        }
    }

    /// The page-cycle keys staged on this tab row.
    ///
    /// A row carries a label only when a cycle key really lands on it: the same
    /// target [`Browser::cycle_tab`] uses, so the two cannot disagree.
    fn render_tab_cycle_hints(
        &self,
        id: u64,
        cycle_targets: (Option<u64>, Option<u64>),
        compact: bool,
        show_action: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        let (previous, next) = cycle_targets;
        let mut hints = Vec::new();
        for (backwards, target) in [(true, previous), (false, next)] {
            if target != Some(id) {
                continue;
            }
            let (key, command) = if backwards {
                ("tab-previous-hint", Command::PreviousTab)
            } else {
                ("tab-next-hint", Command::NextTab)
            };
            let key: ElementId = (key, id).into();
            hints.push(Self::stage_tab_hint(
                key,
                ui::Shortcut::from(command),
                compact,
                show_action,
                window,
                cx,
            ));
        }
        hints
    }

    /// The ordinal key staged on this tab row.
    ///
    /// The ordinal is the row's position in the strip, pinned tabs included;
    /// rows past the ninth have no key, so they stage nothing.
    fn render_tab_ordinal_hint(
        &self,
        id: u64,
        position: usize,
        compact: bool,
        show_action: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let shortcut = ordinal_shortcut(position)?;
        Some(Self::stage_tab_hint(
            ("tab-ordinal-hint", id).into(),
            shortcut,
            compact,
            show_action,
            window,
            cx,
        ))
    }

    /// Stage a tab row's label where the row has room for it.
    ///
    /// A collapsed row has no trailing space and keeps its close control in its
    /// top trailing corner, so its label takes the free bottom leading corner
    /// rather than being dropped with the space it would have used.
    fn stage_tab_hint(
        key: ElementId,
        shortcut: ui::Shortcut,
        compact: bool,
        show_action: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        match (compact, show_action) {
            (true, true) => ui::keytips::bottom_corner_anchor(window, cx, key, shortcut),
            (true, false) => ui::keytips::anchor(window, cx, key, shortcut),
            (false, _) => ui::keytips::trailing_before_anchor(
                window,
                cx,
                key,
                shortcut,
                px(TAB_CLOSE_RESERVE),
            ),
        }
    }

    pub(super) fn render_location(
        &self,
        target: LocationTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let on_click = cx.listener(Self::click_location(target));
        let id = target.label_id();
        let icon = target.icon();
        let label = i18n::text(id, cx);
        let nav = el! {
            (id, icon, label).NavItem::new {
                presentation: $self.sidebar.presentation;
                selected: $self.location_selected(target);
                on_click: on_click;
            }
        };
        let cursor = ui::region::is_focus_visible(&self.sidebar.regions.locations, window)
            && self.sidebar.regions.location == target;
        el! { .div { relative; w_full } [nav,
            @if (cursor) $(ui::region::focus_ring(cx)), @end,
        ] }
        .into_any_element()
    }

    /// The footer's far-left slot reports the current location scope.
    ///
    /// It is hidden for now. The slot and the footer geometry stay in place so
    /// no other control moves; only this label is suppressed. Bringing it back
    /// is a source change, not a user setting.
    fn render_location_scope(&self, cx: &App) -> AnyElement {
        if !SHOW_LOCATION_SCOPE {
            return el! { .div { # "location-scope"; size_full } }.into_any_element();
        }
        el! {
            .div {
                # "location-scope"; size_full; flex; items_center;
                text_xs; text_color: $cx.theme().muted_foreground;
                @if (self.sidebar.collapsed) justify_center; @else px: CONTENT_PADDING.px; @end;
            } [
                @if (self.pages.is_empty()) $i18n::text("local", cx), @end,
            ]
        }
        .into_any_element()
    }

    fn directory_page(
        &self,
        body: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ContentPage {
        let title: SharedString = if self.current_location() == Location::from(&self.locations.home)
        {
            // Use the localized name for the home directory.
            i18n::text("home", cx)
        } else {
            self.current_tab()
                .map(|slot| self.tab_title(slot, cx))
                .unwrap_or_default()
                .into()
        };
        let favorite = self
            .locations
            .places
            .is_favorite(self.current_place_location());
        let on_favorite = cx.listener(Self::on_favorite);
        let hidden = self
            .command_button("hidden", IconName::Eye, Command::Hidden, cx)
            .active(self.settings.preferences.files.show_hidden);
        let refresh = self.command_button("refresh", IconName::RefreshCw, Command::Refresh, cx);
        let details = self
            .command_button("details", IconName::Info, Command::Details, cx)
            .active(self.details_visible());
        let on_columns = cx.listener(Self::on_columns);
        // The filter box itself is the real target of its binding.
        let filter_hint = ui::keytips::row_anchor(
            window,
            cx,
            "filter-hint".into(),
            ui::Shortcut::from(Command::Filter),
        );
        let filter = el! {
            .div { relative; w: FILTER_WIDTH.px; min_w: 96.0.px; flex_shrink_1 } [
                ($&self.toolbar.filter.input, cx).ui::field::input { small; cleanable: true; prefix: IconName::Search.Icon::new },
                filter_hint,
            ]
        };
        let favorite = el! {
            "favorite".IconButton::new {
                icon: IconName::Star;
                label: $i18n::text(if favorite { "favorite-remove" } else { "favorite-add" }, cx);
                active: favorite;
                command: Command::Favorite;
                on_click: on_favorite;
            }
        };
        let columns = el! {
            "columns".IconButton::new {
                icon: IconName::Columns3Cog; label: $i18n::text("columns-configure", cx);
                command: Command::Columns;
                disabled: $(!self.columns_available() || self.grid_mode()); on_click: on_columns;
            }
        };
        let sort = self.sort_button(cx);
        let view_mode = self
            .command_button(
                "view-mode",
                if self.grid_mode() {
                    IconName::List
                } else {
                    IconName::LayoutGrid
                },
                Command::ViewMode,
                cx,
            )
            .disabled(!self.folder_settled())
            .label(i18n::text(
                if self.grid_mode() {
                    "view-list"
                } else {
                    "view-grid"
                },
                cx,
            ));
        el! {
            .ContentPage::default { # "directory-page" } [
                .PageHeader::default [
                    .PageTitle::default [title],
                    .PageActions::default { min_w_0; flex_shrink_1; } [filter, view_mode, favorite, sort, columns, hidden, details, refresh],
                ],
                .PageContent::default [body],
            ]
        }
    }

    fn render_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(location) = self
            .current_tab()
            .and_then(|s| s.navigation.pending_location())
            .cloned()
        {
            return self.render_location_loading(&location, cx);
        }
        if matches!(self.current_location(), Location::Remote(_)) {
            return self.render_remote(window, cx);
        }
        if let Some(collection) = self.collection() {
            return self.render_collection(collection, window, cx);
        }
        if self
            .current_tab()
            .is_some_and(|slot| !slot.files_available())
        {
            return self.render_device_unavailable(cx);
        }
        let sort = super::columns::sort_for(&self.current_location(), &self.locations.places);
        if let Some(slot) = self.active_mut()
            && slot.sort != sort
        {
            slot.sort = sort;
            slot.refresh_visible();
        }
        self.sync_preview(cx);
        let details_progress =
            motion::state_progress("files-details", self.details_visible(), window, cx);
        let width = f32::from(window.bounds().size.width)
            - self.width()
            - TABLE_INSET * 2.
            - PREVIEW_WIDTH * details_progress;
        let mode = self.view_mode();
        let font_scale = f32::from(gds::appearance::typography::scaled(1., cx));
        if let Some(slot) = self.active_mut() {
            let geometry = grid::Geometry::new(width, font_scale);
            let reflow = slot.view_mode != mode
                || slot.grid.columns != geometry.columns
                || slot.grid.row_height != geometry.row_height;
            slot.view_mode = mode;
            slot.grid = geometry;
            if reflow
                && let Some(index) = slot
                    .cursor
                    .as_ref()
                    .and_then(|path| slot.table_rows.iter().position(|entry| &entry.path == path))
            {
                slot.scroll_to_entry(index);
            }
        }
        let list = self.render_file_table(window, cx);
        let Some(path) = self.current_path() else {
            return el! { .div }.into_any_element();
        };
        let menu = Self::directory_menu_builder(cx.entity().downgrade(), path);
        let list_area = PageTableArea::default()
            .id("file-list-area")
            .child(list)
            .context_menu(menu)
            .into_any_element();
        let rows = self.active().map_or(0, |slot| slot.table_rows.len());
        let scroll = self
            .active()
            .map(|slot| slot.scroll.0.borrow().base_handle.clone())
            .unwrap_or_default();
        let list_area = if self.grid_mode() {
            self.grid_surface(list_area, cx)
        } else {
            ui::ListSurface::new("directory-selection-surface", list_area, rows, scroll)
                .inset_x(px(TABLE_INSET))
                .header_height(px(if rows == 0 {
                    0.
                } else {
                    DataListStyle::HEADER_HEIGHT
                }))
                .on_blank_press(cx.listener(Self::on_list_background_press))
                .into_any_element()
        };
        // A picker holds one path at a time, so its list has no rubber-band
        // surface: a batch selection could neither be reported nor kept.
        let list_area = if self.picker.is_some() {
            el! {
                .div { # "file-list-plain"; flex; flex_col; flex_1; min_h_0; min_w_0; overflow_hidden } [list_area]
            }
            .into_any_element()
        } else {
            self.marquee_surface(list_area.into_any_element(), window, cx)
        };
        let content = self.directory_page(list_area, window, cx);
        let panel = (details_progress > 0.).then(|| {
            el! {
                .div {
                    # "preview-panel"; w: $(PREVIEW_WIDTH * details_progress).px; flex_shrink_0; h_full; min_h_0;
                    flex; flex_col; overflow_hidden; border_l_1; border_color: $cx.theme().border;
                } [
                    .div { w: PREVIEW_WIDTH.px; flex_1; min_h_0; flex_shrink_0; flex; flex_col; overflow_hidden } [$(self.files.preview.clone())],
                ]
            }
        });
        // The workspace content column is not a flex parent, so the row has to fill
        // the available height itself for the list area to receive any space.
        el! {
            .div { flex; size_full; min_h_0; min_w_0 } [content, ..panel]
        }
        .into_any_element()
    }

    fn render_file_table(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(slot) = self
            .active()
            .filter(|slot| slot.navigation.directory().is_some())
        else {
            let loading = self.active().is_some_and(OpenTab::is_loading);
            return self
                .empty_file_list(
                    i18n::text(if loading { "loading" } else { "load-failed" }, cx),
                    cx,
                )
                .into_any_element();
        };
        let navigation = &slot.navigation;
        if slot.visible.is_empty() {
            let message = if !slot.filter.is_empty() {
                "empty-filter"
            } else if slot
                .picker
                .as_ref()
                .is_some_and(picker::Projection::narrows)
            {
                "picker-empty-files"
            } else {
                "empty-directory"
            };
            return self
                .empty_file_list(i18n::text(message, cx), cx)
                .into_any_element();
        }
        if self.grid_mode() {
            return self.render_file_grid(cx);
        }
        let data = table::Data {
            rows: slot.table_rows.clone(),
            selected: navigation.view().selected.clone(),
            sort: slot.sort,
            scroll: slot.scroll.clone(),
            content_width: f32::from(window.bounds().size.width)
                - self.width()
                - TABLE_INSET * 2.
                - PREVIEW_WIDTH
                    * motion::state_progress("files-details", self.details_visible(), window, cx),
            layout: self.columns_layout(&self.current_location()).clone(),
            location: self.current_location(),
            widths: self.locations.places.column_widths(self.current_location()),
        };
        table::FileTable::sync_state(&self.files.table, data, cx);
        let height =
            DataListStyle::HEADER_HEIGHT + DataListStyle::ROW_HEIGHT * slot.visible.len() as f32;
        let table = el! {
            ($&self.files.table).DataTable::new {
                bordered: false;
                with_size: $(gpui_kit::component::Size::Size(px(DataListStyle::ROW_HEIGHT)));
                header_row_height: DataListStyle::HEADER_HEIGHT.px;
                into_any_element;
            }
        };
        let empty_space = el! {
            .div {
                # "directory-empty-space"; absolute; left_0; right_0;
                top: height.px; bottom: SCROLLBAR_LANE.px;
            }
        };
        let Some(path) = self.current_path() else {
            return empty_space.into_any_element();
        };
        let empty_menu = Self::directory_menu_builder(cx.entity().downgrade(), path.clone());
        let empty_space = drag_drop::drop_target(empty_space, path, cx.entity().downgrade())
            .context_menu(empty_menu);
        // The table owns the full content viewport so its horizontal scrollbar
        // stays at the bottom instead of riding under the last short row. The
        // blank area below the rows remains a separate target, inset by the
        // scrollbar lane so the scrollbar itself stays draggable.
        el! {
            .div { # "file-table-area"; key_context: FILE_TABLE_CONTEXT; relative; flex; flex_col; size_full; min_h_0 } [
                .div { flex_1; min_h_0; min_w_0 } [table],
                empty_space,
            ]
        }
        .into_any_element()
    }

    fn empty_file_list(&self, message: SharedString, cx: &mut Context<Self>) -> Stateful<Div> {
        let empty = el! {
            .div { # "empty-file-list"; size_full; flex; items_center; justify_center;
            } [.EmptyState::default { p_0; size_full } [.EmptyDescription::default { text_color: $(cx.theme().foreground) } [message]]]
        };
        if let Some(path) = self.current_path() {
            drag_drop::drop_target(empty, path, cx.entity().downgrade())
        } else {
            empty
        }
    }

    fn render_collection(
        &self,
        collection: Collection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Trash and Network are virtual locations; they never borrow a
        // filesystem path from the active tab.
        match collection {
            Collection::Volumes => self.render_devices_page(window, cx),
            Collection::Trash => self.render_trash(window, cx),
            Collection::Recent | Collection::Network | Collection::Favorites => {
                self.render_places_page(collection, window, cx)
            }
        }
    }

    /// Window-level toggles live in the footer, never duplicated in the toolbar.
    ///
    /// A picker replaces the terminal toggle with its own decisions, so its
    /// filter group, read-only mark, cancel and confirm sit where the terminal
    /// control sits in a browser window; leaving picker mode restores it.
    fn render_footer_controls(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.pages.is_empty() {
            return el! { .div }.into_any_element();
        }
        if self.picker.is_some() {
            let picker = self.render_picker_controls(window, cx);
            return picker;
        }
        if !self.settings.preferences.terminal_enabled() {
            return el! { .div }.into_any_element();
        }
        let terminal = self
            .command_button("terminal", IconName::Terminal, Command::Terminal, cx)
            .label(i18n::text(
                if self.terminal_visible() {
                    "files-terminal-hide"
                } else {
                    "files-terminal-show"
                },
                cx,
            ))
            .toggled(self.terminal_visible())
            .disabled(self.active().is_none());
        el! { .div { flex; items_center; flex_shrink_0; gap_2; } [terminal] }.into_any_element()
    }

    fn render_status(&self, cx: &App) -> AnyElement {
        let mut labels = Vec::new();
        if self.pages.current() == Some(&pages::Page::Help) {
            return el! { .div }.into_any_element();
        }
        if !self.pages.is_empty() {
            labels.push(i18n::text("settings-auto-apply", cx));
        } else if self
            .current_tab()
            .is_some_and(|s| s.navigation.is_loading())
        {
            labels.push(i18n::text("loading", cx));
        } else if let Some(view) = self.remote_view() {
            let (visible, selected) = view.read(cx).counts();
            labels.push(i18n::count("item-count", visible, cx));
            if selected > 0 {
                labels.push(i18n::count("selection-count", selected, cx));
            }
        } else if self.collection() == Some(Collection::Volumes) {
            let count = self
                .devices
                .service
                .read(cx)
                .snapshot
                .volumes
                .iter()
                .filter(|volume| volume.visible)
                .count();
            labels.push(i18n::count("volume-count", count, cx));
        } else if self.collection() == Some(Collection::Trash) {
            if self.collections.trash.is_loading() {
                labels.push(i18n::text("recycle-loading", cx));
            }
            labels.push(i18n::count(
                "item-count",
                self.collections.trash.rows().len(),
                cx,
            ));
            let selected = self.collections.trash.selected_count();
            if selected > 0 {
                labels.push(i18n::count("selection-count", selected, cx));
            }
        } else if let Some(slot) = self.active().filter(|slot| slot.files_available())
            && let Some(directory) = slot.navigation.directory()
        {
            if slot.is_loading() {
                labels.push(i18n::text("loading", cx));
            }
            if slot.filter.is_empty() {
                if slot
                    .picker
                    .as_ref()
                    .is_some_and(picker::Projection::narrows)
                {
                    let mut args = fluent_bundle::FluentArgs::new();
                    args.set("visible", slot.visible.len() as i64);
                    args.set("total", directory.entries.len() as i64);
                    labels.push(i18n::format("filter-count", Some(&args), cx));
                } else {
                    labels.push(i18n::count("item-count", directory.entries.len(), cx));
                }
            } else {
                let mut args = fluent_bundle::FluentArgs::new();
                args.set("visible", slot.visible.len() as i64);
                args.set("total", directory.entries.len() as i64);
                labels.push(i18n::format("filter-count", Some(&args), cx));
            }
            let selected = slot.navigation.view().selected.len();
            if selected > 0 {
                labels.push(i18n::count("selection-count", selected, cx));
            }
        }
        let labels = labels
            .into_iter()
            .map(|label| el! { .div { flex_shrink_0; } [label] });
        el! {
            .div { w_full; flex; items_center; gap: TABLE_INSET.px; min_w_0; text_xs; text_color: $cx.theme().muted_foreground } [
                ..labels,
                @if (let Some(clipboard) = clipboard_status::render(cx))
                    clipboard,
                @end,
            ]
        }.into_any_element()
    }
}

impl LocationTarget {
    pub(super) const ALL: [Self; 6] = [
        Self::Home,
        Self::Recent,
        Self::Favorites,
        Self::Volumes,
        Self::Network,
        Self::Trash,
    ];
    fn label_id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Recent => "recent",
            Self::Favorites => "favorites",
            Self::Volumes => "volumes",
            Self::Network => "network",
            Self::Trash => "trash",
        }
    }
    fn icon(self) -> IconName {
        match self {
            Self::Home => IconName::House,
            Self::Recent => IconName::Clock,
            Self::Favorites => IconName::Star,
            Self::Volumes => IconName::HardDrive,
            Self::Network => IconName::Network,
            Self::Trash => IconName::Trash,
        }
    }
}

impl Collection {
    pub(super) fn label_id(self) -> &'static str {
        match self {
            Self::Volumes => "volumes",
            Self::Recent => "recent",
            Self::Favorites => "favorites",
            Self::Network => "network",
            Self::Trash => "trash",
        }
    }
}
