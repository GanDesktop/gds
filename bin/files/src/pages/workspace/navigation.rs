use super::model::{Row, Tab};
use super::{model::*, ui::Browser};
use crate::{
    entities::{
        file::{ViewMode, directory},
        location::{
            Location, VolumeIdentity,
            navigation::{self, LoadOutcome, LoadRequest},
            volumes,
        },
        tags,
    },
    features::file_operations::recycle,
};
use anti_webui::prelude::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

enum WatchEvent {
    Inspect(tokio::sync::oneshot::Sender<Option<Arc<directory::DirectorySnapshot>>>),
    Apply {
        before: Arc<directory::DirectorySnapshot>,
        snapshot: Arc<directory::DirectorySnapshot>,
    },
    Failed(String),
}

pub(super) enum Loaded {
    Unavailable(String),
    Directory(Option<Arc<directory::DirectorySnapshot>>),
    Places(Vec<Location>),
    Network(Vec<PathBuf>),
    Trash(Vec<recycle::Entry>),
    Tags {
        listing: tags::Listing,
        backend: tags::Baloo,
        entries: Vec<directory::Entry>,
    },
    Volumes,
}

impl Browser {
    pub(super) fn resolve_path(&self, location: &Location) -> Option<PathBuf> {
        match location {
            Location::Directory(path) => Some(path.into()),
            Location::Volume { identity, relative } => {
                let volume = self.volume_at_location(identity)?;
                let preferred = volume.id.stable.as_ref().and_then(|id| {
                    self.0
                        .preferences
                        .snapshot()
                        .files
                        .volume_mount_points
                        .get(id)
                        .cloned()
                });
                preferred
                    .as_ref()
                    .filter(|preferred| volume.mount_points.contains(preferred))
                    .or_else(|| {
                        volume.mount_points.iter().min_by(|left, right| {
                            left.components()
                                .count()
                                .cmp(&right.components().count())
                                .then_with(|| left.cmp(right))
                        })
                    })
                    .map(|root| root.join(relative))
            }
            _ => None,
        }
    }

    pub(super) fn volume_at_location(&self, identity: &VolumeIdentity) -> Option<volumes::Volume> {
        let snapshot = self.0.volumes.snapshot();
        let mut matching = snapshot
            .volumes
            .into_iter()
            .filter(|volume| match identity {
                VolumeIdentity::Stable(id) => volume.id.stable.as_ref() == Some(id),
                VolumeIdentity::Attachment(id) => &volume.id.attachment == id,
            });
        let volume = matching.next()?;
        matching.next().is_none().then_some(volume)
    }

    pub(super) fn volume_identity(
        snapshot: &volumes::Snapshot,
        volume: &volumes::VolumeId,
    ) -> VolumeIdentity {
        if let Some(stable) = &volume.stable
            && snapshot
                .volumes
                .iter()
                .filter(|candidate| candidate.id.stable.as_ref() == Some(stable))
                .count()
                == 1
        {
            return VolumeIdentity::Stable(stable.clone());
        }
        VolumeIdentity::Attachment(volume.attachment.clone())
    }

    pub(super) fn location_title(&self, location: &Location) -> String {
        if let Location::Volume { identity, relative } = location
            && relative.is_empty()
            && let Some(volume) = self.volume_at_location(identity)
        {
            return volume.name;
        }
        location.label()
    }

    pub(super) fn location_address(location: &Location, path: Option<&Path>) -> String {
        path.map_or_else(
            || location.display_address(),
            |path| path.display().to_string(),
        )
    }

    pub(super) fn local_location(&self, path: &Path) -> Location {
        let snapshot = self.0.volumes.snapshot();
        if let Some(volume) = snapshot.at_path(path)
            && !volume.system_mount
            && let Some(root) = volume
                .mount_points
                .iter()
                .filter(|root| path.starts_with(root))
                .max_by_key(|root| root.components().count())
            && let Ok(relative) = path.strip_prefix(root)
        {
            return Location::Volume {
                identity: Self::volume_identity(&snapshot, &volume.id),
                relative: relative.as_os_str().to_owned(),
            };
        }
        Location::from(path)
    }
    pub fn open(&self, location: Location) {
        self.open_in_tab(self.read(|state| state.tab().id), location);
    }
    pub(super) fn open_in_tab(&self, id: u64, location: Location) {
        let (location, path) = if let Location::Directory(path) = location {
            let path = PathBuf::from(path);
            (self.local_location(&path), Some(path))
        } else {
            let path = self.resolve_path(&location);
            (location, path)
        };
        if matches!(location, Location::Volume { .. }) && path.is_none() {
            self.report_notice(gds::i18n::text("device-not-mounted"));
            return;
        }
        let options = directory::ReadOptions {
            show_hidden: self.0.preferences.snapshot().files.show_hidden,
        };
        let request = self.update(|state| {
            let active = state.tab().id == id;
            let tab = state.tab_by_id(id)?;
            if active {
                tab.navigation.view_mut().scroll = self.0.list.scroll_offset().y;
            }
            Some(tab.navigation.prepare_open(location, path, options))
        });
        if let Some(request) = request {
            self.load(id, request);
        }
    }

    pub(super) fn open_address(&self) {
        let input = self.0.address.get_untracked();
        let result = if crate::entities::location::scheme::recognizes(&input) {
            Location::from_uri(&input).map_err(|error| error.to_string())
        } else {
            self.read(|state| {
                let current = state.tab().navigation.path().unwrap_or(&state.home);
                if input == current.to_string_lossy() {
                    Ok(current.to_owned())
                } else {
                    navigation::resolve_input(&input, current, Some(&state.home))
                }
            })
            .map(Location::from)
            .map_err(|error| error.to_string())
        };
        match result {
            Ok(location) => {
                self.finish_address_edit();
                self.open(location);
                self.focus_content_after_mount();
            }
            Err(error) => self.report_notice(error),
        }
    }

    pub fn refresh(&self) {
        if self.read(|state| matches!(state.tab().contents, Contents::Volumes)) {
            self.0.volumes.refresh();
        }
        self.refresh_tab(self.read(|state| state.tab().id));
    }
    pub(super) fn reveal_paths(&self, paths: Vec<PathBuf>) {
        self.update(|state| state.tab_mut().reveal = paths);
    }
    pub(super) fn refresh_tab(&self, id: u64) {
        let Some(location) = self.read(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.navigation.location().clone())
        }) else {
            return;
        };
        let path = self.resolve_path(&location);
        let reveal_hidden = self.read(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .is_some_and(|tab| {
                    tab.reveal.iter().any(|path| {
                        path.file_name()
                            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
                    })
                })
        });
        let options = directory::ReadOptions {
            show_hidden: self.0.preferences.snapshot().files.show_hidden || reveal_hidden,
        };
        let request = self.update(|state| {
            let tab = state.tab_by_id(id)?;
            Some(if tab.navigation.revision() == 0 {
                tab.navigation.prepare_initial(
                    path,
                    options,
                    matches!(tab.initial, InitialVisit::Explicit),
                )
            } else {
                tab.navigation.prepare_refresh(path, options)
            })
        });
        if let Some(request) = request {
            self.load(id, request);
        }
    }

    pub(super) fn history(&self, forward: bool) {
        let target = self.read(|state| {
            let tab = state.tab();
            let nav = &tab.navigation;
            let index = if forward {
                nav.history_index().checked_add(1)
            } else {
                nav.history_index().checked_sub(1)
            }?;
            nav.history()
                .get(index)
                .map(|visit| (tab.id, index, visit.location.clone()))
        });
        if let Some((id, index, location)) = target {
            self.visit_history(id, index, &location);
        }
    }

    pub(super) fn visit_history(&self, id: u64, index: usize, location: &Location) {
        let path = self.resolve_path(location);
        let options = directory::ReadOptions {
            show_hidden: self.0.preferences.snapshot().files.show_hidden,
        };
        let request = self.update(|state| {
            let tab = state.tab_mut();
            if tab.id != id
                || tab
                    .navigation
                    .history()
                    .get(index)
                    .is_none_or(|visit| &visit.location != location)
            {
                return None;
            }
            tab.navigation
                .prepare_history(index, path, options)
                .map(|request| (tab.id, request))
        });
        if let Some((id, request)) = request {
            self.load(id, request);
        }
    }

    pub(super) fn cancel_location_load(&self) {
        let pending = self.update(|state| {
            let tab = state.tab_mut();
            let pending = tab.navigation.pending_location().cloned();
            tab.navigation.cancel_pending();
            tab.cancel();
            if matches!(tab.contents, Contents::LoadingCollection) {
                tab.contents = if matches!(tab.navigation.location(), Location::Network) {
                    Contents::Network {
                        mounts: Vec::new(),
                        error: None,
                    }
                } else {
                    Contents::Unavailable(gds::i18n::text("load-cancelled"))
                };
            }
            pending
        });
        if let Some(Location::Remote(address)) = pending
            && self.0.connector.pending().is_some_and(|(pending, intent)| {
                pending == address && intent == crate::features::connect_server::Intent::Open
            })
        {
            self.0.connector.cancel();
        }
        self.sync_address();
    }

    pub(super) fn parent(&self) {
        let (current, path) = self.read(|state| {
            let navigation = &state.tab().navigation;
            (
                navigation.location().clone(),
                navigation.path().map(Path::to_owned),
            )
        });
        let parent = match current {
            Location::Volume { identity, relative } => {
                Path::new(&relative)
                    .parent()
                    .map(|relative| Location::Volume {
                        identity,
                        relative: relative.as_os_str().to_owned(),
                    })
            }
            other => crate::entities::location::scheme::parent(&other),
        }
        .or_else(|| path.as_deref().and_then(Path::parent).map(Location::from));
        if let Some(parent) = parent {
            self.open(parent);
        }
    }

    pub(super) fn new_tab(&self, location: Option<Location>) {
        self.update(|state| {
            let location = location.unwrap_or_else(|| state.tab().navigation.location().clone());
            let id = state.next_tab;
            state.next_tab += 1;
            state
                .tabs
                .push(Tab::new(id, location, &state.places, false));
            state.active = state.tabs.len() - 1;
        });
        self.refresh();
    }
    pub(super) fn select_tab(&self, index: usize) {
        let should_load = self.update(|state| {
            if index >= state.tabs.len() || index == state.active {
                return false;
            }
            if self.0.list.logical_rect().is_some() {
                state.tab_mut().navigation.view_mut().scroll = self.0.list.scroll_offset().y;
            }
            state.active = index;
            !state.tab().navigation.is_loading()
                && state.tab().navigation.directory().is_none()
                && state.tab().rows.is_empty()
        });
        self.sync_address();
        if should_load {
            self.refresh();
        }
    }
    pub(super) fn tab_index(&self, id: u64) -> Option<usize> {
        self.read(|state| state.tabs.iter().position(|tab| tab.id == id))
    }
    pub(super) fn close_tab(&self, index: usize) {
        let Some((id, pinned, location)) = self.read(|state| {
            state
                .tabs
                .get(index)
                .map(|tab| (tab.id, tab.pinned, tab.navigation.location().clone()))
        }) else {
            return;
        };
        if pinned {
            let browser = self.clone();
            let name = self.location_title(&location);
            self.confirm(
                gds::i18n::text("unpin-close-title"),
                gds::t!("unpin-close-message", name = name),
                move || browser.close_tab_approved(id),
            );
        } else {
            self.close_tab_approved(id);
        }
    }
    fn close_tab_approved(&self, id: u64) {
        self.update(|state| {
            let Some(index) = state.tabs.iter().position(|tab| tab.id == id) else {
                return;
            };
            let active = state.tab().id;
            let removed = state.tabs.remove(index);
            if removed.pinned {
                state
                    .places
                    .set_pinned(removed.navigation.location(), false);
            }
            if state.tabs.is_empty() {
                let id = state.next_tab;
                state.next_tab += 1;
                state.tabs.push(Tab::new(
                    id,
                    Location::from(&state.home),
                    &state.places,
                    false,
                ));
            }
            state.active = state
                .tabs
                .iter()
                .position(|tab| tab.id == active)
                .unwrap_or_else(|| index.min(state.tabs.len() - 1));
        });
        self.sync_address();
        self.save_places();
        if self.read(|state| state.tab().navigation.revision() == 0) {
            self.refresh();
        }
        self.focus_content_after_mount();
    }
    pub(super) fn toggle_pin(&self, index: usize) {
        self.update(|state| {
            if index >= state.tabs.len() {
                return;
            }
            let active = state.tab().id;
            let mut tab = state.tabs.remove(index);
            tab.pinned = !tab.pinned;
            state
                .places
                .set_pinned(tab.navigation.location(), tab.pinned);
            let pinned = state.tabs.iter().take_while(|tab| tab.pinned).count();
            state.tabs.insert(pinned, tab);
            state.active = state
                .tabs
                .iter()
                .position(|tab| tab.id == active)
                .expect("pinning preserves active tab");
            state.places.reorder_pinned(
                state
                    .tabs
                    .iter()
                    .filter(|tab| tab.pinned)
                    .map(|tab| tab.navigation.location().clone())
                    .collect(),
            );
        });
        self.save_places();
    }
    pub(super) fn reorder_tab(&self, from: usize, to: usize) {
        self.update(|state| {
            if from >= state.tabs.len() || to >= state.tabs.len() {
                return;
            }
            let active = state.tab().id;
            let tab = state.tabs.remove(from);
            state.tabs.insert(to, tab);
            state.active = state
                .tabs
                .iter()
                .position(|tab| tab.id == active)
                .expect("active tab remains after reorder");
            state.places.reorder_pinned(
                state
                    .tabs
                    .iter()
                    .filter(|tab| tab.pinned)
                    .map(|tab| tab.navigation.location().clone())
                    .collect(),
            );
        });
        self.save_places();
    }

    fn load(&self, id: u64, request: LoadRequest) {
        if let Location::Remote(address) = request.location() {
            self.load_remote(id, request.clone(), address.clone());
            return;
        }
        if matches!(request.location(), Location::Favorites) {
            self.refresh_favorites();
        }
        let preferences = self.0.preferences.snapshot();
        let tags_disabled = gds::i18n::text("tags-disabled");
        let tasks = self.0.tasks.named_child("Browse location");
        let cancel = tasks.token();
        self.update(|state| {
            if let Some(tab) = state.tab_by_id(id) {
                tab.cancel();
                tab.work = Some(tasks);
                if matches!(
                    request.location(),
                    Location::Trash | Location::Tags(_) | Location::Network
                ) {
                    let switching = tab.navigation.location() != request.location()
                        || matches!(tab.contents, Contents::Directory);
                    tab.navigation.present_collection(&request);
                    if switching {
                        tab.contents = Contents::LoadingCollection;
                        tab.rows.clear();
                    }
                }
            }
        });
        let scope = self
            .read(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .and_then(|tab| tab.work.as_ref())
                    .map(|owner| (**owner).clone())
            })
            .expect("load belongs to current tab");
        let weak = self.downgrade();
        let partial_request = request.clone();
        let partial = self.0.owner.with(|| {
            ui_channel(move |snapshot| {
                if let Some(browser) = weak.upgrade() {
                    browser.update(|state| {
                        if let Some(tab) = state.tab_by_id(id)
                            && !matches!(
                                tab.navigation
                                    .apply_partial_load(&partial_request, snapshot),
                                crate::entities::location::navigation::PartialLoadOutcome::Stale
                            )
                        {
                            tab.contents = Contents::Directory;
                        }
                    });
                    browser.reproject(id);
                }
            })
        });
        let worker_request = request.clone();
        let stream = preferences.files.streaming.options();
        let collection = self.read(|state| match request.location() {
            Location::Recent => Some(state.places.recent.clone()),
            Location::Favorites => Some(state.places.favorites.clone()),
            _ => None,
        });
        self.work(
            "Load Files location",
            async move {
                if let Some(locations) = collection {
                    return Ok(Loaded::Places(locations));
                }
                match worker_request.location() {
                    Location::Network => scope
                        .spawn_blocking_named(
                            "Discover mounted network folders",
                            crate::entities::location::volumes::read_network_mounts,
                        )
                        .await
                        .map_err(|error| error.to_string())?
                        .map(Loaded::Network)
                        .map_err(|error| error.to_string()),
                    Location::Trash => scope
                        .spawn_blocking_named("Read trash", || {
                            recycle::list()
                                .map(Loaded::Trash)
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?,
                    Location::Tags(address) => {
                        if !preferences.tags.enabled {
                            return Ok(Loaded::Unavailable(tags_disabled));
                        }
                        let backend = tags::connect(&preferences.tags.commands).await?;
                        let listing = backend
                            .browse(address.tag())
                            .await
                            .map_err(|error| error.to_string())?;
                        let files = listing.files.clone();
                        let entries = scope
                            .spawn_blocking_named("Inspect tagged files", move || {
                                files
                                    .iter()
                                    .map(|path| {
                                        directory::Entry::inspect(path)
                                            .map_err(|error| error.to_string())
                                    })
                                    .collect::<Result<Vec<_>, _>>()
                            })
                            .await
                            .map_err(|error| error.to_string())??;
                        Ok(Loaded::Tags {
                            listing,
                            backend,
                            entries,
                        })
                    }
                    Location::Volumes => Ok(Loaded::Volumes),
                    _ => scope
                        .spawn_blocking_named("Read directory", move || {
                            worker_request
                                .load_base_streaming(stream, &cancel, move |snapshot| {
                                    partial.send(snapshot)
                                })
                                .map(Loaded::Directory)
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?,
                }
            },
            move |browser, result| browser.finish_load(id, request, result),
        );
    }

    fn finish_load(&self, id: u64, request: LoadRequest, result: Result<Loaded, String>) {
        let result = self.update(|state| {
            let current_places = match request.location() {
                Location::Favorites => Some(state.places.favorites.clone()),
                Location::Recent => Some(state.places.recent.clone()),
                _ => None,
            };
            let tab = state.tab_by_id(id)?;
            if !tab.navigation.is_current_request(&request) {
                return None;
            }
            let loaded = match result {
                Ok(loaded) => loaded,
                Err(error) if matches!(request.location(), Location::Network) => {
                    let mounts =
                        match std::mem::replace(&mut tab.contents, Contents::LoadingCollection) {
                            Contents::Network { mounts, .. } => mounts,
                            _ => Vec::new(),
                        };
                    tab.contents = Contents::Network {
                        mounts,
                        error: Some(error),
                    };
                    tab.navigation.cancel_request(&request);
                    return Some(Ok(()));
                }
                Err(error) if matches!(request.location(), Location::Trash | Location::Tags(_)) => {
                    tab.navigation.cancel_request(&request);
                    tab.contents = Contents::Unavailable(error);
                    return Some(Ok(()));
                }
                Err(error) => {
                    tab.navigation.cancel_request(&request);
                    return Some(Err(error));
                }
            };
            let snapshot = match &loaded {
                Loaded::Directory(snapshot) => snapshot.clone(),
                _ => None,
            };
            match tab.navigation.finish_load(&request, Ok(snapshot)) {
                Ok(LoadOutcome::Applied) => {}
                Ok(LoadOutcome::Stale) => return None,
                Err(error) => return Some(Err(error.to_string())),
            }
            tab.contents = match loaded {
                Loaded::Unavailable(message) => Contents::Unavailable(message),
                Loaded::Directory(_) => Contents::Directory,
                Loaded::Places(places) => Contents::Places(current_places.unwrap_or(places)),
                Loaded::Network(mounts) => Contents::Network {
                    mounts,
                    error: None,
                },
                Loaded::Trash(entries) => Contents::Trash(entries),
                Loaded::Tags {
                    listing,
                    backend,
                    entries,
                } => Contents::Tags {
                    listing,
                    backend,
                    entries,
                },
                Loaded::Volumes => Contents::Volumes,
            };
            if request.records_recent() {
                state.places.record(request.location().clone());
            }
            Some(Ok(()))
        });
        match result {
            Some(Err(error)) => self.report_notice(error),
            Some(Ok(())) => {
                if let Some(snapshot) = self.read(|state| {
                    state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == id)?
                        .navigation
                        .directory_arc()
                }) {
                    self.report_directory_warnings(&snapshot);
                }
                self.reproject(id);
                if self.read(|state| state.tab().id == id) {
                    self.sync_address();
                }
                self.save_places();
                self.watch_directory(id);
                self.enrich_directory(id);
            }
            None => {}
        }
    }

    fn report_directory_warnings(&self, snapshot: &directory::DirectorySnapshot) {
        if snapshot.warnings.is_empty() {
            return;
        }
        let mut message = gds::i18n::count("warning-count", snapshot.warnings.len());
        for warning in &snapshot.warnings {
            tracing::warn!(
                path = %warning.path.display(), operation = ?warning.operation,
                error_kind = ?warning.source.kind(), "{}", warning.source
            );
            let path = warning
                .path
                .as_os_str()
                .to_string_lossy()
                .escape_debug()
                .to_string();
            message.push_str(&format!("\n{path}: {}", warning.source));
        }
        self.report_notice(message);
    }

    fn refresh_favorites(&self) {
        let work = {
            let mut state = self.0.state.borrow_mut();
            if state.favorites_reload.is_some() || state.saving_places || state.places.has_pending()
            {
                return;
            }
            let Some(work) = state.places.reload() else {
                return;
            };
            state.favorites_reload = Some(state.places.favorites.clone());
            work
        };
        self.work(
            "Reload saved favorites",
            async move { work.await.map_err(|error| error.to_string()) },
            |browser, result| {
                let ids = browser.update(|state| {
                    let Some(previous) = state.favorites_reload.take() else {
                        return Vec::new();
                    };
                    let Ok(snapshot) = result.as_ref() else {
                        return Vec::new();
                    };
                    if state.places.favorites != previous
                        || state.saving_places
                        || state.places.has_pending()
                    {
                        return Vec::new();
                    }
                    state.places.refresh_favorites(snapshot.clone());
                    let favorites = state.places.favorites.clone();
                    state
                        .tabs
                        .iter_mut()
                        .filter(|tab| matches!(tab.navigation.location(), Location::Favorites))
                        .map(|tab| {
                            tab.contents = Contents::Places(favorites.clone());
                            tab.id
                        })
                        .collect::<Vec<_>>()
                });
                for id in ids {
                    browser.reproject(id);
                }
                if let Err(error) = result {
                    browser.report_notice(error);
                }
            },
        );
    }
    pub(super) fn sortable_columns(&self) -> Vec<crate::entities::file::columns::Column> {
        use crate::entities::file::columns::Column;
        self.read(|state| match state.tab().contents {
            Contents::Directory => Column::ALL.to_vec(),
            _ => Vec::new(),
        })
    }
    pub(super) fn reproject(&self, id: u64) {
        let volumes = self.0.volumes.snapshot();
        let show_hidden = self.0.preferences.snapshot().files.show_hidden;
        self.update(|state| {
            let picking = state.picker.is_some();
            let picker = state.picker.as_ref().map(|picker| picker.projection());
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            let filter = tab.navigation.view().filter.to_lowercase();
            let rows = match &tab.contents {
                Contents::Unavailable(_)
                | Contents::LoadingCollection
                | Contents::Network { .. } => Vec::new(),
                Contents::Directory => {
                    let entries = tab
                        .navigation
                        .directory()
                        .map(|directory| directory.entries.as_ref())
                        .unwrap_or(&[]);
                    let mut visible: Vec<_> = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| {
                            entry.matches_filter(&filter)
                                && picker.as_ref().is_none_or(|picker| picker.keeps(entry))
                        })
                        .map(|(index, _)| index)
                        .collect();
                    tab.sort.sort_indices(entries, &mut visible);
                    visible
                        .into_iter()
                        .map(|index| Row::File(entries[index].clone()))
                        .collect::<Vec<_>>()
                }
                Contents::Places(locations) => locations
                    .iter()
                    .filter(|location| !picking || !matches!(location, Location::Remote(_)))
                    .cloned()
                    .map(Row::Place)
                    .collect(),
                Contents::Trash(entries) => entries.iter().cloned().map(Row::Trash).collect(),
                Contents::Tags {
                    listing, entries, ..
                } => {
                    let mut visible = (0..entries.len()).collect::<Vec<_>>();
                    tab.sort.sort_indices(entries, &mut visible);
                    listing
                        .tags
                        .iter()
                        .cloned()
                        .map(Row::Tag)
                        .chain(
                            visible
                                .into_iter()
                                .map(|index| Row::File(entries[index].clone())),
                        )
                        .collect()
                }
                Contents::Remote { listing, .. } => listing
                    .entries
                    .iter()
                    .filter(|entry| show_hidden || !entry.name.starts_with('.'))
                    .cloned()
                    .map(Row::Remote)
                    .collect(),
                Contents::Volumes => volumes
                    .volumes
                    .iter()
                    .filter(|volume| volume.visible)
                    .cloned()
                    .map(Row::Volume)
                    .collect(),
            };
            tab.rows = rows
                .into_iter()
                .filter(|row| {
                    filter.is_empty()
                        || match row {
                            Row::Place(location) => {
                                location.display_address().to_lowercase().contains(&filter)
                            }
                            _ => row.label().to_lowercase().contains(&filter),
                        }
                })
                .collect();
            tab.backing = crate::shared::lib::selection::SelectionBacking::from_items(
                tab.rows.iter().map(Row::key),
            );
            if !tab.navigation.is_loading() {
                let keys: std::collections::HashSet<_> = tab.rows.iter().map(Row::key).collect();
                tab.navigation
                    .view_mut()
                    .selection
                    .retain(|key| keys.contains(key));
            }
            if !tab.navigation.is_loading() && !tab.reveal.is_empty() {
                let reveal = std::mem::take(&mut tab.reveal);
                let selected = tab
                    .rows
                    .iter()
                    .filter(|row| {
                        row.path()
                            .is_some_and(|path| reveal.iter().any(|candidate| candidate == path))
                    })
                    .map(Row::key)
                    .collect();
                tab.navigation.view_mut().selection = selected;
            }
        });
    }

    pub(super) fn sync_address(&self) {
        let (address, filter, scroll, details, widths) = self.read(|state| {
            let tab = state.tab();
            (
                Self::location_address(tab.navigation.location(), tab.navigation.path()),
                tab.navigation.view().filter.clone(),
                tab.navigation.view().scroll,
                state
                    .places
                    .details_visible(tab.navigation.location().clone()),
                state
                    .places
                    .column_widths(tab.navigation.location().clone())
                    .into_iter()
                    .filter_map(|(key, width)| {
                        crate::entities::file::columns::Column::from_id(&key)
                            .map(|column| (column, width))
                    })
                    .collect(),
            )
        });
        batch(|| {
            if self.0.address_presentation.get_untracked() == AddressPresentation::Breadcrumbs {
                self.0.address.set_if_changed(address);
            }
            self.0.filter.set_if_changed(filter);
            self.0.details.set_if_changed(details);
            self.0.column_widths.set_if_changed(widths);
        });
        self.0.list.scroll_to(0.0, scroll);
    }

    pub(super) fn filter(&self, value: String) {
        let id = self.update(|state| {
            let tab = state.tab_mut();
            tab.navigation.view_mut().filter = value;
            tab.reveal.clear();
            tab.id
        });
        self.reproject(id);
        self.update_selection(|state| {
            let tab = state.tab_mut();
            let keys = tab
                .rows
                .iter()
                .map(Row::key)
                .collect::<std::collections::HashSet<_>>();
            let view = tab.navigation.view_mut();
            view.selection.retain(|key| keys.contains(key));
            if view.cursor.as_ref().is_some_and(|key| !keys.contains(key)) {
                view.cursor = None;
            }
            if view.anchor.as_ref().is_some_and(|key| !keys.contains(key)) {
                view.anchor = None;
            }
        });
    }
    pub(super) fn toggle_view(&self) {
        let restore_focus = self.0.list_focused.get();
        self.update(|state| {
            let tab = state.tab_mut();
            tab.mode = match tab.mode {
                ViewMode::List => ViewMode::Grid,
                ViewMode::Grid => ViewMode::List,
            };
            let location = tab.navigation.location().clone();
            let mode = tab.mode;
            state.places.set_view_mode(location, mode);
        });
        if restore_focus {
            self.focus_content_after_mount();
        }
        self.save_places();
    }
    pub(super) fn favorite(&self) {
        let Some(location) = self.read(|state| {
            let tab = state.tab();
            match tab.navigation.location() {
                Location::Recent | Location::Favorites | Location::Tags(_) => {
                    let selected = tab.selected();
                    let [row] = selected.as_slice() else {
                        return None;
                    };
                    Self::place_location(row)
                }
                Location::Network | Location::Volumes | Location::Trash => None,
                location => Some(location.clone()),
            }
        }) else {
            return;
        };
        self.update(|state| state.places.toggle_favorite(location));
        self.save_places();
        self.refresh_location(&Location::Favorites);
    }

    fn watch_directory(&self, id: u64) {
        let data = self.read(|state| {
            let tab = state.tabs.iter().find(|tab| tab.id == id)?;
            Some((
                tab.navigation.directory_arc()?,
                tab.navigation.revision(),
                (**tab.work.as_ref()?).clone(),
            ))
        });
        let Some((initial, revision, scope)) = data else {
            return;
        };
        let config = self.0.preferences.snapshot().files.watching;
        let weak = self.downgrade();
        let updates = self.0.owner.with(|| {
            ui_channel(move |event: WatchEvent| {
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                match event {
                    WatchEvent::Inspect(reply) => {
                        let snapshot = browser.read(|state| {
                            state
                                .tabs
                                .iter()
                                .find(|tab| {
                                    tab.id == id
                                        && tab.navigation.revision() == revision
                                        && !tab.navigation.is_loading()
                                })
                                .and_then(|tab| tab.navigation.directory_arc())
                        });
                        let _ = reply.send(snapshot);
                    }
                    WatchEvent::Apply {
                        before,
                        mut snapshot,
                    } => {
                        let changed = browser.update(|state| {
                            let Some(tab) = state.tab_by_id(id) else {
                                return false;
                            };
                            if tab.navigation.revision() != revision || tab.navigation.is_loading()
                            {
                                return false;
                            }
                            if let Some(current) = tab.navigation.directory_arc() {
                                let entries = Arc::make_mut(&mut snapshot)
                                    .entries
                                    .iter()
                                    .map(|entry| {
                                        let Some(latest) = current.entry_for_path(&entry.path)
                                        else {
                                            return entry.clone();
                                        };
                                        if before
                                            .entry_for_path(&entry.path)
                                            .is_some_and(|base| entry.ptr_eq(base))
                                        {
                                            latest.clone()
                                        } else {
                                            entry.clone().with_reused_probes(latest)
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                Arc::make_mut(&mut snapshot).entries = entries.into();
                            }
                            tab.enrichment = None;
                            tab.navigation.update_directory(revision, snapshot)
                        });
                        if changed {
                            browser.reproject(id);
                            browser.enrich_directory(id);
                        }
                    }
                    WatchEvent::Failed(error) => browser.report_notice(error),
                }
            })
        });
        let cancel = scope.token();
        let io = scope.clone();
        scope.spawn_named("Watch Files directory", async move {
            let mut watch =
                match directory::Watching::with_limit(&initial.path, config.incremental_limit) {
                    Ok(watch) => watch,
                    Err(error) => {
                        updates.send(WatchEvent::Failed(error.to_string()));
                        return;
                    }
                };
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    changed = watch.changed() => if !changed { break; }
                }
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(config.debounce()) => {}
                }
                let changes = watch.take_changes();
                let (reply, latest) = tokio::sync::oneshot::channel();
                updates.send(WatchEvent::Inspect(reply));
                let previous = tokio::select! {
                    _ = cancel.cancelled() => break,
                    latest = latest => match latest {
                        Ok(Some(snapshot)) => snapshot,
                        _ => break,
                    }
                };
                let base = previous.clone();
                let worker_cancel = cancel.clone();
                match io
                    .spawn_blocking_named("Reconcile directory changes", move || {
                        changes.apply_shared(&previous, &worker_cancel)
                    })
                    .await
                {
                    Ok(Ok(Some(snapshot))) => updates.send(WatchEvent::Apply {
                        before: base,
                        snapshot,
                    }),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) if error.is_cancelled() => break,
                    Ok(Err(error)) => updates.send(WatchEvent::Failed(error.to_string())),
                    Err(error) => {
                        updates.send(WatchEvent::Failed(error.to_string()));
                        break;
                    }
                }
            }
        });
    }
}
