//! Cross-domain operation dispatchers (open, rename, delete, paste, and column sort).

use std::path::{Path, PathBuf};

use gds::service::desktop::OpenMode;
use zgui::reactive::spawn_local;

use super::super::{
    navigation::{EnrichmentRunner, Navigator},
    projection::Rows,
    state::{
        context::Context,
        model::{Contents, Row, RowKey, TRASH_TASK_KEY},
    },
};
use super::{
    devices::DeviceOps, lifecycle::Lifecycle, local::LocalOps, paste::PasteOps, picker::PickerOps,
    remote::RemoteOps, tags::TagOps,
};
use crate::{
    entities::{
        file::columns::Column,
        location::{Location, scheme},
    },
    features::pick_files,
    shared::{
        i18n,
        lib::clipboard::{self, Clipboard, Payload},
    },
};

pub(in crate::pages::workspace) struct Dispatch<'a>(pub &'a Context);

impl Dispatch<'_> {
    pub(in crate::pages::workspace) fn open_selected(&self) {
        let in_picker = self.0.read_untracked(|state| state.picker.is_some());
        let selected = self.0.read_untracked(|state| {
            let tab = state.tab();
            let view = tab.navigation.view();
            tab.rows
                .iter()
                .enumerate()
                .filter_map(|(idx, row)| {
                    view.selection
                        .contains(&row.key())
                        .then_some((idx, row.clone()))
                })
                .collect::<Vec<_>>()
        });
        if selected.is_empty() {
            if in_picker {
                PickerOps(self.0).accept_picker();
            }
            return;
        }
        if selected.len() == 1 {
            let (index, row) = &selected[0];
            if in_picker
                && let Row::File(entry) = row
                && !pick_files::Projection::is_directory(entry)
            {
                PickerOps(self.0).accept_picker();
                return;
            }
            self.open_row(*index, false);
            return;
        }
        if in_picker {
            PickerOps(self.0).accept_picker();
            return;
        }
        let rows = Rows(self.0);
        let navigator = Navigator(self.0);
        let lifecycle = Lifecycle(self.0);
        for (idx, row) in selected {
            match row {
                Row::File(entry) if pick_files::Projection::is_directory(&entry) => {
                    let location = rows.local_location(&entry.path);
                    navigator.new_tab(Some(location));
                }
                Row::File(entry) => {
                    lifecycle.open_file(entry.path.clone(), OpenMode::DefaultApplication);
                }
                _ => self.open_row(idx, false),
            }
        }
    }

    pub(in crate::pages::workspace) fn open_row(&self, index: usize, new_tab: bool) {
        let (row, in_picker) = self
            .0
            .read_untracked(|state| (state.tab().rows.get(index).cloned(), state.picker.is_some()));
        let Some(row) = row else {
            return;
        };
        let navigator = Navigator(self.0);
        match row {
            Row::File(entry) => {
                if pick_files::Projection::is_directory(&entry) {
                    let location = Rows(self.0).local_location(&entry.path);
                    if new_tab && !in_picker {
                        navigator.new_tab(Some(location));
                    } else {
                        navigator.open(location);
                    }
                } else if in_picker {
                    PickerOps(self.0).activate_picker_path(entry.path.clone());
                } else {
                    Lifecycle(self.0).open_file(entry.path.clone(), OpenMode::DefaultApplication);
                }
            }
            Row::Place(location) => {
                if new_tab && !in_picker {
                    navigator.new_tab(Some(location));
                } else {
                    navigator.open(location);
                }
            }
            Row::Tag(tag) => {
                if let Ok(address) = scheme::TagAddress::new(&tag) {
                    let location = Location::Tags(address);
                    if new_tab && !in_picker {
                        navigator.new_tab(Some(location));
                    } else {
                        navigator.open(location);
                    }
                }
            }
            Row::Remote(entry) => RemoteOps(self.0).activate_remote(entry, new_tab),
            Row::Volume(volume) => DeviceOps(self.0).open_volume(&volume.id, new_tab),
            Row::Trash(_) => {}
        }
    }

    pub(in crate::pages::workspace) fn rename(&self) {
        let target = self.0.read_untracked(|state| {
            if state.operation.busy() {
                return None;
            }
            let selected = state.tab().selected();
            if selected.len() != 1 {
                return None;
            }
            selected.into_iter().next()
        });
        let Some(row) = target else {
            return;
        };
        match row {
            Row::Remote(entry) => RemoteOps(self.0).rename_remote(entry),
            Row::Volume(volume) => DeviceOps(self.0).rename_volume(volume),
            Row::File(entry) => LocalOps(self.0).rename_local(entry),
            Row::Place(_) | Row::Trash(_) | Row::Tag(_) => {}
        }
    }

    pub(in crate::pages::workspace) fn delete(&self, permanent: bool) {
        enum DeleteTarget {
            None,
            Favorites(Vec<Location>),
            Remote(Vec<Row>),
            Trash(bool),
            Tags(Vec<Row>),
            Local(Vec<PathBuf>),
        }
        let target = self.0.read_untracked(|state| {
            let tab = state.tab();
            let selected = tab.selected();
            if selected.is_empty() {
                return DeleteTarget::None;
            }
            if matches!(tab.navigation.location(), Location::Favorites) {
                if permanent {
                    return DeleteTarget::None;
                }
                let bookmarks: Vec<Location> = selected
                    .into_iter()
                    .filter_map(|row| match row {
                        Row::Place(loc) => Some(loc),
                        _ => None,
                    })
                    .collect();
                return if bookmarks.is_empty() {
                    DeleteTarget::None
                } else {
                    DeleteTarget::Favorites(bookmarks)
                };
            }
            if matches!(tab.contents, Contents::Trash(_)) {
                return if self.0.is_background_task_busy_untracked(TRASH_TASK_KEY) {
                    DeleteTarget::None
                } else {
                    DeleteTarget::Trash(false)
                };
            }
            if state.operation.busy() {
                return DeleteTarget::None;
            }
            match tab.contents {
                Contents::Remote { .. } => DeleteTarget::Remote(selected),
                Contents::Tags { .. } if selected.iter().any(|r| matches!(r, Row::Tag(_))) => {
                    DeleteTarget::Tags(selected)
                }
                Contents::Directory | Contents::Recent(_) | Contents::Tags { .. } => {
                    let paths = selected
                        .into_iter()
                        .filter_map(|row| row.path().map(Path::to_path_buf))
                        .collect();
                    DeleteTarget::Local(paths)
                }
                _ => DeleteTarget::None,
            }
        });
        match target {
            DeleteTarget::None => {}
            DeleteTarget::Favorites(bookmarks) => {
                self.0.update(|state| {
                    for loc in &bookmarks {
                        state.places.set_favorite(loc.clone(), false);
                    }
                    let view = state.tab_mut().navigation.view_mut();
                    for loc in bookmarks {
                        view.selection.remove(&RowKey::Place(loc));
                    }
                });
                self.0.save_places();
                Navigator(self.0).refresh_location(&Location::Favorites);
                self.0.focus_content();
            }
            DeleteTarget::Remote(rows) => RemoteOps(self.0).delete_remote(rows),
            DeleteTarget::Trash(all) => LocalOps(self.0).restore_trash(true, all),
            DeleteTarget::Tags(rows) => TagOps(self.0).delete_tags(rows),
            DeleteTarget::Local(paths) => LocalOps(self.0).delete_local(paths, permanent),
        }
    }

    pub(in crate::pages::workspace) fn paste(&self) {
        let destination = self.0.read_untracked(|state| {
            if state.operation.busy() {
                return None;
            }
            let tab = state.tab();
            if !matches!(tab.contents, Contents::Directory) {
                return None;
            }
            tab.navigation.path().map(Path::to_path_buf)
        });
        let Some(destination) = destination else {
            return;
        };
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let payload = Clipboard::current().read_payload().await;
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                match payload {
                    Ok(Payload::Empty) => {}
                    Ok(Payload::Uris { mode, uris }) => {
                        if mode == clipboard::Mode::Cut {
                            ctx.report_notice(i18n::text_untracked("remote-cut-unavailable"));
                        } else {
                            RemoteOps(&ctx).paste_uri_payload(uris, destination, false);
                        }
                    }
                    Ok(Payload::Local(snapshot)) => {
                        PasteOps(&ctx).start_local_paste(destination, snapshot);
                    }
                    Err(error) => ctx.report_notice(error.to_string()),
                }
            }));
        });
    }

    pub(in crate::pages::workspace) fn column_sort(&self, column: Column) {
        let id = Rows(self.0).sort_by(column);
        EnrichmentRunner(self.0).enrich_directory(id);
    }
}
