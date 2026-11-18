use crate::pages::workspace::{
    model::{Contents, Row},
    ui::Browser,
};
use crate::{
    entities::{
        file::ViewMode,
        location::{Location, volumes},
    },
    shared::ui::actions::Command,
};
use anti_webui::prelude::*;
use gds::ui::{Icon, menu::MenuEntry};
use gds_uim::el;

struct Action {
    label: &'static str,
    icon: Icon,
    command: Option<Command>,
    run: fn(&Browser),
    enabled: bool,
    checked: bool,
}

impl Action {
    fn new(
        label: &'static str,
        icon: Icon,
        command: Command,
        run: fn(&Browser),
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

    fn custom(label: &'static str, icon: Icon, run: fn(&Browser), enabled: bool) -> Self {
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

impl Browser {
    fn special_actions(&self) -> Option<Vec<Action>> {
        let (location, contents_loaded, busy, grid, selected, empty, reloading) =
            self.read(|state| {
                let tab = state.tab();
                let loading = tab.navigation.is_loading();
                let contents_loaded = match tab.contents {
                    Contents::LoadingCollection | Contents::Unavailable(_) => false,
                    Contents::Remote { .. } => true,
                    _ => !matches!(tab.navigation.location(), Location::Remote(_)),
                };
                (
                    tab.navigation.location().clone(),
                    contents_loaded,
                    state.operation.busy() || loading,
                    tab.mode == ViewMode::Grid,
                    tab.selected(),
                    tab.rows.is_empty(),
                    state.favorites_reload.is_some(),
                )
            });
        let one = if let [row] = selected.as_slice() {
            Some(row)
        } else {
            None
        };
        let refresh = |enabled| {
            Action::new(
                "refresh",
                Icon::Refresh,
                Command::Refresh,
                Browser::refresh,
                enabled,
            )
        };
        let actions = match location {
            Location::Trash => vec![
                Action::new(
                    "recycle-restore",
                    Icon::Undo,
                    Command::RestoreSelection,
                    |browser| browser.restore_trash(false, false),
                    !busy && !selected.is_empty(),
                ),
                Action::new(
                    "delete-permanently",
                    Icon::Trash,
                    Command::PurgeSelection,
                    |browser| browser.restore_trash(true, false),
                    !busy && !selected.is_empty(),
                ),
                Action::new(
                    "recycle-empty-action",
                    Icon::Trash,
                    Command::EmptyTrash,
                    |browser| browser.restore_trash(true, true),
                    !busy && !empty,
                ),
                refresh(!busy),
            ],
            Location::Volumes => {
                let volume = one.and_then(|row| match row {
                    Row::Volume(volume) if !self.0.volumes.is_busy(&volume.id) => Some(volume),
                    _ => None,
                });
                vec![
                    Action::new(
                        "rename",
                        Icon::TextCursor,
                        Command::Rename,
                        Browser::rename,
                        volume.is_some_and(volumes::Volume::can_set_label) && !busy,
                    ),
                    Action::new(
                        "device-mount",
                        Icon::Plug,
                        Command::MountVolume,
                        Browser::mount_selected_volume,
                        volume.is_some_and(|volume| {
                            volume.mountable && !volume.locked && volume.mount_points.is_empty()
                        }) && !busy,
                    ),
                    Action::new(
                        "device-unmount",
                        Icon::Unplug,
                        Command::UnmountVolume,
                        Browser::unmount_selected_volume,
                        volume.is_some_and(volumes::Volume::can_unmount) && !busy,
                    ),
                    refresh(true),
                ]
            }
            Location::Network => {
                let mut actions = vec![refresh(
                    !self.read(|state| state.tab().navigation.is_loading()),
                )];
                if self.read(|state| state.picker.is_none()) {
                    actions.push(Action::custom(
                        "remote-add-location",
                        Icon::Add,
                        Browser::connect_server,
                        !self.0.connector.is_connecting(),
                    ));
                }
                actions
            }
            Location::Remote(_) if !contents_loaded => Vec::new(),
            Location::Remote(_) => {
                let favorite = self.read(|state| state.places.is_favorite(&location));
                vec![
                    Action::custom("remote-upload", Icon::Upload, Browser::upload_remote, !busy),
                    Action::custom(
                        "remote-download",
                        Icon::Download,
                        Browser::download_selected_remote,
                        !busy && !selected.is_empty(),
                    ),
                    Action::new(
                        "new-folder",
                        Icon::FolderPlus,
                        Command::NewFolder,
                        Browser::new_folder,
                        !busy,
                    ),
                    Action::new(
                        "rename",
                        Icon::Edit,
                        Command::Rename,
                        Browser::rename,
                        !busy && one.is_some(),
                    ),
                    Action::new(
                        "delete-permanently",
                        Icon::Trash,
                        Command::DeletePermanent,
                        |browser| browser.delete(true),
                        !busy && !selected.is_empty(),
                    ),
                    Action::new(
                        if favorite {
                            "place-favorite-remove"
                        } else {
                            "place-favorite-add"
                        },
                        Icon::Star,
                        Command::Favorite,
                        Browser::favorite,
                        !busy,
                    )
                    .checked(favorite),
                    refresh(!busy),
                    Action::custom(
                        "remote-disconnect",
                        Icon::Unplug,
                        Browser::disconnect_remote,
                        !busy,
                    ),
                ]
            }
            Location::Recent | Location::Favorites | Location::Tags(_) => {
                let place = one.and_then(Self::place_location);
                let recent = location == Location::Recent;
                let favorite = place
                    .as_ref()
                    .is_some_and(|place| self.read(|state| state.places.is_favorite(place)));
                let mut actions = vec![Action::new(
                    if grid { "view-list" } else { "view-grid" },
                    if grid { Icon::List } else { Icon::Grid },
                    Command::ViewMode,
                    Browser::toggle_view,
                    contents_loaded && !busy,
                )];
                if !recent {
                    actions.push(Action::new(
                        "copy-files",
                        Icon::Copy,
                        Command::Copy,
                        |browser| browser.copy(false),
                        place
                            .as_ref()
                            .is_some_and(|place| place.local_path().is_some())
                            && !busy,
                    ));
                }
                actions.extend([
                    Action::new(
                        "copy-paths",
                        Icon::ClipboardCopy,
                        Command::CopyPaths,
                        Browser::copy_selected_paths,
                        place.is_some(),
                    ),
                    Action::new(
                        if favorite {
                            "place-favorite-remove"
                        } else {
                            "place-favorite-add"
                        },
                        Icon::Star,
                        Command::Favorite,
                        Browser::favorite,
                        place.is_some(),
                    )
                    .checked(favorite),
                ]);
                if recent {
                    actions.push(Action::new(
                        "clear-recent",
                        Icon::Trash,
                        Command::ClearRecent,
                        |browser| browser.dispatch(Command::ClearRecent),
                        !empty,
                    ));
                } else {
                    if matches!(one, Some(Row::Tag(_))) {
                        actions.push(Action::new(
                            "tags-delete",
                            Icon::Trash,
                            Command::Trash,
                            |browser| browser.delete(false),
                            !busy,
                        ));
                    }
                    actions.push(refresh(!busy && !reloading));
                }
                actions
            }
            _ => return None,
        };
        Some(actions)
    }

    pub(super) fn special_page_entries(&self) -> Option<Vec<MenuEntry>> {
        Some(
            self.special_actions()?
                .into_iter()
                .map(|action| {
                    let Action {
                        label,
                        icon,
                        command,
                        run,
                        enabled,
                        checked,
                    } = action;
                    let mut item = self
                        .menu_action(label, run)
                        .icon(icon)
                        .disabled_when(move || !enabled);
                    if checked {
                        item = item.checked_when(|| true);
                    }
                    if let Some(command) = command {
                        item = item.shortcut(command.default_stroke());
                    }
                    item.into()
                })
                .collect(),
        )
    }

    pub(in crate::pages::workspace) fn special_page_actions(&self) -> Option<Div> {
        let buttons = self.special_actions()?.into_iter().map(|action| {
            self.control(action.label, action.icon, action.label, action.run)
                .disabled(!action.enabled)
                .active(action.checked)
        });
        Some(el! { .div.{flex; items_center; gap: 8.0_f32}.[..buttons] })
    }

    fn mount_selected_volume(&self) {
        self.dispatch(Command::MountVolume);
    }

    fn unmount_selected_volume(&self) {
        self.dispatch(Command::UnmountVolume);
    }
}
