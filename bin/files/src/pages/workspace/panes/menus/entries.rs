//! Context-menu entry builders for selection, directory, tab, quick-access, and history surfaces.

use std::path::{Path, PathBuf};

use gds::i18n;
use gds::ui::{IconName, menu::MenuEntry};
use zgui::reactive::UnsyncCallback;

use super::super::{inspection::Inspection, terminals::TerminalDock};
use super::actions::{
    change_volume, copy_selected_paths, directory_action_available, open_external_terminal,
    open_selected_tab, open_selected_with, place_location, run_directory_action,
    show_selected_parent,
};
use crate::{
    entities::{
        file::{
            directory::{Entry, EntryKind},
            ui::FileIcon,
        },
        location::{Location, volumes},
    },
    pages::workspace::{
        navigation::{Navigator, QuickAccess},
        operations::{DeviceOps, Dispatch, LocalOps, RemoteOps, TagOps},
        projection::Rows,
        state::{
            context::Context,
            menus::{DirectoryAction, DirectoryTarget, Surface},
            model::{Contents, Row, TRASH_TASK_KEY, remote_task_key},
            ui_flags::QuickAccessAddState,
        },
    },
    shared::{lib::clipboard::Clipboard, ui::actions::Command},
};

fn command_icon(command: Command) -> Option<IconName> {
    Some(match command {
        Command::NewWindow => IconName::Plus,
        Command::NewFolder => IconName::FolderPlus,
        Command::Copy => IconName::Copy,
        Command::Cut => IconName::Scissors,
        Command::Paste => IconName::ClipboardPaste,
        Command::CopyPaths => IconName::ClipboardCopy,
        Command::Open => IconName::ExternalLink,
        Command::OpenInNewTab => IconName::Folder,
        Command::Rename => IconName::TextCursor,
        Command::Trash | Command::DeletePermanent | Command::PurgeSelection => IconName::Trash,
        Command::EmptyTrash | Command::ClearRecent => IconName::Broom,
        Command::RestoreSelection | Command::Undo => IconName::Undo2,
        Command::Redo => IconName::Redo2,
        Command::Refresh => IconName::RefreshCw,
        Command::MountVolume => IconName::Plug,
        Command::UnmountVolume => IconName::Unplug,
        Command::Favorite => IconName::Star,
        Command::Settings => IconName::Settings,
        Command::Help => IconName::BookOpen,
        _ => return None,
    })
}

fn menu_action(ctx: &Context, action: impl Fn(&Context) + 'static) -> UnsyncCallback<()> {
    let weak = ctx.downgrade();
    UnsyncCallback::new(move |()| {
        if let Some(ctx) = weak.upgrade() {
            ctx.0.owner.with(|| action(&ctx));
        }
    })
}

fn labeled_item(
    ctx: &Context,
    label: String,
    icon: Option<IconName>,
    shortcut: Option<Command>,
    disabled: bool,
    action: impl Fn(&Context) + 'static,
) -> MenuEntry {
    MenuEntry::Item {
        label,
        icon,
        shortcut: shortcut.map(Command::shortcut),
        disabled,
        destructive: false,
        on_select: menu_action(ctx, action),
    }
}

fn menu_item_entry(
    ctx: &Context,
    label_id: &'static str,
    icon: Option<IconName>,
    shortcut: Option<Command>,
    disabled: bool,
    action: impl Fn(&Context) + 'static,
) -> MenuEntry {
    labeled_item(
        ctx,
        i18n::text_untracked(label_id),
        icon,
        shortcut,
        disabled,
        action,
    )
}

fn command_item(ctx: &Context, command: Command, action: impl Fn(&Context) + 'static) -> MenuEntry {
    labeled_item(
        ctx,
        i18n::text_untracked(command.label_id()),
        command_icon(command),
        Some(command),
        false,
        action,
    )
}

fn paste_item(ctx: &Context) -> MenuEntry {
    labeled_item(
        ctx,
        i18n::text_untracked(Command::Paste.label_id()),
        Some(IconName::ClipboardPaste),
        Some(Command::Paste),
        !Clipboard::current().can_paste_untracked(),
        |c| Dispatch(c).paste(),
    )
}

fn plural_label(count: usize, singular: &'static str, plural: &'static str) -> String {
    if count > 1 {
        i18n::count_untracked(plural, count)
    } else {
        i18n::text_untracked(singular)
    }
}

fn edit_properties_entry(ctx: &Context, entry: &Entry, disabled: bool) -> MenuEntry {
    let edit_entry = entry.clone();
    menu_item_entry(
        ctx,
        "details-edit-properties-menu",
        Some(IconName::Pencil),
        None,
        disabled,
        move |c| Inspection(c).edit_item_properties(edit_entry.clone(), None),
    )
}

fn favorite_toggle_entry(ctx: &Context, location: Location, favorite: bool) -> MenuEntry {
    menu_item_entry(
        ctx,
        if favorite {
            "place-favorite-remove"
        } else {
            "place-favorite-add"
        },
        Some(IconName::Star),
        Some(Command::Favorite),
        false,
        move |c| {
            c.update(|state| state.places.toggle_favorite(location.clone()));
            c.save_places();
            Navigator(c).refresh_location(&Location::Favorites);
        },
    )
}

pub(super) fn context_popup_entries(
    ctx: &Context,
    surface: Surface,
) -> Option<(f32, f32, Vec<MenuEntry>)> {
    match surface {
        Surface::Selection { target, x, y } => ctx
            .read_untracked(|state| target.resolve_row(state))
            .map(|row| (x, y, selection_entries(ctx, &row))),
        Surface::Directory { target, x, y } => Some((x, y, directory_entries(ctx, target))),
        Surface::Trash { x, y, .. } => Some((x, y, trash_entries(ctx, false))),
        Surface::Tab { id, x, y } => ctx
            .read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .map(|tab| tab.pinned)
            })
            .map(|pinned| (x, y, tab_entries(ctx, id, pinned))),
        Surface::Volume { volume, x, y } => {
            Some((x, y, selection_entries(ctx, &Row::Volume(volume))))
        }
        Surface::QuickAccessSection { x, y } => Some((x, y, vec![quick_access_add_entry(ctx)])),
        Surface::QuickAccessItem { path, x, y } => {
            Some((x, y, quick_access_item_entries(ctx, path)))
        }
        Surface::History { tab, visits, x, y } => Some((x, y, history_entries(ctx, tab, visits))),
        _ => None,
    }
}

fn quick_access_add_entry(ctx: &Context) -> MenuEntry {
    let state = QuickAccess(ctx).quick_access_add_state_untracked();
    let label = match state {
        QuickAccessAddState::CanAdd => i18n::text_untracked("quick-access-add-current"),
        QuickAccessAddState::AlreadyAdded => i18n::text_untracked("quick-access-already-added"),
        QuickAccessAddState::NotLocal => i18n::text_untracked("quick-access-local-only"),
    };
    labeled_item(
        ctx,
        label,
        Some(IconName::Plus),
        None,
        state != QuickAccessAddState::CanAdd,
        |c| QuickAccess(c).add_current_to_quick_access(),
    )
}

fn quick_access_item_entries(ctx: &Context, path: PathBuf) -> Vec<MenuEntry> {
    let path_new = path.clone();
    vec![
        menu_item_entry(
            ctx,
            Command::OpenInNewTab.label_id(),
            Some(IconName::FolderPlus),
            None,
            false,
            move |c| QuickAccess(c).open_quick_access(&path_new, true),
        ),
        MenuEntry::Separator,
        quick_access_add_entry(ctx),
        menu_item_entry(
            ctx,
            "quick-access-remove",
            Some(IconName::X),
            None,
            false,
            move |c| QuickAccess(c).remove_quick_access(&path),
        ),
    ]
}

fn history_entries(ctx: &Context, tab: u64, visits: Vec<(usize, Location)>) -> Vec<MenuEntry> {
    let rows = Rows(ctx);
    visits
        .into_iter()
        .map(|(index, location)| {
            let label = rows.resolve_path_untracked(&location).map_or_else(
                || rows.location_title(&location),
                |path| path.display().to_string(),
            );
            let icon = Some(location.icon());
            let weak = ctx.downgrade();
            MenuEntry::Item {
                label,
                icon,
                shortcut: None,
                disabled: false,
                destructive: false,
                on_select: UnsyncCallback::new(move |()| {
                    if let Some(ctx) = weak.upgrade() {
                        Navigator(&ctx).visit_history(tab, index, &location);
                        ctx.focus_content_after_mount();
                    }
                }),
            }
        })
        .collect()
}

fn directory_entries(ctx: &Context, target: DirectoryTarget) -> Vec<MenuEntry> {
    let item = |action: DirectoryAction, icon: IconName, command: Option<Command>| {
        let target_run = target.clone();
        let disabled = !directory_action_available(ctx, action, &target);
        menu_item_entry(
            ctx,
            action.label(),
            Some(icon),
            command,
            disabled,
            move |c| run_directory_action(c, action, target_run.clone()),
        )
    };

    let mut entries = vec![item(
        DirectoryAction::NewFolder,
        IconName::FolderPlus,
        Some(Command::NewFolder),
    )];
    let templates = crate::features::file_operations::templates::discover_user();
    if !templates.is_empty() {
        let disabled = !directory_action_available(ctx, DirectoryAction::NewFolder, &target);
        let template_entries = templates
            .into_iter()
            .map(|template| {
                let target_run = target.clone();
                let template_path = template.path;
                labeled_item(
                    ctx,
                    template.label,
                    Some(IconName::FileText),
                    None,
                    disabled,
                    move |c| {
                        let Some(target_path) =
                            c.read_untracked(|state| target_run.current_path(state))
                        else {
                            c.report_notice(i18n::text_untracked("location-changed"));
                            return;
                        };
                        LocalOps(c).create_from_template(target_path, template_path.clone());
                    },
                )
            })
            .collect();
        entries.push(MenuEntry::Submenu {
            label: i18n::text_untracked("new-from-template"),
            icon: Some(IconName::FileText),
            entries: template_entries,
        });
    }
    entries.extend([
        MenuEntry::Separator,
        item(
            DirectoryAction::Paste,
            IconName::ClipboardPaste,
            Some(Command::Paste),
        ),
        MenuEntry::Separator,
    ]);
    if ctx.0.preferences.snapshot().terminal_enabled() {
        if ctx.read_untracked(|state| state.picker.is_none()) {
            entries.push(item(
                DirectoryAction::InternalTerminal,
                IconName::Terminal,
                None,
            ));
        }
        entries.push(item(DirectoryAction::Terminal, IconName::Terminal, None));
    }
    entries.push(item(
        DirectoryAction::Refresh,
        IconName::RefreshCw,
        Some(Command::Refresh),
    ));
    entries
}

fn tab_entries(ctx: &Context, id: u64, pinned: bool) -> Vec<MenuEntry> {
    vec![
        menu_item_entry(
            ctx,
            if pinned { "unpin-tab" } else { "pin-tab" },
            None,
            None,
            false,
            move |c| {
                if let Some(index) =
                    c.read_untracked(|state| state.tabs.iter().position(|tab| tab.id == id))
                {
                    Navigator(c).toggle_pin(index);
                }
            },
        ),
        menu_item_entry(
            ctx,
            Command::NewTab.label_id(),
            None,
            Some(Command::NewTab),
            false,
            |c| Navigator(c).new_tab(None),
        ),
        menu_item_entry(
            ctx,
            Command::CloseTab.label_id(),
            None,
            Some(Command::CloseTab),
            false,
            move |c| {
                if let Some(index) =
                    c.read_untracked(|state| state.tabs.iter().position(|tab| tab.id == id))
                {
                    Navigator(c).close_tab(index);
                }
            },
        ),
    ]
}

fn selection_entries(ctx: &Context, row: &Row) -> Vec<MenuEntry> {
    match row {
        Row::Trash(_) => trash_entries(ctx, true),
        Row::Volume(volume) => volume_entries(ctx, volume),
        Row::Tag(_) | Row::Place(_) => place_entries(ctx, row),
        Row::Remote(entry) => remote_entries(ctx, entry),
        Row::File(_)
            if ctx.read_untracked(|state| {
                matches!(state.tab().navigation.location(), Location::Tags(_))
            }) =>
        {
            place_entries(ctx, row)
        }
        Row::File(entry)
            if ctx.read_untracked(|state| {
                matches!(state.tab().navigation.location(), Location::Recent)
            }) =>
        {
            recent_file_entries(ctx, entry)
        }
        Row::File(entry) => file_entries(ctx, entry),
    }
}

fn volume_entries(ctx: &Context, volume: &volumes::Volume) -> Vec<MenuEntry> {
    let busy = DeviceOps(ctx).device_is_busy_untracked(&volume.id);
    let rename = volume.clone();
    let mut items = Vec::new();
    if !volume.mount_points.is_empty() {
        let open = volume.id.clone();
        let new_tab = open.clone();
        items.push(menu_item_entry(
            ctx,
            "open",
            Some(IconName::Folder),
            Some(Command::Open),
            busy,
            move |c| DeviceOps(c).open_volume(&open, false),
        ));
        items.push(menu_item_entry(
            ctx,
            Command::OpenInNewTab.label_id(),
            Some(IconName::Folder),
            Some(Command::OpenInNewTab),
            busy,
            move |c| DeviceOps(c).open_volume(&new_tab, true),
        ));
    }
    if volume.can_set_label() {
        items.push(menu_item_entry(
            ctx,
            "rename",
            Some(IconName::TextCursor),
            Some(Command::Rename),
            busy,
            move |c| DeviceOps(c).rename_volume(rename.clone()),
        ));
    }
    if volume.mount_points.is_empty() {
        let id = volume.id.clone();
        let enabled = volume.mountable && !volume.locked && !busy;
        items.push(menu_item_entry(
            ctx,
            "device-mount",
            Some(IconName::Plug),
            Some(Command::MountVolume),
            !enabled,
            move |c| change_volume(c, volumes::DeviceAction::Mount(id.clone())),
        ));
    } else if volume.can_unmount() {
        let id = volume.id.clone();
        items.push(menu_item_entry(
            ctx,
            "device-unmount",
            Some(IconName::Unplug),
            Some(Command::UnmountVolume),
            busy,
            move |c| change_volume(c, volumes::DeviceAction::Unmount(id.clone())),
        ));
    }
    let snapshot = ctx.0.volumes.snapshot_untracked();
    if let Some(drive) = volume.drive.as_ref().and_then(|id| snapshot.drive(id)) {
        if drive.can_eject {
            let id = volume.id.clone();
            items.push(menu_item_entry(
                ctx,
                "device-eject",
                Some(IconName::Unplug),
                None,
                busy,
                move |c| change_volume(c, volumes::DeviceAction::Eject(id.clone())),
            ));
        }
        if drive.can_power_off {
            let id = volume.id.clone();
            items.push(menu_item_entry(
                ctx,
                "device-power-off",
                Some(IconName::CircleX),
                None,
                busy,
                move |c| change_volume(c, volumes::DeviceAction::PowerOff(id.clone())),
            ));
        }
    }
    items
}

fn remote_entries(ctx: &Context, entry: &gds::service::remote::Entry) -> Vec<MenuEntry> {
    let (selected, remote_busy) = ctx.read_untracked(|state| {
        let tab = state.tab();
        (
            tab.selected(),
            state.operation.busy()
                || tab.navigation.is_loading()
                || ctx.is_background_task_busy_untracked(&remote_task_key(tab.id)),
        )
    });
    let count = selected.len();
    let many = count > 1;
    let all_dirs = !selected.is_empty()
        && selected.iter().all(|item| {
            matches!(
                item,
                Row::Remote(rem) if rem.kind == gds::service::remote::EntryKind::Directory
            )
        });
    let is_dir = entry.kind == gds::service::remote::EntryKind::Directory && all_dirs;
    let copy_label = plural_label(count, Command::Copy.label_id(), "copy-selected-files");
    let copy_paths_label = plural_label(count, "copy-path", "copy-selected-paths");
    let delete_label = plural_label(
        count,
        Command::DeletePermanent.label_id(),
        "delete-selected",
    );

    let mut items = Vec::new();
    if !many {
        items.push(command_item(ctx, Command::Open, |c| {
            Dispatch(c).open_selected();
        }));
    }
    if is_dir {
        items.push(labeled_item(
            ctx,
            plural_label(
                count,
                Command::OpenInNewTab.label_id(),
                "open-directories-new-tabs",
            ),
            command_icon(Command::OpenInNewTab),
            Some(Command::OpenInNewTab),
            false,
            open_selected_tab,
        ));
    }
    if !items.is_empty() {
        items.push(MenuEntry::Separator);
    }
    items.extend([
        labeled_item(
            ctx,
            copy_label,
            command_icon(Command::Copy),
            Some(Command::Copy),
            remote_busy,
            |c| LocalOps(c).copy(false),
        ),
        paste_item(ctx),
        labeled_item(
            ctx,
            copy_paths_label,
            command_icon(Command::CopyPaths),
            Some(Command::CopyPaths),
            false,
            copy_selected_paths,
        ),
        MenuEntry::Separator,
        menu_item_entry(
            ctx,
            "remote-download",
            Some(IconName::Download),
            None,
            remote_busy,
            |c| RemoteOps(c).download_selected_remote(),
        ),
        menu_item_entry(
            ctx,
            "new-folder",
            Some(IconName::FolderPlus),
            Some(Command::NewFolder),
            remote_busy,
            |c| LocalOps(c).new_folder(),
        ),
        menu_item_entry(
            ctx,
            Command::Rename.label_id(),
            command_icon(Command::Rename),
            Some(Command::Rename),
            remote_busy || count != 1,
            |c| Dispatch(c).rename(),
        ),
        MenuEntry::Item {
            label: delete_label,
            icon: command_icon(Command::DeletePermanent),
            shortcut: Some(Command::DeletePermanent.shortcut()),
            disabled: remote_busy,
            destructive: true,
            on_select: menu_action(ctx, |c| Dispatch(c).delete(true)),
        },
        MenuEntry::Separator,
        menu_item_entry(
            ctx,
            Command::Favorite.label_id(),
            command_icon(Command::Favorite),
            Some(Command::Favorite),
            remote_busy,
            |c| Navigator(c).favorite(),
        ),
        menu_item_entry(
            ctx,
            "remote-disconnect",
            Some(IconName::Unplug),
            None,
            remote_busy,
            |c| RemoteOps(c).disconnect_remote(),
        ),
    ]);
    items
}

fn trash_entries(ctx: &Context, selection: bool) -> Vec<MenuEntry> {
    let trash_busy = ctx.is_background_task_busy_untracked(TRASH_TASK_KEY);
    let (count, loading, has_rows) = ctx.read_untracked(|state| {
        let tab = state.tab();
        (
            tab.navigation.view().selection.len(),
            tab.navigation.is_loading(),
            matches!(&tab.contents, Contents::Trash(entries) if !entries.is_empty()),
        )
    });
    let busy = trash_busy || loading;
    let mut entries = Vec::new();
    if selection {
        let restore = if count == 1 {
            i18n::text_untracked("recycle-restore")
        } else {
            i18n::count_untracked("recycle-restore-selected", count)
        };
        for (label, icon, command, purge) in [
            (restore, IconName::Undo2, Command::RestoreSelection, false),
            (
                i18n::count_untracked("recycle-purge-selected", count),
                IconName::Trash,
                Command::PurgeSelection,
                true,
            ),
        ] {
            let weak = ctx.downgrade();
            entries.push(MenuEntry::Item {
                label,
                icon: Some(icon),
                shortcut: Some(command.shortcut()),
                disabled: busy || count == 0,
                destructive: purge,
                on_select: UnsyncCallback::new(move |()| {
                    if let Some(ctx) = weak.upgrade() {
                        LocalOps(&ctx).restore_trash(purge, false);
                    }
                }),
            });
        }
        entries.push(MenuEntry::Separator);
    }
    entries.extend([
        command_item(ctx, Command::Refresh, |c| Navigator(c).refresh()),
        MenuEntry::Separator,
        menu_item_entry(
            ctx,
            "recycle-empty-action",
            Some(IconName::Broom),
            Some(Command::EmptyTrash),
            busy || !has_rows,
            |c| LocalOps(c).restore_trash(true, true),
        ),
    ]);
    entries
}

fn place_entries(ctx: &Context, row: &Row) -> Vec<MenuEntry> {
    let Some(location) = place_location(row) else {
        return Vec::new();
    };
    let local = location.local_path();
    let favorite = ctx.read_untracked(|state| state.places.is_favorite(&location));
    let recent =
        ctx.read_untracked(|state| matches!(state.tab().navigation.location(), Location::Recent));
    let target = location.clone();
    let new_tab = location.clone();
    let mut entries = vec![
        command_item(ctx, Command::Open, move |c| {
            Navigator(c).open(target.clone());
        }),
        command_item(ctx, Command::OpenInNewTab, move |c| {
            Navigator(c).new_tab(Some(new_tab.clone()));
        }),
    ];
    if local.is_some() {
        entries.push(menu_item_entry(
            ctx,
            "tags-show-parent",
            None,
            None,
            false,
            show_selected_parent,
        ));
    }
    entries.push(MenuEntry::Separator);
    if local.is_some() && !recent {
        entries.push(command_item(ctx, Command::Copy, |c| {
            LocalOps(c).copy(false)
        }));
    }
    entries.push(command_item(ctx, Command::CopyPaths, copy_selected_paths));
    entries.push(MenuEntry::Separator);
    entries.push(favorite_toggle_entry(ctx, location.clone(), favorite));
    if let Some(path) = local
        && Inspection(ctx).can_edit_item_properties()
    {
        let edit_path = path.to_owned();
        entries.push(menu_item_entry(
            ctx,
            "details-edit-properties-menu",
            Some(IconName::Pencil),
            None,
            false,
            move |c| Inspection(c).edit_path_properties(edit_path.clone(), None),
        ));
    }
    if matches!(row, Row::Tag(_)) {
        let tag = row.clone();
        entries.push(menu_item_entry(
            ctx,
            "tags-delete",
            Some(IconName::Trash),
            Some(Command::Trash),
            false,
            move |c| TagOps(c).delete_tags(vec![tag.clone()]),
        ));
    }
    if !recent {
        entries.push(MenuEntry::Separator);
        entries.push(command_item(ctx, Command::Refresh, |c| {
            Navigator(c).refresh();
        }));
    }
    entries
}

fn recent_file_entries(ctx: &Context, entry: &Entry) -> Vec<MenuEntry> {
    let (selected, picker, busy) = ctx.read_untracked(|state| {
        let tab = state.tab();
        (
            tab.selected(),
            state.picker.is_some(),
            state.operation.busy() || tab.navigation.is_loading(),
        )
    });
    let count = selected.len();
    let many = count > 1;
    let is_dir = FileIcon::is_directory(entry);
    let selected_paths: Vec<PathBuf> = selected
        .iter()
        .filter_map(|row| row.path().map(Path::to_path_buf))
        .collect();
    let location = Location::from(entry.path.as_path());
    let favorite = ctx.read_untracked(|state| state.places.is_favorite(&location));

    let mut entries = Vec::new();
    if !many {
        entries.push(command_item(ctx, Command::Open, |c| {
            Dispatch(c).open_selected();
        }));
        if !is_dir && !picker {
            entries.push(menu_item_entry(
                ctx,
                "open-with",
                Some(IconName::ExternalLink),
                None,
                false,
                open_selected_with,
            ));
        }
        entries.push(menu_item_entry(
            ctx,
            "tags-show-parent",
            Some(IconName::FolderOpen),
            None,
            false,
            show_selected_parent,
        ));
        entries.push(MenuEntry::Separator);
    }
    entries.push(labeled_item(
        ctx,
        plural_label(count, Command::Copy.label_id(), "copy-selected-files"),
        command_icon(Command::Copy),
        Some(Command::Copy),
        busy,
        |c| LocalOps(c).copy(false),
    ));
    entries.push(labeled_item(
        ctx,
        plural_label(count, "copy-path", "copy-selected-paths"),
        command_icon(Command::CopyPaths),
        Some(Command::CopyPaths),
        selected_paths.iter().any(|p| p.to_str().is_none()),
        copy_selected_paths,
    ));
    entries.push(MenuEntry::Separator);
    if !many {
        entries.push(favorite_toggle_entry(ctx, location, favorite));
        if entry.kind != EntryKind::SymbolicLink && Inspection(ctx).can_edit_item_properties() {
            entries.push(edit_properties_entry(ctx, entry, busy));
        }
        entries.push(MenuEntry::Separator);
    }
    let remove_paths = selected_paths;
    entries.push(menu_item_entry(
        ctx,
        "recent-remove",
        Some(IconName::X),
        None,
        busy || remove_paths.is_empty(),
        move |c| LocalOps(c).remove_from_recent(remove_paths.clone()),
    ));
    entries.push(labeled_item(
        ctx,
        plural_label(count, Command::Trash.label_id(), "trash-selected"),
        command_icon(Command::Trash),
        Some(Command::Trash),
        busy,
        |c| Dispatch(c).delete(false),
    ));
    entries
}

fn file_entries(ctx: &Context, entry: &Entry) -> Vec<MenuEntry> {
    let (selected, picker, busy, current_dir) = ctx.read_untracked(|state| {
        let tab = state.tab();
        (
            tab.selected(),
            state.picker.is_some(),
            state.operation.busy() || tab.navigation.is_loading(),
            tab.navigation.path().map(ToOwned::to_owned),
        )
    });
    let many = selected.len() > 1;
    let count = selected.len();
    let unavailable = selected
        .iter()
        .filter_map(Row::path)
        .any(|path| ctx.0.volumes.removal_in_progress(path));
    let directories = selected
        .iter()
        .all(|row| matches!(row, Row::File(file) if FileIcon::is_directory(file)));

    let action = |command: Command, plural: &'static str, run: fn(&Context), disabled: bool| {
        let label = if many {
            i18n::count_untracked(plural, count)
        } else if command == Command::CopyPaths {
            i18n::text_untracked("copy-path")
        } else {
            i18n::text_untracked(command.label_id())
        };
        labeled_item(
            ctx,
            label,
            command_icon(command),
            Some(command),
            disabled || (command != Command::CopyPaths && unavailable),
            run,
        )
    };

    let mut entries = Vec::new();
    if !many {
        entries.push(action(
            Command::Open,
            "open",
            |c| Dispatch(c).open_selected(),
            false,
        ));
        if !directories && !picker {
            entries.push(menu_item_entry(
                ctx,
                "open-with",
                Some(IconName::ExternalLink),
                None,
                unavailable,
                open_selected_with,
            ));
        }
    }
    if directories {
        entries.push(action(
            Command::OpenInNewTab,
            "open-directories-new-tabs",
            open_selected_tab,
            false,
        ));
    }
    if !entries.is_empty() {
        entries.push(MenuEntry::Separator);
    }
    entries.push(action(
        Command::Copy,
        "copy-selected-files",
        |c| LocalOps(c).copy(false),
        busy,
    ));
    entries.push(action(
        Command::Cut,
        "cut-selected-files",
        |c| LocalOps(c).copy(true),
        busy,
    ));
    let paste_dir_unavailable = current_dir
        .as_ref()
        .is_none_or(|path| ctx.0.volumes.removal_in_progress(path));
    if !many {
        let paste_disabled =
            busy || paste_dir_unavailable || !Clipboard::current().can_paste_untracked();
        entries.push(menu_item_entry(
            ctx,
            "paste-files",
            Some(IconName::ClipboardPaste),
            Some(Command::Paste),
            paste_disabled,
            |c| Dispatch(c).paste(),
        ));
    }
    entries.push(MenuEntry::Separator);
    entries.push(action(
        Command::CopyPaths,
        "copy-selected-paths",
        copy_selected_paths,
        selected
            .iter()
            .any(|row| row.path().is_some_and(|p| p.to_str().is_none())),
    ));
    entries.push(MenuEntry::Separator);
    if !many {
        entries.push(action(
            Command::Rename,
            "rename",
            |c| Dispatch(c).rename(),
            busy || !cfg!(any(target_os = "linux", target_os = "freebsd"))
                || entry.path.file_name().is_none(),
        ));
        #[cfg(unix)]
        if current_dir.is_some() {
            let entry_link = entry.path.clone();
            entries.push(menu_item_entry(
                ctx,
                "create-symlink",
                Some(IconName::Link),
                None,
                busy || unavailable || paste_dir_unavailable,
                move |c| LocalOps(c).create_symlink(entry_link.clone()),
            ));
        }
        if entry.kind != EntryKind::SymbolicLink && Inspection(ctx).can_edit_item_properties() {
            entries.push(edit_properties_entry(ctx, entry, busy || unavailable));
        }
        entries.push(MenuEntry::Separator);
    }
    entries.push(action(
        Command::Trash,
        "trash-selected",
        |c| Dispatch(c).delete(false),
        busy,
    ));
    entries.push(action(
        Command::DeletePermanent,
        "delete-selected",
        |c| Dispatch(c).delete(true),
        busy,
    ));
    if !many && ctx.0.preferences.snapshot().terminal_enabled() {
        let target = if directories {
            Some(entry.path.as_path())
        } else {
            entry.path.parent()
        };
        if let Some(target) = target {
            entries.push(MenuEntry::Separator);
            if !picker {
                let path = target.to_owned();
                entries.push(menu_item_entry(
                    ctx,
                    "files-terminal-open-here",
                    Some(IconName::Terminal),
                    None,
                    unavailable,
                    move |c| TerminalDock(c).new_terminal_at(path.clone()),
                ));
            }
            let path = target.to_owned();
            entries.push(menu_item_entry(
                ctx,
                "terminal-open-external",
                Some(IconName::Terminal),
                None,
                unavailable,
                move |c| open_external_terminal(c, path.clone()),
            ));
        }
    }
    entries
}
