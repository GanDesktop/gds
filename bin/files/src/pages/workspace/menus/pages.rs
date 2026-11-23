use gds::i18n;
use gds::ui::IconName;
use gds::ui::button::IconButtonProps;
use gds::ui::menu::MenuEntry;
use zgui::prelude::*;
use zgui::reactive::UnsyncCallback;
use zgui::view;

use crate::entities::{
    file::ViewMode,
    location::{Location, volumes},
};
use crate::pages::workspace::{
    model::{Contents, Row, TRASH_TASK_KEY, remote_task_key},
    ui::Browser,
};
use crate::shared::ui::actions::Command;

struct Action {
    label: &'static str,
    icon: IconName,
    command: Option<Command>,
    run: fn(&Browser),
    enabled: bool,
    checked: bool,
}

impl Action {
    fn new(
        label: &'static str,
        icon: IconName,
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

    fn custom(label: &'static str, icon: IconName, run: fn(&Browser), enabled: bool) -> Self {
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
        if self.read_structure(|state| {
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
        ) = self.read(|state| {
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
        let trash_busy = self.is_background_task_busy(TRASH_TASK_KEY) || loading;
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
                Browser::refresh,
                enabled,
            )
        };
        let actions = match location {
            Location::Trash => vec![
                Action::new(
                    "recycle-restore",
                    IconName::Undo2,
                    Command::RestoreSelection,
                    |browser| browser.restore_trash(false, false),
                    !trash_busy && !selected.is_empty(),
                ),
                Action::new(
                    "delete-permanently",
                    IconName::Trash,
                    Command::PurgeSelection,
                    |browser| browser.restore_trash(true, false),
                    !trash_busy && !selected.is_empty(),
                ),
                refresh(!trash_busy),
                Action::new(
                    "recycle-empty-action",
                    IconName::Broom,
                    Command::EmptyTrash,
                    |browser| browser.restore_trash(true, true),
                    !trash_busy && !empty,
                ),
            ],
            Location::Volumes => {
                let snapshot = self.0.volumes.snapshot();
                let volume = one.and_then(|row| match row {
                    Row::Volume(volume) if !self.device_is_busy(&volume.id) => Some(volume),
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
                        Browser::rename,
                        volume.is_some_and(volumes::Volume::can_set_label),
                    ),
                    Action::new(
                        "device-mount",
                        IconName::Plug,
                        Command::MountVolume,
                        Browser::mount_selected_volume,
                        volume.is_some_and(|vol| {
                            vol.mountable && !vol.locked && vol.mount_points.is_empty()
                        }),
                    ),
                    Action::new(
                        "device-unmount",
                        IconName::Unplug,
                        Command::UnmountVolume,
                        Browser::unmount_selected_volume,
                        volume.is_some_and(volumes::Volume::can_unmount),
                    ),
                ];
                if drive.is_some_and(|d| d.can_eject) {
                    actions.push(Action::custom(
                        "device-eject",
                        IconName::Unplug,
                        Browser::eject_selected_volume,
                        volume.is_some(),
                    ));
                }
                if drive.is_some_and(|d| d.can_power_off) {
                    actions.push(Action::custom(
                        "device-power-off",
                        IconName::CircleX,
                        Browser::power_off_selected_volume,
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
                        Browser::connect_server,
                        !self.0.connector.is_connecting(),
                    ));
                }
                actions
            }
            Location::Remote(_) if !contents_loaded => Vec::new(),
            Location::Remote(_) => {
                let remote_busy = busy || self.is_background_task_busy(&remote_task_key(tab_id));
                let favorite = self.read(|state| state.places.is_favorite(&location));
                let show_hidden = self.0.preferences.state().with(|p| p.files.show_hidden);
                vec![
                    Action::new(
                        if grid { "view-list" } else { "view-grid" },
                        if grid {
                            IconName::List
                        } else {
                            IconName::LayoutGrid
                        },
                        Command::ViewMode,
                        Browser::toggle_view,
                        !remote_busy,
                    ),
                    Action::new(
                        "show-hidden",
                        IconName::Eye,
                        Command::Hidden,
                        Browser::toggle_remote_hidden,
                        !remote_busy,
                    )
                    .checked(show_hidden),
                    Action::custom(
                        "remote-upload",
                        IconName::Upload,
                        Browser::upload_remote,
                        !remote_busy,
                    ),
                    Action::custom(
                        "remote-download",
                        IconName::Download,
                        Browser::download_selected_remote,
                        !remote_busy && !selected.is_empty(),
                    ),
                    Action::new(
                        "new-folder",
                        IconName::FolderPlus,
                        Command::NewFolder,
                        Browser::new_folder,
                        !remote_busy,
                    ),
                    Action::new(
                        "rename",
                        IconName::Pencil,
                        Command::Rename,
                        Browser::rename,
                        !remote_busy && one.is_some(),
                    ),
                    Action::new(
                        "delete-permanently",
                        IconName::Trash,
                        Command::DeletePermanent,
                        |browser| browser.delete(true),
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
                        Browser::favorite,
                        !remote_busy,
                    )
                    .checked(favorite),
                    refresh(!remote_busy),
                    Action::custom(
                        "remote-disconnect",
                        IconName::Unplug,
                        Browser::disconnect_remote,
                        !remote_busy,
                    ),
                ]
            }
            Location::Recent | Location::Favorites | Location::Tags(_) => {
                let selected_place = one.and_then(Self::place_location);
                let recent = location == Location::Recent;
                let can_toggle_page_fav = matches!(location, Location::Tags(_));
                let target_place = selected_place
                    .clone()
                    .or_else(|| can_toggle_page_fav.then(|| location.clone()));
                let favorite = target_place
                    .as_ref()
                    .is_some_and(|place| self.read(|state| state.places.is_favorite(place)));
                let details_permitted = self.details_permitted();
                let details_visible = self.0.details.get();
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
                        Browser::toggle_view,
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
                        Browser::toggle_collection_favorite,
                        target_place.is_some(),
                    )
                    .checked(favorite),
                    Action::new(
                        "details-toggle",
                        IconName::Info,
                        Command::Details,
                        Browser::toggle_details,
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
                        |browser| browser.dispatch(Command::ClearRecent),
                        !empty,
                    ));
                } else if has_selected_tag {
                    actions.push(Action::new(
                        "tags-delete",
                        IconName::Trash,
                        Command::Trash,
                        |browser| browser.delete(false),
                        !busy,
                    ));
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
                    let weak = self.downgrade();
                    let run = action.run;
                    let on_select = UnsyncCallback::new(move |()| {
                        if let Some(browser) = weak.upgrade() {
                            browser.0.owner.with(|| run(&browser));
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

    pub(in crate::pages::workspace) fn special_page_actions(&self) -> Option<AnyView> {
        let actions = self.special_actions()?;
        let is_remote = self.read(|state| {
            matches!(state.tab().navigation.location(), Location::Remote(_))
                && matches!(state.tab().contents, Contents::Remote { .. })
        });
        let mut buttons = Vec::with_capacity(actions.len() + usize::from(is_remote));
        if is_remote {
            buttons.push(self.column_menu());
        }
        buttons.extend(actions.into_iter().map(|action| {
            let b_click = self.clone();
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
                            run(&b_click);
                        }
                    }
                )
            })
        }));
        Some(AnyView::new(view! {
            row(class = "gds-files-page-action-strip") {{buttons}}
        }))
    }

    fn toggle_remote_hidden(&self) {
        self.dispatch(Command::Hidden);
    }

    fn toggle_collection_favorite(&self) {
        let selected_place = self.read_untracked(|state| {
            let selected = state.tab().selected();
            if let [row] = selected.as_slice() {
                Self::place_location(row)
            } else {
                None
            }
        });
        if let Some(place) = selected_place {
            self.update(|state| {
                state.places.toggle_favorite(place);
            });
            self.save_places();
            self.refresh_location(&Location::Favorites);
        } else {
            self.favorite();
        }
    }

    fn mount_selected_volume(&self) {
        self.dispatch(Command::MountVolume);
    }

    fn unmount_selected_volume(&self) {
        self.dispatch(Command::UnmountVolume);
    }

    fn eject_selected_volume(&self) {
        let volume = self.read_untracked(|state| {
            state
                .tab()
                .selected()
                .into_iter()
                .find_map(|row| match row {
                    Row::Volume(volume) => Some(volume),
                    _ => None,
                })
        });
        if let Some(volume) = volume {
            self.change_volume(volumes::DeviceAction::Eject(volume.id));
        }
    }

    fn power_off_selected_volume(&self) {
        let volume = self.read_untracked(|state| {
            state
                .tab()
                .selected()
                .into_iter()
                .find_map(|row| match row {
                    Row::Volume(volume) => Some(volume),
                    _ => None,
                })
        });
        if let Some(volume) = volume {
            self.change_volume(volumes::DeviceAction::PowerOff(volume.id));
        }
    }
}
