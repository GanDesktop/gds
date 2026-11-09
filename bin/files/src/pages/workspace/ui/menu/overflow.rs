//! Page actions for the compact toolbar.
use super::*;
use gds_uim::el;

pub(super) fn rows(
    menu: PopupMenu,
    view: WeakEntity<Browser>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(entity) = view.upgrade() else {
        return menu;
    };
    let browser = entity.read(cx);
    let mut menu = ui::menu::popup(menu);
    let mut items = Vec::new();
    let mut add = |cmd: Command, label, icon, enabled, checked| {
        items.push(
            ui::menu::item(i18n::text(label, cx), Some(cmd.into()), enabled)
                .icon(icon)
                .checked(checked)
                .on_click(Browser::menu_command(view.clone(), cmd)),
        );
    };
    match browser.collection() {
        None if matches!(browser.current_location(), Location::Remote(_)) => {
            if let Some(remote) = browser.remote_view() {
                items.extend(remote.read(cx).toolbar_menu(remote.downgrade(), cx));
            }
        }
        None => {
            let grid = browser.grid_mode();
            let favorite = browser
                .locations
                .places
                .is_favorite(browser.current_place_location());
            add(
                Command::ViewMode,
                if grid { "view-list" } else { "view-grid" },
                if grid {
                    IconName::List
                } else {
                    IconName::LayoutGrid
                },
                browser.folder_settled(),
                false,
            );
            add(
                Command::Favorite,
                if favorite {
                    "favorite-remove"
                } else {
                    "favorite-add"
                },
                IconName::Star,
                true,
                favorite,
            );
            add(
                Command::Columns,
                "columns-configure",
                IconName::Columns3Cog,
                browser.columns_available() && !grid,
                false,
            );
            add(
                Command::Hidden,
                Command::Hidden.label_id(),
                IconName::Eye,
                true,
                browser.settings.preferences.files.show_hidden,
            );
            add(
                Command::Details,
                Command::Details.label_id(),
                IconName::Info,
                false,
                false,
            );
            add(
                Command::Refresh,
                "refresh",
                IconName::RefreshCw,
                true,
                false,
            );
        }
        Some(Collection::Recent | Collection::Favorites | Collection::Tags) => {
            let grid = browser.grid_mode();
            add(
                Command::ViewMode,
                if grid { "view-list" } else { "view-grid" },
                if grid {
                    IconName::List
                } else {
                    IconName::LayoutGrid
                },
                browser.browse_settled(),
                false,
            );
            let recent = browser.collection() == Some(Collection::Recent);
            let selected = browser.selected_place();
            let favorite = selected
                .as_ref()
                .is_some_and(|place| browser.locations.places.is_favorite(place));
            if !recent {
                add(
                    Command::Copy,
                    "copy-files",
                    IconName::Copy,
                    browser.selected_place_path().is_some(),
                    false,
                );
            }
            add(
                Command::CopyPaths,
                Command::CopyPaths.label_id(),
                IconName::ClipboardCopy,
                selected.is_some(),
                false,
            );
            add(
                Command::Favorite,
                if favorite {
                    "place-favorite-remove"
                } else {
                    "place-favorite-add"
                },
                IconName::Star,
                selected.is_some(),
                favorite,
            );
            if recent {
                add(
                    Command::ClearRecent,
                    "clear-recent",
                    IconName::Broom,
                    browser
                        .place_locations(Collection::Recent)
                        .is_some_and(|rows| !rows.is_empty()),
                    false,
                );
            } else {
                if browser.selected_deletable_tag().is_some() {
                    add(Command::Trash, "tags-delete", IconName::Trash, true, false);
                }
                add(
                    Command::Refresh,
                    "refresh",
                    IconName::RefreshCw,
                    !browser.collections.favorites.loading,
                    false,
                );
            }
        }
        Some(Collection::Trash) => {
            let trash = &browser.collections.trash;
            let busy = trash.is_busy() || browser.operations.in_progress;
            let selected = trash.selected_count() > 0;
            add(
                Command::RestoreSelection,
                "recycle-restore",
                IconName::Undo2,
                !busy && selected,
                false,
            );
            add(
                Command::PurgeSelection,
                "delete-permanently",
                IconName::Trash,
                !busy && selected,
                false,
            );
            add(
                Command::EmptyTrash,
                "recycle-empty-action",
                IconName::Trash,
                !busy && !trash.rows().is_empty(),
                false,
            );
            add(
                Command::Refresh,
                "refresh",
                IconName::RefreshCw,
                !busy,
                false,
            );
        }
        Some(Collection::Volumes) => {
            let target = browser.volume_action_target(cx);
            for (cmd, label, icon) in [
                (Command::Rename, "rename", IconName::TextCursor),
                (Command::MountVolume, "device-mount", IconName::Plug),
                (Command::UnmountVolume, "device-unmount", IconName::Unplug),
            ] {
                let enabled = target.as_ref().is_some_and(|id| {
                    if cmd == Command::Rename {
                        browser.volume_can_rename(id, cx)
                    } else {
                        browser.volume_command_enabled(id, cmd, cx)
                    }
                });
                add(cmd, label, icon, enabled, false);
            }
            add(
                Command::Refresh,
                "refresh",
                IconName::RefreshCw,
                true,
                false,
            );
        }
        Some(Collection::Network) => {
            add(
                Command::Refresh,
                "refresh",
                IconName::RefreshCw,
                !browser.collections.network.discovery.loading,
                false,
            );
            if browser.picker.is_none() {
                let opener = view.clone();
                items.push(
                    el! { $(i18n::text("remote-add-location", cx)).PopupMenuItem::new {
                        icon: IconName::Plus;
                        disabled: $(browser.remote.connector.read(cx).connecting);
                        on_click: $(move |_, window, _| {
                            let opener = opener.clone();
                            window.on_next_frame(move |window, cx| {
                                let _ = opener.update(cx, |browser, cx| {
                                    browser.set_menu_open(None, cx);
                                    browser.window.focus.focus(window, cx);
                                    browser.add_network_location(window, cx);
                                });
                            });
                        });
                    } },
                );
            }
        }
    }
    for item in items {
        menu = menu.item(item);
    }
    let sort = browser
        .collection()
        .is_none()
        .then(|| browser.active().map(|slot| slot.sort))
        .flatten();
    if let Some(sort) = sort {
        let view = view.clone();
        menu = menu.submenu(i18n::text("sort", cx), window, cx, move |menu, _, cx| {
            super::super::sort::rows(menu, sort, view.clone(), cx)
        });
    }
    menu
}
