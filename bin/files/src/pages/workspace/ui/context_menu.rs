//! Keyboard context menus reuse the same captured targets and menu builders
//! as pointer menus. Their position follows the selected row, not the mouse.

use gds::appearance::tokens;
use gds_uim::el;
use std::{collections::HashSet, ffi::OsString};

use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

pub(super) struct OpenMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _dismiss: Subscription,
}

#[derive(Clone, PartialEq)]
enum Target {
    Files {
        path: PathBuf,
        selected: Vec<PathBuf>,
    },
    Volume(storage::VolumeId),
    Trash {
        id: OsString,
        selected: HashSet<OsString>,
    },
    Place {
        collection: Collection,
        location: Location,
    },
}

struct RowAnchor {
    index: usize,
    height: Pixels,
    inset: Pixels,
    scroll: ScrollHandle,
}

struct SelectionTarget {
    target: Target,
    anchor: RowAnchor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MenuOrigin {
    tab_id: u64,
    collection: Option<Collection>,
}

struct MenuRequest {
    target: Target,
    position: Point<Pixels>,
    focus: FocusHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaceMenuAction {
    Open,
    OpenInNewTab,
    ShowParent,
    Copy,
    CopyPaths,
    Favorite,
    EditTags,
    DeleteTag,
    Refresh,
}

impl PlaceMenuAction {
    fn label_id(self, favorite: bool) -> &'static str {
        match self {
            Self::Open => Command::Open.label_id(),
            Self::OpenInNewTab => Command::OpenInNewTab.label_id(),
            Self::ShowParent => "tags-show-parent",
            Self::Copy => Command::Copy.label_id(),
            Self::CopyPaths => Command::CopyPaths.label_id(),
            Self::Favorite if favorite => "place-favorite-remove",
            Self::Favorite => "place-favorite-add",
            Self::EditTags => "tags-edit",
            Self::DeleteTag => "tags-delete",
            Self::Refresh => Command::Refresh.label_id(),
        }
    }

    fn icon(self, location: &Location, favorite: bool) -> IconName {
        match self {
            Self::Open | Self::OpenInNewTab => location.icon(),
            Self::ShowParent => IconName::Folder,
            Self::Copy => IconName::Copy,
            Self::CopyPaths => IconName::ClipboardCopy,
            Self::Favorite if favorite => IconName::StarOff,
            Self::Favorite => IconName::Star,
            Self::EditTags => IconName::Tag,
            Self::DeleteTag => IconName::Trash,
            Self::Refresh => IconName::RefreshCw,
        }
    }

    const fn command(self) -> Option<Command> {
        match self {
            Self::Open => Some(Command::Open),
            Self::OpenInNewTab => Some(Command::OpenInNewTab),
            Self::Copy => Some(Command::Copy),
            Self::CopyPaths => Some(Command::CopyPaths),
            Self::Favorite => Some(Command::Favorite),
            Self::DeleteTag => Some(Command::Trash),
            Self::Refresh => Some(Command::Refresh),
            Self::ShowParent | Self::EditTags => None,
        }
    }
}

impl RowAnchor {
    fn position(&self) -> Point<Pixels> {
        let bounds = self.scroll.bounds();
        let bottom = bounds.top() + self.height * (self.index + 1) as f32 + self.scroll.offset().y;
        point(
            bounds.left() + self.inset,
            bottom.clamp(bounds.top(), bounds.bottom()),
        )
    }
}

impl Browser {
    fn collection_menu_anchor(
        &self,
        collection: Collection,
        index: usize,
        height: Pixels,
        cx: &App,
    ) -> Option<RowAnchor> {
        if collection == Collection::Network {
            return None;
        }
        if self.grid_mode() {
            let slot = self.current_tab()?;
            slot.scroll_to_entry(index);
            let tile = slot.grid.tile(index, point(px(0.), px(0.)), px(0.));
            Some(RowAnchor {
                index: index / slot.grid.columns,
                height: px(slot.grid.row_height),
                inset: tile.left() - px(view::TABLE_INSET),
                scroll: slot.scroll.0.borrow().base_handle.clone(),
            })
        } else {
            let table = self.place_table(collection)?;
            let handle = &table.read(cx).vertical_scroll_handle;
            handle.scroll_to_item(index, ScrollStrategy::Nearest);
            Some(RowAnchor {
                index,
                height,
                inset: px(tokens::DataListStyle::CELL_PADDING_X),
                scroll: handle.0.borrow().base_handle.clone(),
            })
        }
    }

    fn selection_menu_target(&self, cx: &App) -> Option<SelectionTarget> {
        let height = px(tokens::DataListStyle::ROW_HEIGHT);
        match self.collection() {
            Some(Collection::Volumes) => {
                let id = self.volume_action_target(cx)?;
                let index = self
                    .devices
                    .service
                    .read(cx)
                    .snapshot
                    .volumes
                    .iter()
                    .filter(|volume| volume.visible)
                    .position(|volume| volume.id == id)?;
                let handle = &self.devices.table.read(cx).vertical_scroll_handle;
                handle.scroll_to_item(index, ScrollStrategy::Nearest);
                Some(SelectionTarget {
                    target: Target::Volume(id),
                    anchor: RowAnchor {
                        index,
                        height,
                        inset: px(tokens::DataListStyle::CELL_PADDING_X),
                        scroll: handle.0.borrow().base_handle.clone(),
                    },
                })
            }
            Some(Collection::Trash) => {
                let selected = self.collections.trash.selection();
                let index = self
                    .collections
                    .trash
                    .cursor_index()
                    .filter(|&index| selected.contains(&self.collections.trash.rows()[index].id))
                    .or_else(|| {
                        self.collections
                            .trash
                            .rows()
                            .iter()
                            .position(|entry| selected.contains(&entry.id))
                    })?;
                let id = self.collections.trash.rows()[index].id.clone();
                let handle = self.collections.trash.scroll_handle(cx);
                handle.scroll_to_item(index, ScrollStrategy::Nearest);
                Some(SelectionTarget {
                    target: Target::Trash { id, selected },
                    anchor: RowAnchor {
                        index,
                        height,
                        inset: px(tokens::DataListStyle::CELL_PADDING_X),
                        scroll: handle.0.borrow().base_handle.clone(),
                    },
                })
            }
            Some(
                collection @ (Collection::Recent
                | Collection::Favorites
                | Collection::Network
                | Collection::Tags),
            ) => {
                let location = self.selected_place()?;
                let index = self
                    .place_locations(collection)?
                    .iter()
                    .position(|entry| entry == &location)?;
                let anchor = self.collection_menu_anchor(collection, index, height, cx)?;
                Some(SelectionTarget {
                    target: Target::Place {
                        collection,
                        location,
                    },
                    anchor,
                })
            }
            None => {
                let slot = self
                    .active()
                    .filter(|slot| slot.files_available() && !slot.is_loading())?;
                let selected = self.selected_visible_paths();
                let path = slot
                    .cursor
                    .as_ref()
                    .filter(|path| selected.contains(path))
                    .or(selected.first())?
                    .clone();
                let index = slot
                    .table_rows
                    .iter()
                    .position(|entry| entry.path == path)?;
                slot.scroll_to_entry(index);
                let scroll = slot.scroll.0.borrow().base_handle.clone();
                let anchor = if self.grid_mode() {
                    let tile = slot.grid.tile(index, point(px(0.), px(0.)), px(0.));
                    RowAnchor {
                        index: index / slot.grid.columns,
                        height: px(slot.grid.row_height),
                        inset: tile.left() - px(view::TABLE_INSET),
                        scroll,
                    }
                } else {
                    RowAnchor {
                        index,
                        height,
                        inset: px(tokens::DataListStyle::CELL_PADDING_X),
                        scroll,
                    }
                };
                Some(SelectionTarget {
                    target: Target::Files { path, selected },
                    anchor,
                })
            }
        }
    }

    pub(super) fn open_selection_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_keys_available(window, cx) {
            return;
        }
        let Some(SelectionTarget { target, .. }) = self.selection_menu_target(cx) else {
            return;
        };
        let origin = MenuOrigin {
            tab_id: self.tabs.active_id,
            collection: self.collection(),
        };
        let browser = cx.entity().downgrade();
        // Let the selected row scroll into view before positioning its menu.
        window.on_next_frame(move |window, cx| {
            let prepared = browser
                .update(cx, |view, cx| {
                    let current_origin = MenuOrigin {
                        tab_id: view.tabs.active_id,
                        collection: view.collection(),
                    };
                    if !view.list_keys_available(window, cx) || current_origin != origin {
                        return None;
                    }
                    let current = view.selection_menu_target(cx)?;
                    (current.target == target).then(|| MenuRequest {
                        target,
                        position: current.anchor.position(),
                        focus: view.window.focus.clone(),
                    })
                })
                .ok()
                .flatten();
            if let Some(request) = prepared {
                Self::show_selection_menu(browser, request, window, cx);
            }
        });
        cx.notify();
    }

    fn show_selection_menu(
        browser: WeakEntity<Self>,
        request: MenuRequest,
        window: &mut Window,
        cx: &mut App,
    ) {
        let MenuRequest {
            target,
            position,
            focus,
        } = request;
        let builder = browser.clone();
        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
            let menu = menu.action_context(focus);
            let Some(view) = builder.upgrade() else {
                return menu;
            };
            view.update(cx, |view, cx| match target {
                Target::Files { path, .. } => view.entry_context_menu(path, menu, window, cx),
                Target::Volume(id) => view.build_volume_menu(builder.clone(), &id, menu, cx),
                Target::Trash { .. } => view.trash_selection_menu(menu, cx),
                Target::Place { .. } => view.place_selection_menu(menu, cx),
            })
        });
        if menu.read(cx).is_empty() {
            return;
        }
        let menu_id = menu.entity_id();
        let _ = browser.update(cx, |view, cx| {
            let dismiss =
                cx.subscribe_in(&menu, window, move |view, _, _: &DismissEvent, _, cx| {
                    if view
                        .menus
                        .context
                        .as_ref()
                        .is_some_and(|open| open.menu.entity_id() == menu_id)
                    {
                        view.menus.context = None;
                        cx.notify();
                    }
                });
            view.menus.context = Some(OpenMenu {
                menu: menu.clone(),
                position,
                _dismiss: dismiss,
            });
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);
        // The new menu enters the focus tree on this frame.
        let opening = menu.downgrade();
        window.on_next_frame(move |window, cx| {
            if browser.upgrade().is_some_and(|view| {
                view.read(cx)
                    .menus
                    .context
                    .as_ref()
                    .is_some_and(|open| open.menu.entity_id() == menu_id)
            }) && let Some(menu) = opening.upgrade()
            {
                menu.focus_handle(cx).focus(window, cx);
                window.dispatch_action(Box::new(gpui_kit::base::actions::SelectDown), cx);
            }
        });
    }

    pub(super) fn render_selection_menu(&self, window: &Window) -> Option<AnyElement> {
        let open = self.menus.context.as_ref()?;
        Some(ui::menu::overlay(open.menu.clone(), open.position, window))
    }

    pub(super) fn place_selection_menu(
        &self,
        mut menu: PopupMenu,
        cx: &Context<Self>,
    ) -> PopupMenu {
        menu = ui::menu::popup(menu);
        let Some(location) = self.selected_place() else {
            return menu;
        };
        let collection = self.collection();
        let local = location.local_path().is_some();
        let mut open = vec![PlaceMenuAction::Open, PlaceMenuAction::OpenInNewTab];
        if local {
            open.push(PlaceMenuAction::ShowParent);
        }
        let mut copy = Vec::new();
        if local && matches!(collection, Some(Collection::Favorites | Collection::Tags)) {
            copy.push(PlaceMenuAction::Copy);
        }
        copy.push(PlaceMenuAction::CopyPaths);
        let mut organize = vec![PlaceMenuAction::Favorite];
        if local && self.settings.preferences.tags.enabled {
            organize.push(PlaceMenuAction::EditTags);
        }
        let mut groups = vec![open, copy, organize];
        if self.selected_deletable_tag().is_some() {
            groups.push(vec![PlaceMenuAction::DeleteTag]);
        }
        if matches!(collection, Some(Collection::Favorites | Collection::Tags)) {
            groups.push(vec![PlaceMenuAction::Refresh]);
        }
        for (group, items) in groups.into_iter().enumerate() {
            if group > 0 {
                menu = menu.separator();
            }
            for item in items {
                let favorite = self.locations.places.is_favorite(&location);
                let label = item.label_id(favorite);
                let icon = item.icon(&location, favorite);
                let command = item.command();
                let shortcut = command
                    .filter(|command| *command != Command::OpenInNewTab || self.picker.is_none());
                let row = ui::menu::row(i18n::text(label, cx), shortcut.map(Into::into));
                let browser = cx.entity().downgrade();
                let target = location.clone();
                menu = menu.item(el! { row.PopupMenuItem::element {
                    icon: icon;
                    on_click: $(move |_, window, cx| {
                        let _ = browser.update(cx, |view, cx| {
                            if view.collection() != collection || view.selected_place().as_ref() != Some(&target) { return; }
                            if let Some(command) = item.command() {
                                view.dispatch(command, window, cx);
                                return;
                            }
                            match item {
                                PlaceMenuAction::ShowParent => {
                                    if let Some(path) = target.local_path() && let Some(parent) = path.parent() {
                                        if let Some(slot) = view.current_tab_mut() { slot.pending_selection = Some(path.to_path_buf()); }
                                        view.navigate(parent.to_path_buf(), cx);
                                    }
                                }
                                PlaceMenuAction::EditTags => {
                                    if let Some(path) = target.local_path() { view.edit_tags(path.to_path_buf(), window, cx); }
                                }
                                PlaceMenuAction::Open
                                | PlaceMenuAction::OpenInNewTab
                                | PlaceMenuAction::Copy
                                | PlaceMenuAction::CopyPaths
                                | PlaceMenuAction::Favorite
                                | PlaceMenuAction::DeleteTag
                                | PlaceMenuAction::Refresh => {}
                            }
                        });
                    });
                } });
            }
        }
        menu
    }
}
