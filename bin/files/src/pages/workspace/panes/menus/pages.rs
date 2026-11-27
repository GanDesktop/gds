//! Page-level toolbar action strip and page-tools menu entries for special and directory surfaces.

use gds::i18n;
use gds::ui::IconName;
use gds::ui::button::IconButtonProps;
use gds::ui::menu::MenuEntry;
use zgui::prelude::*;
use zgui::reactive::UnsyncCallback;
use zgui::view;

use super::super::inspection::Inspection;
use super::actions::{change_volume, place_location};
use super::columns::{column_menu, sort_entries};
use crate::{
    entities::{
        file::ViewMode,
        location::{Location, volumes},
    },
    pages::workspace::{
        navigation::Navigator,
        operations::{DeviceOps, Dispatch, LocalOps, RemoteOps},
        projection::{Geometry, Rows, UiState},
        state::{
            context::Context,
            model::{Contents, Row, TRASH_TASK_KEY, remote_task_key},
        },
    },
    shared::ui::actions::Command,
};

struct Action {
    label: &'static str,
    icon: IconName,
    command: Option<Command>,
    run: fn(&Context),
    enabled: bool,
    checked: bool,
}

impl Action {
    fn new(
        label: &'static str,
        icon: IconName,
        command: Command,
        run: fn(&Context),
        enabled: bool,
    ) -> Self {
        Self {
            label,
            icon,
            command: Some(command),
            run,
            enabled,
            checked: false,
        }
    }

    fn custom(label: &'static str, icon: IconName, run: fn(&Context), enabled: bool) -> Self {
        Self {
            label,
            icon,
            command: None,
            run,
            enabled,
            checked: false,
        }
    }

    fn checked(self, checked: bool) -> Self {
        Self { checked, ..self }
    }
}

fn page_menu_item(
    ctx: &Context,
    label_id: &'static str,
    icon: IconName,
    command: Command,
    disabled: bool,
    action: fn(&Context),
) -> MenuEntry {
    let weak = ctx.downgrade();
    MenuEntry::Item {
        label: i18n::text_untracked(label_id),
        icon: Some(icon),
        shortcut: Some(command.shortcut()),
        disabled,
        destructive: false,
        on_select: UnsyncCallback::new(move |()| {
            if let Some(ctx) = weak.upgrade() {
                ctx.0.owner.with(|| action(&ctx));
            }
        }),
    }
}

fn special_actions(ctx: &Context) -> Option<Vec<Action>> {
    if ctx.read_structure(|state| {
        matches!(
            state.tab().navigation.location(),
            Location::Directory(_) | Location::Volume { .. }
        )
    }) {
        return None;
    }
    let (
        tab_id,
        location,
        contents_loaded,
        loading,
        file_busy,
        grid,
        selected,
        empty,
        reloading,
        pure_tag_catalogue,
        in_picker,
    ) = ctx.read(|state| {
        let tab = state.tab();
        let loading = tab.navigation.is_loading();
        let contents_loaded = match tab.contents {
            Contents::LoadingCollection | Contents::Unavailable(_) => false,
            Contents::Remote { .. } => true,
            _ => !matches!(tab.navigation.location(), Location::Remote(_)),
        };
        (
            tab.id,
            tab.navigation.location().clone(),
            contents_loaded,
            loading,
            state.operation.busy(),
            tab.mode == ViewMode::Grid,
            tab.selected(),
            tab.rows.is_empty(),
            state.favorites_reload.is_some(),
            tab.is_pure_tag_catalogue(),
            state.picker.is_some(),
        )
    });
    let trash_busy = ctx.is_background_task_busy(TRASH_TASK_KEY) || loading;
    let busy = file_busy || loading;
    let one = if let [row] = selected.as_slice() {
        Some(row)
    } else {
        None
    };
    let refresh = |enabled| {
        Action::new(
            "refresh",
            IconName::RefreshCw,
            Command::Refresh,
            |c| Navigator(c).refresh(),
            enabled,
        )
    };
    let actions = match location {
        Location::Trash => vec![
            Action::new(
                "recycle-restore",
                IconName::Undo2,
                Command::RestoreSelection,
                |c| LocalOps(c).restore_trash(false, false),
                !trash_busy && !selected.is_empty(),
            ),
            Action::new(
                "delete-permanently",
                IconName::Trash,
                Command::PurgeSelection,
                |c| LocalOps(c).restore_trash(true, false),
                !trash_busy && !selected.is_empty(),
            ),
            refresh(!trash_busy),
            Action::new(
                "recycle-empty-action",
                IconName::Broom,
                Command::EmptyTrash,
                |c| LocalOps(c).restore_trash(true, true),
                !trash_busy && !empty,
            ),
        ],
        Location::Volumes => {
            let snapshot = ctx.0.volumes.snapshot();
            let devices = DeviceOps(ctx);
            let volume = one.and_then(|row| match row {
                Row::Volume(volume) if !devices.device_is_busy(&volume.id) => Some(volume),
                _ => None,
            });
            let drive = volume
                .and_then(|vol| vol.drive.as_ref())
                .and_then(|id| snapshot.drive(id));
            let mut actions = vec![
                Action::new(
                    "rename",
                    IconName::TextCursor,
                    Command::Rename,
                    |c| Dispatch(c).rename(),
                    volume.is_some_and(volumes::Volume::can_set_label),
                ),
                Action::new(
                    "device-mount",
                    IconName::Plug,
                    Command::MountVolume,
                    mount_selected_volume,
                    volume.is_some_and(|vol| {
                        vol.mountable && !vol.locked && vol.mount_points.is_empty()
                    }),
                ),
                Action::new(
                    "device-unmount",
                    IconName::Unplug,
                    Command::UnmountVolume,
                    unmount_selected_volume,
                    volume.is_some_and(volumes::Volume::can_unmount),
                ),
            ];
            if drive.is_some_and(|d| d.can_eject) {
                actions.push(Action::custom(
                    "device-eject",
                    IconName::Unplug,
                    eject_selected_volume,
                    volume.is_some(),
                ));
            }
            if drive.is_some_and(|d| d.can_power_off) {
                actions.push(Action::custom(
                    "device-power-off",
                    IconName::CircleX,
                    power_off_selected_volume,
                    volume.is_some(),
                ));
            }
            actions.push(refresh(true));
            actions
        }
        Location::Network => {
            let mut actions = vec![refresh(!loading)];
            if !in_picker {
                actions.push(Action::custom(
                    "remote-add-location",
                    IconName::Plus,
                    |c| RemoteOps(c).connect_server(),
                    !ctx.0.connector.is_connecting(),
                ));
            }
            actions
        }
        Location::Remote(_) if !contents_loaded => Vec::new(),
        Location::Remote(_) => {
            let remote_busy = busy || ctx.is_background_task_busy(&remote_task_key(tab_id));
            let favorite = ctx.read(|state| state.places.is_favorite(&location));
            let show_hidden = ctx.0.preferences.state().with(|p| p.files.show_hidden);
            vec![
                Action::new(
                    if grid { "view-list" } else { "view-grid" },
                    if grid {
                        IconName::List
                    } else {
                        IconName::LayoutGrid
                    },
                    Command::ViewMode,
                    |c| Navigator(c).toggle_view(),
                    !remote_busy,
                ),
                Action::new(
                    "show-hidden",
                    IconName::Eye,
                    Command::Hidden,
                    toggle_remote_hidden,
                    !remote_busy,
                )
                .checked(show_hidden),
                Action::custom(
                    "remote-upload",
                    IconName::Upload,
                    |c| RemoteOps(c).upload_remote(),
                    !remote_busy,
                ),
                Action::custom(
                    "remote-download",
                    IconName::Download,
                    |c| RemoteOps(c).download_selected_remote(),
                    !remote_busy && !selected.is_empty(),
                ),
                Action::new(
                    "new-folder",
                    IconName::FolderPlus,
                    Command::NewFolder,
                    |c| LocalOps(c).new_folder(),
                    !remote_busy,
                ),
                Action::new(
                    "rename",
                    IconName::Pencil,
                    Command::Rename,
                    |c| Dispatch(c).rename(),
                    !remote_busy && one.is_some(),
                ),
                Action::new(
                    "delete-permanently",
                    IconName::Trash,
                    Command::DeletePermanent,
                    |c| Dispatch(c).delete(true),
                    !remote_busy && !selected.is_empty(),
                ),
                Action::new(
                    if favorite {
                        "place-favorite-remove"
                    } else {
                        "place-favorite-add"
                    },
                    IconName::Star,
                    Command::Favorite,
                    |c| Navigator(c).favorite(),
                    !remote_busy,
                )
                .checked(favorite),
                refresh(!remote_busy),
                Action::custom(
                    "remote-disconnect",
                    IconName::Unplug,
                    |c| RemoteOps(c).disconnect_remote(),
                    !remote_busy,
                ),
            ]
        }
        Location::Recent | Location::Favorites | Location::Tags(_) => {
            let selected_place = one.and_then(place_location);
            let recent = location == Location::Recent;
            let can_toggle_page_fav = matches!(location, Location::Tags(_));
            let target_place = selected_place
                .clone()
                .or_else(|| can_toggle_page_fav.then(|| location.clone()));
            let favorite = target_place
                .as_ref()
                .is_some_and(|place| ctx.read(|state| state.places.is_favorite(place)));
            let details_permitted = Geometry(ctx).details_permitted();
            let details_visible = ctx.0.details.get();
            let has_selected_tag = selected.iter().any(|row| matches!(row, Row::Tag(_)));

            let mut actions = vec![
                Action::new(
                    if grid { "view-list" } else { "view-grid" },
                    if grid {
                        IconName::List
                    } else {
                        IconName::LayoutGrid
                    },
                    Command::ViewMode,
                    |c| Navigator(c).toggle_view(),
                    contents_loaded && !busy,
                ),
                Action::new(
                    if favorite {
                        "place-favorite-remove"
                    } else {
                        "place-favorite-add"
                    },
                    IconName::Star,
                    Command::Favorite,
                    toggle_collection_favorite,
                    target_place.is_some(),
                )
                .checked(favorite),
                Action::new(
                    "details-toggle",
                    IconName::Info,
                    Command::Details,
                    |c| Inspection(c).toggle_details(),
                    details_permitted && !pure_tag_catalogue,
                )
                .checked(details_visible && details_permitted && !pure_tag_catalogue),
                refresh(!busy && !reloading),
            ];
            if recent {
                actions.push(Action::new(
                    "clear-recent",
                    IconName::Broom,
                    Command::ClearRecent,
                    |c| LocalOps(c).clear_recent(),
                    !empty,
                ));
            } else if has_selected_tag {
                actions.push(Action::new(
                    "tags-delete",
                    IconName::Trash,
                    Command::Trash,
                    |c| Dispatch(c).delete(false),
                    !busy,
                ));
            }
            actions
        }
        _ => return None,
    };
    Some(actions)
}

fn special_page_entries(ctx: &Context) -> Option<Vec<MenuEntry>> {
    Some(
        special_actions(ctx)?
            .into_iter()
            .map(|action| {
                let weak = ctx.downgrade();
                let run = action.run;
                let on_select = UnsyncCallback::new(move |()| {
                    if let Some(ctx) = weak.upgrade() {
                        ctx.0.owner.with(|| run(&ctx));
                    }
                });
                if action.checked && action.command != Some(Command::Favorite) {
                    MenuEntry::Checkbox {
                        label: i18n::text(action.label),
                        shortcut: action.command.map(Command::shortcut),
                        checked: true,
                        disabled: !action.enabled,
                        on_toggle: UnsyncCallback::new(move |_| on_select.run(())),
                    }
                } else {
                    let icon = if action.checked && action.command == Some(Command::Favorite) {
                        IconName::StarOff
                    } else {
                        action.icon
                    };
                    MenuEntry::Item {
                        label: i18n::text(action.label),
                        icon: Some(icon),
                        shortcut: action.command.map(Command::shortcut),
                        disabled: !action.enabled,
                        destructive: false,
                        on_select,
                    }
                }
            })
            .collect(),
    )
}

pub(super) fn page_entries(ctx: &Context) -> Vec<MenuEntry> {
    let geometry = Geometry(ctx);
    let ui_state = UiState(ctx);
    let include_filter = geometry.height_tier_untracked() == gds::ui::window::HeightTier::Micro
        && ui_state.filter_available_untracked();
    if let Some(mut entries) = special_page_entries(ctx) {
        if include_filter {
            entries.insert(
                0,
                page_menu_item(
                    ctx,
                    "filter",
                    IconName::Search,
                    Command::Filter,
                    false,
                    |c| UiState(c).activate_filter(),
                ),
            );
        }
        let sort = sort_entries(ctx);
        if !sort.is_empty() {
            entries.push(MenuEntry::Submenu {
                label: i18n::text_untracked("sort"),
                icon: Some(IconName::ArrowDownWideNarrow),
                entries: sort,
            });
        }
        return entries;
    }

    let grid = ctx.read_untracked(|state| state.tab().mode == ViewMode::Grid);
    let favorite =
        ctx.read_untracked(|state| state.places.is_favorite(state.tab().navigation.location()));
    let columns = Rows(ctx).configurable_columns_untracked();
    let show_hidden = ctx.0.preferences.snapshot().files.show_hidden;
    let details_disabled =
        !geometry.details_permitted_untracked() || !ui_state.details_available_structure();

    let mut entries = Vec::new();
    if include_filter {
        entries.push(page_menu_item(
            ctx,
            "filter",
            IconName::Search,
            Command::Filter,
            false,
            |c| UiState(c).activate_filter(),
        ));
    }
    entries.push(page_menu_item(
        ctx,
        if grid { "view-list" } else { "view-grid" },
        if grid {
            IconName::List
        } else {
            IconName::LayoutGrid
        },
        Command::ViewMode,
        false,
        |c| Navigator(c).toggle_view(),
    ));
    entries.push(page_menu_item(
        ctx,
        if favorite {
            "favorite-remove"
        } else {
            "favorite-add"
        },
        if favorite {
            IconName::StarOff
        } else {
            IconName::Star
        },
        Command::Favorite,
        false,
        |c| Navigator(c).favorite(),
    ));
    entries.push(page_menu_item(
        ctx,
        "columns-configure",
        IconName::Columns3Cog,
        Command::Columns,
        grid || !columns,
        |c| Rows(c).configure_columns(),
    ));
    let toggle_hidden = {
        let weak = ctx.downgrade();
        move || {
            if let Some(ctx) = weak.upgrade() {
                ctx.0.owner.with(|| {
                    let visible = ctx.0.preferences.snapshot().files.show_hidden;
                    ctx.edit_preferences(|preferences| {
                        preferences.files.show_hidden = !visible;
                    });
                    Navigator(&ctx).refresh();
                });
            }
        }
    };
    entries.push(if show_hidden {
        MenuEntry::Checkbox {
            label: i18n::text_untracked(Command::Hidden.label_id()),
            shortcut: Some(Command::Hidden.shortcut()),
            checked: true,
            disabled: false,
            on_toggle: UnsyncCallback::new(move |_| toggle_hidden()),
        }
    } else {
        MenuEntry::Item {
            label: i18n::text_untracked(Command::Hidden.label_id()),
            icon: Some(IconName::Eye),
            shortcut: Some(Command::Hidden.shortcut()),
            disabled: false,
            destructive: false,
            on_select: UnsyncCallback::new(move |()| toggle_hidden()),
        }
    });
    entries.push(page_menu_item(
        ctx,
        Command::Details.label_id(),
        IconName::Info,
        Command::Details,
        details_disabled,
        |c| Inspection(c).toggle_details(),
    ));
    entries.push(page_menu_item(
        ctx,
        Command::Refresh.label_id(),
        IconName::RefreshCw,
        Command::Refresh,
        false,
        |c| Navigator(c).refresh(),
    ));
    let sort = sort_entries(ctx);
    if !sort.is_empty() {
        entries.push(MenuEntry::Submenu {
            label: i18n::text_untracked("sort"),
            icon: Some(IconName::ArrowDownWideNarrow),
            entries: sort,
        });
    }
    entries
}

pub(super) fn special_page_actions(ctx: &Context) -> Option<AnyView> {
    let actions = special_actions(ctx)?;
    let is_remote = ctx.read(|state| {
        matches!(state.tab().navigation.location(), Location::Remote(_))
            && matches!(state.tab().contents, Contents::Remote { .. })
    });
    let mut buttons = Vec::with_capacity(actions.len() + usize::from(is_remote));
    if is_remote {
        buttons.push(column_menu(ctx));
    }
    buttons.extend(actions.into_iter().map(|action| {
        let click_ctx = ctx.clone();
        let run = action.run;
        AnyView::new(view! {
            IconButton(
                icon = action.icon,
                label = i18n::text(action.label),
                active = Signal::stored_local(action.checked),
                disabled = Signal::stored_local(!action.enabled),
                shortcut = action.command.map(Command::shortcut),
                on:click = move |_| {
                    if action.enabled {
                        run(&click_ctx);
                    }
                }
            )
        })
    }));
    Some(AnyView::new(view! {
        row(class = "gds-files-page-action-strip") {{buttons}}
    }))
}

fn toggle_remote_hidden(ctx: &Context) {
    ctx.edit_preferences(|prefs| {
        prefs.files.show_hidden = !prefs.files.show_hidden;
    });
}

fn toggle_collection_favorite(ctx: &Context) {
    let selected_place = ctx.read_untracked(|state| {
        let selected = state.tab().selected();
        if let [row] = selected.as_slice() {
            place_location(row)
        } else {
            None
        }
    });
    if let Some(place) = selected_place {
        ctx.update(|state| {
            state.places.toggle_favorite(place);
        });
        ctx.save_places();
        Navigator(ctx).refresh_location(&Location::Favorites);
    } else {
        Navigator(ctx).favorite();
    }
}

fn selected_volume(ctx: &Context) -> Option<volumes::Volume> {
    ctx.read_untracked(|state| {
        state
            .tab()
            .selected()
            .into_iter()
            .find_map(|row| match row {
                Row::Volume(volume) => Some(volume),
                _ => None,
            })
    })
}

fn selected_or_current_volume(ctx: &Context) -> Option<volumes::Volume> {
    selected_volume(ctx).or_else(|| {
        ctx.read_untracked(|state| match state.tab().navigation.location() {
            Location::Volume { identity, .. } => Rows(ctx).volume_at_location_untracked(identity),
            _ => None,
        })
    })
}

fn mount_selected_volume(ctx: &Context) {
    if let Some(volume) = selected_or_current_volume(ctx)
        && volume.mount_points.is_empty()
        && volume.mountable
        && !volume.locked
    {
        change_volume(ctx, volumes::DeviceAction::Mount(volume.id));
    }
}

fn unmount_selected_volume(ctx: &Context) {
    if let Some(volume) = selected_or_current_volume(ctx)
        && volume.can_unmount()
    {
        change_volume(ctx, volumes::DeviceAction::Unmount(volume.id));
    }
}

fn eject_selected_volume(ctx: &Context) {
    if let Some(volume) = selected_volume(ctx) {
        change_volume(ctx, volumes::DeviceAction::Eject(volume.id));
    }
}

fn power_off_selected_volume(ctx: &Context) {
    if let Some(volume) = selected_volume(ctx) {
        change_volume(ctx, volumes::DeviceAction::PowerOff(volume.id));
    }
}
