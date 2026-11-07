use super::*;
use gds_uim::el;
use gpui_kit::base::actions::{SelectDown, SelectFirst, SelectLast, SelectUp};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

impl Browser {
    pub(super) fn on_deselect_all(
        &mut self,
        _: &DeselectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::DeselectAll, window, cx);
    }

    pub(super) fn on_selection_menu(
        &mut self,
        _: &SelectionMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::ContextMenu, window, cx);
    }

    pub(super) fn on_application_menu(
        &mut self,
        _: &ApplicationMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Menu, window, cx);
    }
    pub(super) fn on_help(
        &mut self,
        _: &gds::ui::help::OpenHelp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Help, window, cx);
    }

    pub(super) fn on_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Settings, window, cx);
    }
    pub(super) fn on_new_folder(
        &mut self,
        _: &NewFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::NewFolder, window, cx);
    }
    pub(super) fn on_new_window(
        &mut self,
        _: &NewWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::NewWindow, window, cx);
    }

    pub(super) fn on_new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch(Command::NewTab, window, cx);
    }
    pub(super) fn on_close_tab(
        &mut self,
        _: &CloseTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::CloseTab, window, cx);
    }
    pub(super) fn on_sidebar(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Sidebar, window, cx);
    }
    pub(super) fn on_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Terminal, window, cx);
    }
    pub(super) fn on_edit_location(
        &mut self,
        _: &EditLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::EditLocation, window, cx);
    }
    pub(super) fn on_back(
        &mut self,
        _: &NavigateBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Back, window, cx);
    }
    pub(super) fn on_forward(
        &mut self,
        _: &NavigateForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Forward, window, cx);
    }
    pub(super) fn on_parent(
        &mut self,
        _: &NavigateParent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Parent, window, cx);
    }
    pub(super) fn on_refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch(Command::Refresh, window, cx);
    }
    pub(super) fn on_filter(
        &mut self,
        _: &FilterNames,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Filter, window, cx);
    }
    pub(super) fn on_hidden(
        &mut self,
        _: &ToggleHidden,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Hidden, window, cx);
    }
    pub(super) fn on_open(
        &mut self,
        _: &OpenSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Open, window, cx);
    }
    pub(super) fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch(Command::Cancel, window, cx);
    }

    /// Track the held modifier group so the tab close controls follow the press.
    ///
    /// The hint layer keeps its own copy for the labels; this one redraws the
    /// workspace itself, because revealing or hiding a close control is a
    /// layout change, not a label. The event keeps propagating so the hint layer
    /// still sees it.
    pub(super) fn on_modifiers_changed(
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

    pub(super) fn on_select_all(
        &mut self,
        _: &SelectAllFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::SelectAll, window, cx);
    }

    pub(super) fn on_rename(
        &mut self,
        _: &RenameSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Rename, window, cx);
    }

    pub(super) fn on_details(
        &mut self,
        _: &ToggleDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Details, window, cx);
    }

    pub(super) fn on_configure_columns(
        &mut self,
        _: &ConfigureColumns,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Columns, window, cx);
    }

    pub(super) fn on_favorite_current_folder(
        &mut self,
        _: &FavoriteCurrentFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Favorite, window, cx);
    }

    pub(super) fn on_trash(
        &mut self,
        _: &TrashSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Trash, window, cx);
    }

    pub(super) fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch(Command::NextTab, window, cx);
    }

    pub(super) fn on_previous_tab(
        &mut self,
        _: &PreviousTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::PreviousTab, window, cx);
    }

    pub(super) fn on_delete_permanently(
        &mut self,
        _: &DeleteSelectionPermanently,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::DeletePermanent, window, cx);
    }

    pub(super) fn on_restore_selection(
        &mut self,
        _: &RestoreSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::RestoreSelection, window, cx);
    }

    pub(super) fn on_purge_selection(
        &mut self,
        _: &PurgeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::PurgeSelection, window, cx);
    }

    pub(super) fn on_empty_trash(
        &mut self,
        _: &EmptyTrash,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::EmptyTrash, window, cx);
    }

    pub(super) fn on_clear_recent_action(
        &mut self,
        _: &ClearRecent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::ClearRecent, window, cx);
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
                collection @ (Collection::Recent | Collection::Favorites | Collection::Network),
            ) => self.move_place_cursor(collection, motion, cx),
            _ => false,
        };
        if moved {
            self.window.focus.focus(window, cx);
            cx.stop_propagation();
        }
    }

    pub(super) fn on_select_up(
        &mut self,
        _: &SelectUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_visible_page(selection::Motion::Previous, window, cx);
    }

    pub(super) fn on_select_down(
        &mut self,
        _: &SelectDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_visible_page(selection::Motion::Next, window, cx);
    }

    pub(super) fn on_select_first(
        &mut self,
        _: &SelectFirst,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_visible_page(selection::Motion::First, window, cx);
    }

    pub(super) fn on_select_last(
        &mut self,
        _: &SelectLast,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_visible_page(selection::Motion::Last, window, cx);
    }

    pub(super) fn on_copy_files(
        &mut self,
        _: &CopyFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Copy, window, cx);
    }

    pub(super) fn on_cut_files(
        &mut self,
        _: &CutFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Cut, window, cx);
    }

    pub(super) fn on_undo_files(
        &mut self,
        _: &UndoFileOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Undo, window, cx);
    }

    pub(super) fn on_redo_files(
        &mut self,
        _: &RedoFileOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Redo, window, cx);
    }

    pub(super) fn on_paste_files(
        &mut self,
        _: &PasteFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::Paste, window, cx);
    }

    pub(super) fn on_copy_paths(
        &mut self,
        _: &CopyFilePaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::CopyPaths, window, cx);
    }

    pub(super) fn on_open_in_new_tabs(
        &mut self,
        _: &OpenSelectionInNewTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::OpenInNewTab, window, cx);
    }

    pub(super) fn directory_menu_builder(
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

    pub(super) fn on_mouse_back(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = if !self.pages.is_empty() || self.collection().is_some() {
            Command::Cancel
        } else {
            Command::Back
        };
        cx.stop_propagation();
        window.prevent_default();
        self.dispatch(command, window, cx);
    }

    pub(super) fn on_mouse_forward(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pages.is_empty() || self.collection().is_some() {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        self.dispatch(Command::Forward, window, cx);
    }

    pub(super) fn click_command(
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

    pub(super) fn click_tab(
        id: u64,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| this.switch_tab(id, window, cx)
    }

    pub(super) fn click_tab_action(
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

    pub(super) fn click_location(
        target: LocationTarget,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| {
            this.activate_location_button(target.location(&this.locations.home), window, cx)
        }
    }

    pub(super) fn click_path(
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

    pub(super) fn on_favorite(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_favorite(cx);
    }

    pub(super) fn on_columns(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.open_columns_editor(window, cx);
    }

    pub(super) fn menu_command(
        view: WeakEntity<Self>,
        command: Command,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| this.dispatch(command, window, cx));
            }
        }
    }

    pub(super) fn tab_menu(
        view: WeakEntity<Self>,
        id: u64,
        pinned: bool,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu {
        move |menu, _, cx| {
            let pinnable = view.upgrade().is_some_and(|view| {
                view.read(cx)
                    .tab_slot(id)
                    .is_some_and(|tab| tab.path().is_some())
            });
            let on_pin = Self::menu_pin(view.clone(), id);
            let on_close = Self::menu_close_tab(view.clone(), id);
            ui::menu::popup(menu)
                .item(el! { $(i18n::text(
                    if pinned { "unpin-tab" } else { "pin-tab" },
                    cx,
                )).PopupMenuItem::new {
                    disabled: $(!pinned && !pinnable);
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

impl Browser {
    pub(super) fn on_view_mode(
        &mut self,
        _: &ToggleViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::ViewMode, window, cx);
    }
    pub(super) fn on_sort(&mut self, _: &SortFiles, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch(Command::Sort, window, cx);
    }
}
