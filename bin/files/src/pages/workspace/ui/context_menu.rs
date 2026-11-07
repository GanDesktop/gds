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

struct MenuRequest {
    target: Target,
    position: Point<Pixels>,
    focus: FocusHandle,
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
    fn selection_menu_target(&self, cx: &App) -> Option<(Target, RowAnchor)> {
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
                Some((
                    Target::Volume(id),
                    RowAnchor {
                        index,
                        height,
                        inset: px(tokens::DataListStyle::CELL_PADDING_X),
                        scroll: handle.0.borrow().base_handle.clone(),
                    },
                ))
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
                Some((
                    Target::Trash { id, selected },
                    RowAnchor {
                        index,
                        height,
                        inset: px(tokens::DataListStyle::CELL_PADDING_X),
                        scroll: handle.0.borrow().base_handle.clone(),
                    },
                ))
            }
            Some(
                collection @ (Collection::Recent | Collection::Favorites | Collection::Network),
            ) => {
                let location = self.selected_place()?;
                let index = self
                    .place_locations(collection)?
                    .iter()
                    .position(|entry| entry == &location)?;
                let table = self.place_table(collection)?;
                let handle = &table.read(cx).vertical_scroll_handle;
                handle.scroll_to_item(index, ScrollStrategy::Nearest);
                let scroll = handle.0.borrow().base_handle.clone();
                Some((
                    Target::Place {
                        collection,
                        location,
                    },
                    RowAnchor {
                        index,
                        height,
                        inset: px(tokens::DataListStyle::CELL_PADDING_X),
                        scroll,
                    },
                ))
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
                let (index, height, inset) = if self.grid_mode() {
                    let tile = slot.grid.tile(index, point(px(0.), px(0.)), px(0.));
                    (
                        index / slot.grid.columns,
                        px(slot.grid.row_height),
                        tile.left() - px(view::TABLE_INSET),
                    )
                } else {
                    (index, height, px(tokens::DataListStyle::CELL_PADDING_X))
                };
                Some((
                    Target::Files { path, selected },
                    RowAnchor {
                        index,
                        height,
                        inset,
                        scroll: slot.scroll.0.borrow().base_handle.clone(),
                    },
                ))
            }
        }
    }

    pub(super) fn open_selection_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_keys_available(window, cx) {
            return;
        }
        let Some((target, _)) = self.selection_menu_target(cx) else {
            return;
        };
        let origin = (self.tabs.active_id, self.collection());
        let browser = cx.entity().downgrade();
        // Let the selected row scroll into view before positioning its menu.
        window.on_next_frame(move |window, cx| {
            let prepared = browser
                .update(cx, |view, cx| {
                    if !view.list_keys_available(window, cx)
                        || (view.tabs.active_id, view.collection()) != origin
                    {
                        return None;
                    }
                    let (current, anchor) = view.selection_menu_target(cx)?;
                    (current == target).then(|| (anchor.position(), view.window.focus.clone()))
                })
                .ok()
                .flatten();
            if let Some((position, focus)) = prepared {
                Self::show_selection_menu(
                    browser,
                    MenuRequest {
                        target,
                        position,
                        focus,
                    },
                    window,
                    cx,
                );
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
        let mut commands = vec![Command::Open, Command::OpenInNewTab, Command::CopyPaths];
        if matches!(
            collection,
            Some(Collection::Favorites | Collection::Network)
        ) {
            if location.local_path().is_some() {
                commands.insert(2, Command::Copy);
            }
            commands.push(Command::Refresh);
        }
        commands.push(Command::Favorite);
        for command in commands {
            let browser = cx.entity().downgrade();
            let target = location.clone();
            let label = i18n::text(
                if command == Command::Favorite {
                    if self.locations.places.is_favorite(&location) {
                        "place-favorite-remove"
                    } else {
                        "place-favorite-add"
                    }
                } else {
                    command.label_id()
                },
                cx,
            );
            let icon = match command {
                Command::Open | Command::OpenInNewTab => IconName::Folder,
                Command::Copy => IconName::Copy,
                Command::CopyPaths => IconName::ClipboardCopy,
                Command::Favorite if self.locations.places.is_favorite(&location) => {
                    IconName::StarOff
                }
                Command::Favorite => IconName::Star,
                Command::Refresh => IconName::RefreshCw,
                _ => unreachable!("place menu actions have icons"),
            };
            let shortcut = command != Command::OpenInNewTab || self.picker.is_none();
            let row = ui::menu::row(label, shortcut.then(|| command.into()));
            menu = menu.item(el! { row.PopupMenuItem::element {
                icon: icon;
                on_click: $(move |_, window, cx| {
                    let _ = browser.update(cx, |view, cx| {
                        if view.collection() == collection
                            && view.selected_place().as_ref() == Some(&target)
                        {
                            view.dispatch(command, window, cx);
                        }
                    });
                });
            } });
        }
        menu
    }
}
