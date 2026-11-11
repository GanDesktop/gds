use super::*;

#[derive(Clone, Copy)]
pub(in crate::pages::workspace) enum Motion {
    Previous,
    Next,
    First,
    Last,
}

impl Motion {
    /// The motion a plain arrow key stands for, if any.
    pub(in crate::pages::workspace) fn for_key(key: &str) -> Option<Self> {
        Some(match key {
            "up" => Self::Previous,
            "down" => Self::Next,
            "home" => Self::First,
            "end" => Self::Last,
            _ => return None,
        })
    }

    /// Horizontal arrow motion inside a multi-column grid.
    pub(in crate::pages::workspace) fn for_horizontal_key(key: &str) -> Option<Self> {
        Some(match key {
            "left" => Self::Previous,
            "right" => Self::Next,
            _ => return None,
        })
    }

    pub(in crate::pages::workspace) fn target(
        self,
        current: Option<usize>,
        count: usize,
    ) -> Option<usize> {
        if count == 0 {
            return None;
        }
        Some(match self {
            Self::First => 0,
            Self::Last => count - 1,
            Self::Previous => current.unwrap_or(0).saturating_sub(1),
            Self::Next => current.map_or(0, |index| (index + 1).min(count - 1)),
        })
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn on_list_background_press(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pages.is_empty() || window.has_active_dialog(cx) {
            return;
        }
        self.clear_list_selection(window, cx);
    }

    pub(in crate::pages::workspace) fn clear_list_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.collection() {
            Some(Collection::Volumes) => self.set_selected_volume(None),
            Some(Collection::Trash) => self.collections.trash.clear_selection(),
            Some(
                Collection::Recent | Collection::Favorites | Collection::Network | Collection::Tags,
            ) => {
                if let Some(slot) = self.current_tab_mut() {
                    slot.navigation.view_mut().set_place(None);
                }
            }
            None => {
                if let Some(slot) = self.active_mut() {
                    {
                        let tab = &mut slot.navigation;
                        tab.view_mut().selected.clear();
                    }
                    slot.cursor = None;
                    slot.anchor = None;
                }
            }
        }
        self.window.focus.focus(window, cx);
        cx.notify();
    }

    /// Whether the visible page accepts plain list-navigation keys.
    ///
    /// A dialog, menu or editor owns every keystroke it receives.
    pub(in crate::pages::workspace) fn list_keys_available(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        self.pages.is_empty()
            && self.window.focus.contains_focused(window, cx)
            && !self.sidebar.regions.sidebar_focused(window, cx)
            && !notifications::is_focused(window, cx)
            && !self.menus.is_any_open()
            && !self.operations.rename.read(cx).is_open()
            && !window.has_active_dialog(cx)
            && !self.editing_input_focused(window, cx)
    }

    pub(in crate::pages::workspace) fn file_selection_focused(
        &self,
        window: &Window,
        cx: &App,
    ) -> bool {
        self.pages.is_empty()
            && self.collection().is_none()
            && (self.window.focus.is_focused(window)
                || self
                    .files
                    .table
                    .read(cx)
                    .focus_handle(cx)
                    .contains_focused(window, cx))
    }

    pub(in crate::pages::workspace) fn on_file_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Capture listeners must positively own focus before touching a key;
        // checking only that an editor is absent also steals PTY Escape/arrows.
        if !self.file_selection_focused(window, cx) {
            return;
        }
        // Cancelling a pointer drag must not also clear the file selection.
        if event.keystroke.key == "escape" && cx.stop_active_drag(window) {
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        if let Some(view) = self.remote_view() {
            if !self.list_keys_available(window, cx) {
                return;
            }
            let Some(motion) = Motion::for_key(&event.keystroke.key) else {
                return;
            };
            let (delta, edge) = match motion {
                Motion::Previous => (-1, false),
                Motion::Next => (1, false),
                Motion::First => (-1, true),
                Motion::Last => (1, true),
            };
            view.update(cx, |view, cx| {
                view.move_cursor(delta, edge, modifiers.shift, cx)
            });
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        if matches!(event.keystroke.key.as_str(), "pageup" | "pagedown") {
            if let Some(slot) = self.active() {
                let viewport = f32::from(slot.scroll.0.borrow().base_handle.bounds().size.height);
                let (columns, row_height) = if self.grid_mode() {
                    (slot.grid.columns, slot.grid.row_height)
                } else {
                    (1, gds::appearance::tokens::DataListStyle::ROW_HEIGHT)
                };
                let page_rows = (viewport / row_height).floor().max(1.) as usize;
                let step = page_rows * columns;
                let current = slot
                    .cursor
                    .as_ref()
                    .or_else(|| slot.navigation.view().selected.last())
                    .and_then(|path| slot.table_rows.iter().position(|entry| &entry.path == path))
                    .unwrap_or(0);
                let count = slot.table_rows.len();
                if count > 0 {
                    let index = if event.keystroke.key == "pageup" {
                        current.saturating_sub(step)
                    } else {
                        (current + step).min(count - 1)
                    };
                    self.select(index, modifiers, cx);
                    if let Some(slot) = self.active() {
                        let scroll = &slot.scroll.0.borrow().base_handle;
                        let direction = if event.keystroke.key == "pageup" {
                            1.
                        } else {
                            -1.
                        };
                        let distance = px(page_rows as f32 * row_height);
                        let mut offset = scroll.offset();
                        offset.y =
                            (offset.y + distance * direction).clamp(-scroll.max_offset().y, px(0.));
                        scroll.set_offset(offset);
                    }
                    self.window.focus.focus(window, cx);
                }
            }
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        if self.grid_mode() {
            let Some(slot) = self.active() else {
                return;
            };
            let count = slot.table_rows.len();
            let current = slot
                .cursor
                .as_ref()
                .or_else(|| slot.navigation.view().selected.last())
                .and_then(|path| slot.table_rows.iter().position(|entry| &entry.path == path));
            let target = if let Some(horizontal) = Motion::for_horizontal_key(&event.keystroke.key)
            {
                horizontal.target(current, count)
            } else if let Some(motion) = Motion::for_key(&event.keystroke.key) {
                slot.grid.navigate(motion, current, count)
            } else {
                None
            };
            let Some(index) = target else {
                return;
            };
            self.select(index, modifiers, cx);
            if let Some(slot) = self.active() {
                slot.scroll_to_entry(index);
            }
            self.window.focus.focus(window, cx);
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        let Some(motion) = Motion::for_key(&event.keystroke.key) else {
            return;
        };
        self.move_file_cursor(motion, modifiers.shift, window, cx);
        cx.stop_propagation();
        window.prevent_default();
    }

    pub(in crate::pages::workspace) fn move_file_cursor(
        &mut self,
        motion: Motion,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.active() else {
            return;
        };
        let selected = slot.navigation.view().selected.last();
        let current = slot
            .cursor
            .as_ref()
            .or(selected)
            .and_then(|path| slot.table_rows.iter().position(|entry| &entry.path == path));
        let Some(index) = motion.target(current, slot.table_rows.len()) else {
            return;
        };
        self.select(
            index,
            Modifiers {
                shift: extend,
                ..Default::default()
            },
            cx,
        );
        if let Some(slot) = self.active() {
            slot.scroll_to_entry(index);
        }
        self.window.focus.focus(window, cx);
        cx.notify();
    }

    /// The visible collection whose location rows support keyboard navigation.
    pub(in crate::pages::workspace) fn place_collection(&self) -> Option<Collection> {
        self.collection().filter(|collection| {
            matches!(
                collection,
                Collection::Recent | Collection::Favorites | Collection::Network | Collection::Tags
            )
        })
    }

    pub(in crate::pages::workspace) fn on_place_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(collection) = self.place_collection() else {
            return;
        };
        if !self.list_keys_available(window, cx) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        if self.grid_mode()
            && let Some(horizontal) = Motion::for_horizontal_key(&event.keystroke.key)
        {
            let locations = self.place_locations(collection).unwrap_or_default();
            let Some(slot) = self.current_tab_mut() else {
                return;
            };
            let current = slot
                .navigation
                .view()
                .place()
                .and_then(|location| locations.iter().position(|entry| entry == location));
            if let Some(index) = horizontal.target(current, locations.len()) {
                slot.navigation
                    .view_mut()
                    .set_place(Some(locations[index].clone()));
                slot.scroll_to_entry(index);
                self.sync_workspace_state(cx);
                cx.notify();
                cx.stop_propagation();
                window.prevent_default();
            }
            return;
        }
        let Some(motion) = Motion::for_key(&event.keystroke.key) else {
            return;
        };
        // The cursor moves only; Enter is what opens the selected record.
        if self.move_place_cursor(collection, motion, cx) {
            self.window.focus.focus(window, cx);
            cx.stop_propagation();
            window.prevent_default();
        }
    }

    /// Move a places page's session cursor one row.
    ///
    /// The stored cursor can point at a record that left the list, so the
    /// position is resolved against the current rows every time and an empty
    /// list does nothing. Returns whether the cursor moved.
    pub(in crate::pages::workspace) fn move_place_cursor(
        &mut self,
        collection: Collection,
        motion: Motion,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(locations) = self.place_locations(collection) else {
            return false;
        };
        let current = self
            .current_tab()
            .and_then(|slot| slot.navigation.view().place())
            .and_then(|location| locations.iter().position(|entry| entry == location));
        let target = if self.grid_mode() {
            self.current_tab()
                .and_then(|slot| slot.grid.navigate(motion, current, locations.len()))
        } else {
            motion.target(current, locations.len())
        };
        let Some(index) = target else {
            return false;
        };
        if let Some(slot) = self.current_tab_mut() {
            slot.navigation
                .view_mut()
                .set_place(Some(locations[index].clone()));
            slot.scroll_to_entry(index);
        }
        if !self.grid_mode()
            && let Some(table) = self.place_table(collection)
        {
            table
                .read(cx)
                .vertical_scroll_handle
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
        self.sync_workspace_state(cx);
        cx.notify();
        true
    }

    /// Open the record the places cursor points at, the way a click does.
    ///
    /// The cursor may name a record the list no longer holds, so the path is
    /// checked against the visible rows first.
    pub(in crate::pages::workspace) fn open_place_cursor(&mut self, cx: &mut Context<Self>) {
        if let Some(location) = self.selected_place() {
            if let Location::Directory(path) = location {
                self.activate_path(path.into(), cx);
            } else {
                self.navigate_location(self.tabs.active_id, location, cx);
            }
        }
    }

    pub(in crate::pages::workspace) fn selected_place(&self) -> Option<Location> {
        let collection = self.place_collection()?;
        let location = self.current_tab()?.navigation.view().place()?;
        self.place_locations(collection)?
            .contains(location)
            .then(|| location.clone())
    }

    pub(in crate::pages::workspace) fn selected_place_path(&self) -> Option<PathBuf> {
        self.selected_place()?.local_path().map(Path::to_path_buf)
    }

    pub(in crate::pages::workspace) fn select(
        &mut self,
        visible_index: usize,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let single_selection = self
            .picker
            .as_ref()
            .is_some_and(|picker| !picker.session.allows_multiple());
        let Some(slot) = self.active_mut() else {
            return;
        };
        let Some(path) = slot
            .table_rows
            .get(visible_index)
            .map(|entry| entry.path.clone())
        else {
            return;
        };
        // Single-selection pickers replace the selection on every motion.
        if single_selection {
            let tab = &mut slot.navigation;
            tab.view_mut().selected = vec![path.clone()];
            slot.anchor = Some(path.clone());
            slot.cursor = Some(path);
            cx.notify();
            return;
        }
        let tab = &mut slot.navigation;
        if modifiers.shift {
            let anchor_path = slot
                .anchor
                .clone()
                .or_else(|| slot.cursor.clone())
                .unwrap_or_else(|| path.clone());
            let anchor = slot
                .table_rows
                .iter()
                .position(|entry| entry.path == anchor_path)
                .unwrap_or(visible_index);
            tab.view_mut().selected = slot.table_rows
                [anchor.min(visible_index)..=anchor.max(visible_index)]
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            slot.anchor = Some(anchor_path);
        } else if modifiers.control || modifiers.platform {
            let selected = &mut tab.view_mut().selected;
            if selected.contains(&path) {
                selected.retain(|selected| selected != &path);
            } else {
                selected.push(path.clone());
            }
            slot.anchor = Some(path.clone());
        } else {
            tab.view_mut().selected = vec![path.clone()];
            slot.anchor = Some(path.clone());
        }
        slot.cursor = Some(path);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn select_all_files(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| !picker.session.allows_multiple())
        {
            return;
        }
        if !self.file_selection_focused(window, cx) {
            return;
        }
        let picker_mode = self.picker.is_some();
        let selectable: Option<Vec<PathBuf>> = self.picker.as_ref().and_then(|picker| {
            self.active().map(|slot| {
                slot.table_rows
                    .iter()
                    .filter(|entry| picker.session.accepts_entry(entry))
                    .map(|entry| entry.path.clone())
                    .collect()
            })
        });
        let Some(slot) = self.active_mut() else {
            return;
        };
        let tab = &mut slot.navigation;
        tab.view_mut().selected = selectable.unwrap_or_else(|| {
            slot.table_rows
                .iter()
                .map(|entry| entry.path.clone())
                .collect()
        });
        if picker_mode {
            slot.cursor = tab.view().selected.first().cloned();
        } else if slot.cursor.is_none() {
            slot.cursor = slot.table_rows.first().map(|entry| entry.path.clone());
        }
        slot.anchor = slot.cursor.clone();
        cx.notify();
    }

    pub(in crate::pages::workspace) fn set_sort(
        &mut self,
        sort: sorting::Sort,
        cx: &mut Context<Self>,
    ) {
        let location = self.current_location();
        let Some(slot) = self.active_mut() else {
            return;
        };
        if slot.sort == sort {
            return;
        }
        slot.sort = sort;
        slot.refresh_visible();
        if let Some(index) = slot
            .cursor
            .as_ref()
            .and_then(|path| slot.table_rows.iter().position(|entry| &entry.path == path))
        {
            slot.scroll_to_entry(index);
        }
        self.locations.places.set_sort(location, sort);
        self.save_places(cx);
        cx.notify();
    }
}
