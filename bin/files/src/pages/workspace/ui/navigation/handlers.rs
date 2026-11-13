use super::*;
use gds_uim::el;
use gpui_kit::base::actions::{SelectDown, SelectFirst, SelectLast, SelectUp};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

impl Browser {
    pub(in crate::pages::workspace) fn dispatch_action<A: Action>(
        command: Command,
    ) -> impl Fn(&mut Self, &A, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| this.dispatch(command, window, cx)
    }

    pub(in crate::pages::workspace) fn navigate_page_action<A: Action>(
        motion: selection::Motion,
    ) -> impl Fn(&mut Self, &A, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| this.navigate_visible_page(motion, window, cx)
    }

    pub(in crate::pages::workspace) fn bind_workspace_inputs(
        div: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div.on_action(cx.listener(Self::dispatch_action::<ApplicationMenu>(Command::Menu)))
            .on_action(cx.listener(Self::dispatch_action::<FocusLocations>(
                Command::FocusLocations,
            )))
            .on_action(cx.listener(Self::dispatch_action::<FocusVolumes>(Command::FocusVolumes)))
            .on_action(cx.listener(Self::dispatch_action::<FocusTabs>(Command::FocusTabs)))
            .on_action(cx.listener(Self::dispatch_action::<FocusContent>(Command::FocusContent)))
            .on_action(cx.listener(Self::dispatch_action::<FocusNotifications>(
                Command::FocusNotifications,
            )))
            .on_action(cx.listener(Self::dispatch_action::<CloseNotification>(
                Command::CloseNotification,
            )))
            .on_action(cx.listener(Self::dispatch_action::<CloseAllNotifications>(
                Command::CloseAllNotifications,
            )))
            .on_action(cx.listener(Self::dispatch_action::<OpenSettings>(Command::Settings)))
            .on_action(
                cx.listener(Self::dispatch_action::<gds::ui::help::OpenHelp>(
                    Command::Help,
                )),
            )
            .on_action(cx.listener(Self::dispatch_action::<NewFolder>(Command::NewFolder)))
            .on_action(cx.listener(Self::dispatch_action::<NewTab>(Command::NewTab)))
            .on_action(cx.listener(Self::dispatch_action::<NewWindow>(Command::NewWindow)))
            .on_action(cx.listener(Self::dispatch_action::<CloseTab>(Command::CloseTab)))
            .on_action(cx.listener(Self::dispatch_action::<NextTab>(Command::NextTab)))
            .on_action(cx.listener(Self::dispatch_action::<PreviousTab>(Command::PreviousTab)))
            .on_action(cx.listener(Self::dispatch_action::<ToggleSidebar>(Command::Sidebar)))
            .on_action(cx.listener(Self::dispatch_action::<ToggleTerminal>(Command::Terminal)))
            .on_action(cx.listener(Self::on_return_from_terminal))
            .on_action(cx.listener(Self::dispatch_action::<EditLocation>(Command::EditLocation)))
            .on_action(cx.listener(Self::dispatch_action::<NavigateBack>(Command::Back)))
            .on_action(cx.listener(Self::dispatch_action::<NavigateForward>(Command::Forward)))
            .on_action(cx.listener(Self::dispatch_action::<NavigateParent>(Command::Parent)))
            .on_action(cx.listener(Self::dispatch_action::<Refresh>(Command::Refresh)))
            .on_action(cx.listener(Self::dispatch_action::<FilterNames>(Command::Filter)))
            .on_action(cx.listener(Self::dispatch_action::<ToggleHidden>(Command::Hidden)))
            .on_action(cx.listener(Self::dispatch_action::<OpenSelection>(Command::Open)))
            .on_action(cx.listener(Self::dispatch_action::<OpenSelectionInNewTabs>(
                Command::OpenInNewTab,
            )))
            .on_action(cx.listener(Self::dispatch_action::<CopyFiles>(Command::Copy)))
            .on_action(cx.listener(Self::dispatch_action::<CutFiles>(Command::Cut)))
            .on_action(cx.listener(Self::dispatch_action::<PasteFiles>(Command::Paste)))
            .on_action(cx.listener(Self::dispatch_action::<UndoFileOperation>(Command::Undo)))
            .on_action(cx.listener(Self::dispatch_action::<RedoFileOperation>(Command::Redo)))
            .on_action(cx.listener(Self::dispatch_action::<CopyFilePaths>(Command::CopyPaths)))
            .on_action(cx.listener(Self::dispatch_action::<SelectAllFiles>(Command::SelectAll)))
            .on_action(cx.listener(Self::dispatch_action::<DeselectAll>(Command::DeselectAll)))
            .on_action(cx.listener(Self::dispatch_action::<SelectionMenu>(Command::ContextMenu)))
            .on_action(cx.listener(Self::dispatch_action::<RenameSelection>(Command::Rename)))
            .on_action(cx.listener(Self::dispatch_action::<ToggleDetails>(Command::Details)))
            .on_action(cx.listener(Self::dispatch_action::<ConfigureColumns>(Command::Columns)))
            .on_action(cx.listener(Self::dispatch_action::<ToggleViewMode>(Command::ViewMode)))
            .on_action(cx.listener(Self::dispatch_action::<SortFiles>(Command::Sort)))
            .on_action(cx.listener(Self::dispatch_action::<FavoriteCurrentFolder>(
                Command::Favorite,
            )))
            .on_action(cx.listener(Self::dispatch_action::<TrashSelection>(Command::Trash)))
            .on_action(
                cx.listener(Self::dispatch_action::<DeleteSelectionPermanently>(
                    Command::DeletePermanent,
                )),
            )
            .on_action(cx.listener(Self::dispatch_action::<RestoreSelection>(
                Command::RestoreSelection,
            )))
            .on_action(cx.listener(Self::dispatch_action::<PurgeSelection>(
                Command::PurgeSelection,
            )))
            .on_action(cx.listener(Self::dispatch_action::<EmptyTrash>(Command::EmptyTrash)))
            .on_action(cx.listener(Self::dispatch_action::<ClearRecent>(Command::ClearRecent)))
            .on_action(cx.listener(Self::dispatch_action::<MountVolume>(Command::MountVolume)))
            .on_action(cx.listener(Self::dispatch_action::<UnmountVolume>(
                Command::UnmountVolume,
            )))
            .on_action(cx.listener(Self::on_picker_confirm))
            .on_action(cx.listener(Self::on_picker_confirm_with_control))
            .on_action(cx.listener(Self::on_picker_read_only))
            .on_action(cx.listener(Self::on_picker_next_filter))
            .on_action(cx.listener(Self::on_picker_cancel))
            .on_action(cx.listener(Self::on_select_tab))
            .capture_key_down(cx.listener(Self::on_region_key_down))
            .capture_key_down(cx.listener(Self::on_file_key_down))
            .capture_key_down(cx.listener(Self::on_trash_key_down))
            .capture_key_down(cx.listener(Self::on_place_key_down))
            .capture_key_down(cx.listener(Self::on_volume_key_down))
            .on_action(cx.listener(Self::navigate_page_action::<SelectUp>(
                selection::Motion::Previous,
            )))
            .on_action(cx.listener(Self::navigate_page_action::<SelectDown>(
                selection::Motion::Next,
            )))
            .on_action(cx.listener(Self::navigate_page_action::<SelectFirst>(
                selection::Motion::First,
            )))
            .on_action(cx.listener(Self::navigate_page_action::<SelectLast>(
                selection::Motion::Last,
            )))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(Self::on_mouse_back),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(Self::on_mouse_forward),
            )
            .on_action(cx.listener(Self::dispatch_action::<Cancel>(Command::Cancel)))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
    }

    /// Track the held modifier group so the tab close controls follow the press.
    ///
    /// The hint layer keeps its own copy for the labels; this one redraws the
    /// workspace itself, because revealing or hiding a close control is a
    /// layout change, not a label. The event keeps propagating so the hint layer
    /// still sees it.
    pub(in crate::pages::workspace) fn on_modifiers_changed(
        &mut self,
        _: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The window already carries the new state; the stored group is what the
        // last frame drew, so a redraw is only needed when the two differ.
        let held = Self::current_held_modifier(window);
        if self.window.held_modifier != held {
            self.window.held_modifier = held;
            cx.notify();
        }
    }

    /// Route one of Kit's own table navigation actions to the visible page.
    ///
    /// The Kit table binds these strokes under its own key context and its
    /// actions are dispatched before raw key listeners, so a focused table
    /// would otherwise keep every arrow to itself. The page selection moves
    /// here; a page that does not use the action lets it keep propagating, so
    /// the file table's own key handling is untouched.
    fn navigate_visible_page(
        &mut self,
        motion: selection::Motion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editing_input_focused(window, cx)
            && !window.has_active_dialog(cx)
            && !self.terminal_focused(window, cx)
        {
            if let Some(view) = self.remote_view() {
                let (delta, edge) = match motion {
                    selection::Motion::Previous => (-1, false),
                    selection::Motion::Next => (1, false),
                    selection::Motion::First => (-1, true),
                    selection::Motion::Last => (1, true),
                };
                view.update(cx, |view, cx| {
                    view.move_cursor(delta, edge, window.modifiers().shift, cx)
                });
                cx.stop_propagation();
                return;
            }
        }
        if !self.list_keys_available(window, cx) {
            cx.propagate();
            return;
        }
        let moved = match self.collection() {
            Some(Collection::Volumes) => self.move_volume_cursor(motion, cx),
            Some(Collection::Trash) => self.move_trash_cursor(motion, false, cx),
            Some(
                collection @ (Collection::Recent
                | Collection::Favorites
                | Collection::Network
                | Collection::Tags),
            ) => self.move_place_cursor(collection, motion, cx),
            _ => false,
        };
        if moved {
            self.window.focus.focus(window, cx);
            cx.stop_propagation();
        }
    }

    pub(in crate::pages::workspace) fn directory_menu_builder(
        view: WeakEntity<Self>,
        path: PathBuf,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu {
        move |menu, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| {
                    this.directory_context_menu(path.clone(), menu, window, cx)
                })
            } else {
                menu
            }
        }
    }

    pub(in crate::pages::workspace) fn on_mouse_back(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.prevent_default();
        self.dispatch(Command::Back, window, cx);
    }

    pub(in crate::pages::workspace) fn on_mouse_forward(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.prevent_default();
        self.dispatch(Command::Forward, window, cx);
    }

    pub(in crate::pages::workspace) fn click_command(
        command: Command,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| {
            cx.stop_propagation();
            // A deliberate toolbar click may leave the terminal; keyboard
            // actions cannot cross that focus boundary implicitly.
            if this.terminal_focused(window, cx)
                && !matches!(command, Command::Terminal | Command::Settings)
            {
                this.window.focus.focus(window, cx);
            }
            this.dispatch(command, window, cx);
        }
    }

    pub(in crate::pages::workspace) fn click_tab(
        id: u64,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| this.switch_tab(id, window, cx)
    }

    pub(in crate::pages::workspace) fn click_tab_action(
        id: u64,
        unpin: bool,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| {
            cx.stop_propagation();
            if unpin {
                if this.tab_slot(id).is_some_and(|tab| tab.pinned) {
                    this.toggle_pin(id, cx);
                }
                this.window.focus.focus(window, cx);
            } else {
                this.close_tab(id, window, cx);
            }
        }
    }

    pub(in crate::pages::workspace) fn click_location(
        target: LocationTarget,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| {
            this.activate_location_button(target.location(&this.locations.home), window, cx)
        }
    }

    pub(in crate::pages::workspace) fn click_path(
        path: PathBuf,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| {
            cx.stop_propagation();
            if Some(&path) == this.current_path().as_ref() {
                this.window.focus.focus(window, cx);
                this.dispatch(Command::EditLocation, window, cx);
            } else {
                this.navigate(path.clone(), cx);
                this.window.focus.focus(window, cx);
            }
        }
    }

    pub(in crate::pages::workspace) fn on_favorite(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_favorite(cx);
    }

    pub(in crate::pages::workspace) fn on_columns(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.open_columns_editor(window, cx);
    }

    pub(in crate::pages::workspace) fn menu_command(
        view: WeakEntity<Self>,
        command: Command,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| this.dispatch(command, window, cx));
            }
        }
    }

    pub(in crate::pages::workspace) fn tab_menu(
        view: WeakEntity<Self>,
        id: u64,
        pinned: bool,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu {
        move |menu, _, cx| {
            let on_pin = Self::menu_pin(view.clone(), id);
            let on_close = Self::menu_close_tab(view.clone(), id);
            ui::menu::popup(menu)
                .item(el! { $(i18n::text(
                    if pinned { "unpin-tab" } else { "pin-tab" },
                    cx,
                )).PopupMenuItem::new {
                    on_click: on_pin;
                } })
                .item(
                    el! { $(i18n::text("close-this-tab", cx)).PopupMenuItem::new {
                        disabled: pinned;
                        on_click: on_close;
                    } },
                )
        }
    }

    fn menu_pin(view: WeakEntity<Self>, id: u64) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, _, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| this.toggle_pin(id, cx));
            }
        }
    }

    fn menu_close_tab(
        view: WeakEntity<Self>,
        id: u64,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| this.close_tab(id, window, cx));
            }
        }
    }
}
