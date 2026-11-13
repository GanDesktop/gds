//! Resolve route identities and run the same history transitions for every page.
use super::*;

pub(in crate::pages::workspace) struct ResolvedLocation {
    pub location: Location,
    pub path: Option<PathBuf>,
    pub device: Option<devices::DeviceLocation>,
}

impl Browser {
    pub(in crate::pages::workspace) fn activate_location_button(
        &mut self,
        location: Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.modifiers().control || window.modifiers().platform {
            self.open_location_tab(location, false, Some(window), cx);
        } else {
            self.show_location(location, window, cx);
        }
    }

    pub(in crate::pages::workspace) fn volume_identity(
        &self,
        volume: &storage::VolumeId,
        cx: &App,
    ) -> VolumeIdentity {
        if let Some(stable) = &volume.stable {
            if self
                .devices
                .service
                .read(cx)
                .snapshot
                .volumes
                .iter()
                .filter(|candidate| candidate.id.stable.as_ref() == Some(stable))
                .count()
                == 1
            {
                return VolumeIdentity::Stable(stable.clone());
            }
        }
        VolumeIdentity::Attachment(volume.attachment.clone())
    }

    pub(in crate::pages::workspace) fn resolve_location(
        &self,
        location: &Location,
        cx: &App,
    ) -> ResolvedLocation {
        match location {
            Location::Directory(path) => {
                let path = PathBuf::from(path);
                let device = devices::DeviceLocation::capture(
                    &self.devices.service.read(cx).snapshot,
                    &path,
                );
                let location = device.as_ref().map_or_else(
                    || location.clone(),
                    |device| Location::Volume {
                        identity: self.volume_identity(&device.volume, cx),
                        relative: device.relative.as_os_str().to_owned(),
                    },
                );
                ResolvedLocation {
                    location,
                    path: Some(path),
                    device,
                }
            }
            Location::Volume { identity, relative } => {
                let snapshot = &self.devices.service.read(cx).snapshot;
                let mut matches = snapshot.volumes.iter().filter(|volume| match identity {
                    VolumeIdentity::Stable(id) => volume.id.stable.as_ref() == Some(id),
                    VolumeIdentity::Attachment(id) => &volume.id.attachment == id,
                });
                let volume = matches.next().filter(|_| matches.next().is_none());
                let root = volume.and_then(|volume| self.default_volume_root(volume));
                let available = volume
                    .zip(root.as_ref())
                    .is_some_and(|(volume, root)| volume.mount_points.contains(root));
                let device = devices::DeviceLocation {
                    volume: volume.map(|volume| volume.id.clone()).unwrap_or_else(|| {
                        storage::VolumeId {
                            stable: match identity {
                                VolumeIdentity::Stable(id) => Some(id.clone()),
                                _ => None,
                            },
                            attachment: match identity {
                                VolumeIdentity::Attachment(id) => id.clone(),
                                _ => String::new(),
                            },
                        }
                    }),
                    root: root.clone().unwrap_or_default(),
                    relative: PathBuf::from(relative),
                    availability: if available {
                        devices::Availability::Available
                    } else if volume.is_some() {
                        devices::Availability::Unmounted
                    } else {
                        devices::Availability::Disconnected
                    },
                };
                ResolvedLocation {
                    location: location.clone(),
                    path: available.then(|| root.expect("mounted root").join(relative)),
                    device: Some(device),
                }
            }
            _ => ResolvedLocation {
                location: location.clone(),
                path: None,
                device: None,
            },
        }
    }

    pub(in crate::pages::workspace) fn save_tab_view(&mut self, id: u64, cx: &App) {
        let Some(slot) = self.tab_slot(id) else {
            return;
        };
        let collection = Collection::from_location(slot.navigation.location());
        let scroll = if id == self.tabs.active_id {
            match collection {
                Some(Collection::Volumes) => Some(f32::from(
                    self.devices
                        .table
                        .read(cx)
                        .vertical_scroll_handle
                        .0
                        .borrow()
                        .base_handle
                        .offset()
                        .y,
                )),
                Some(_) if slot.view_mode == ViewMode::Grid => None,
                Some(collection) => self.place_table(collection).map(|table| {
                    f32::from(
                        table
                            .read(cx)
                            .vertical_scroll_handle
                            .0
                            .borrow()
                            .base_handle
                            .offset()
                            .y,
                    )
                }),
                None => None,
            }
        } else {
            None
        };
        let trash = (id == self.tabs.active_id && collection == Some(Collection::Trash))
            .then(|| self.collections.trash.view(cx));
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.save_scroll();
            slot.navigation.view_mut().selected.compact();
            if let Some(scroll) = scroll {
                slot.navigation.view_mut().scroll_offset = scroll;
            }
            if let Some(trash) = trash {
                slot.navigation.view_mut().set_trash(trash);
            }
        }
    }

    pub(in crate::pages::workspace) fn enter_location(&mut self, cx: &mut Context<Self>) {
        if self.collection().is_some() {
            self.discard_marquee();
            self.menus.sort = None;
        }
        let Some(slot) = self.current_tab() else {
            return;
        };
        let offset = slot.navigation.view().scroll_offset;
        match self.collection() {
            Some(Collection::Trash) => {
                let view = slot.navigation.view().trash().cloned().unwrap_or_default();
                self.collections.trash.restore_view(view, cx);
                if !self.collections.trash.is_loading() {
                    self.load_trash(cx);
                }
            }
            Some(Collection::Volumes) => {
                self.devices
                    .table
                    .read(cx)
                    .vertical_scroll_handle
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(offset)));
            }
            Some(collection) => {
                if let Some(table) = self.place_table(collection) {
                    table
                        .read(cx)
                        .vertical_scroll_handle
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(offset)));
                }
                if collection == Collection::Network && !self.collections.network.discovery.loaded {
                    self.refresh_network(cx);
                }
            }
            None => (),
        }
    }

    pub(in crate::pages::workspace) fn navigate_history(
        &mut self,
        command: Command,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.current_tab() else {
            return;
        };
        let index = match command {
            Command::Back => slot.navigation.history_index().checked_sub(1),
            Command::Forward => Some(slot.navigation.history_index() + 1),
            Command::Parent => {
                let location =
                    crate::entities::location::scheme::parent(slot.navigation.location())
                        .or_else(|| slot.path().and_then(Path::parent).map(Location::from));
                if let Some(location) = location {
                    self.navigate_location(self.tabs.active_id, location, cx);
                }
                return;
            }
            _ => return,
        };
        if let Some(index) = index {
            self.navigate_history_entry(self.tabs.active_id, index, cx);
        }
    }

    pub(in crate::pages::workspace) fn navigate_history_entry(
        &mut self,
        tab: u64,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.current_tab().filter(|slot| slot.id == tab) else {
            return;
        };
        if index == slot.navigation.history_index() {
            return;
        }
        let Some(visit) = slot.navigation.history().get(index) else {
            return;
        };
        let location = visit.location.clone();
        if self.remote_view().is_some_and(|view| view.read(cx).busy()) {
            notifications::info(i18n::text("remote-task-running", cx), cx);
            return;
        }
        let resolved = self.resolve_location(&location, cx);
        let options = self.read_options(self.tabs.active_id);
        self.save_tab_view(self.tabs.active_id, cx);
        let Some(slot) = self.current_tab_mut() else {
            return;
        };
        let id = slot.id;
        let request = slot
            .navigation
            .prepare_history(index, resolved.path, options);
        if let Some(request) = request {
            self.start_request(id, request, cx);
        }
        self.toolbar.finish_path_edit();
    }

    pub(in crate::pages::workspace) fn tab_title(&self, slot: &OpenTab, cx: &App) -> String {
        if let Location::Volume { identity, relative } = slot.navigation.location() {
            if relative.is_empty() {
                if let Some(volume) =
                    self.devices
                        .service
                        .read(cx)
                        .snapshot
                        .volumes
                        .iter()
                        .find(|volume| match identity {
                            VolumeIdentity::Stable(id) => volume.id.stable.as_ref() == Some(id),
                            VolumeIdentity::Attachment(id) => &volume.id.attachment == id,
                        })
                {
                    return volume.name.clone();
                }
            }
        }
        slot.title(cx)
    }
}
