//! File, clipboard, trash, history, volume, and sort workflows.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    rc::Rc,
};

use gds::i18n as gds_i18n;
use gds::service::desktop::{Desktop, OpenMode, OpenOutcome};
use gds::ui::IconName;
use gds::ui::drag::ExternalFileDrop;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zgui::prelude::{DragAction, DropHandle};
use zgui::reactive::{on_cleanup_local, spawn_local};

use super::{
    model::{
        ConfirmOptions, Contents, FILES_TASK_KEY, Row, RowKey, TRASH_TASK_KEY, remote_task_key,
    },
    name_form::{NameForm, NamePurpose},
    ui::{Browser, WeakBrowser},
};
use crate::{
    entities::{
        file::columns::Column,
        location::{
            Location, scheme,
            volumes::{self, DeviceAction},
        },
    },
    features::{
        file_operations::{
            Batch, Kind as DeleteKind, history, names, new_folder,
            paste::{self, Inspection, PassOutcome, PasteJob},
            recycle, rename, transfer,
        },
        pick_files,
    },
    shared::{
        i18n,
        lib::clipboard::{self, Clipboard, Payload},
    },
};

const NAME_PREVIEW_LIMIT: usize = 5;

#[derive(Clone, Default)]
struct HistoryPreview {
    undo: Vec<history::Kind>,
    redo: Vec<history::Kind>,
}

struct WindowHistorySlot {
    browser: WeakBrowser,
    preview: RefCell<HistoryPreview>,
}

thread_local! {
    static HISTORY_PREVIEWS: RefCell<HashMap<usize, Rc<WindowHistorySlot>>> = RefCell::new(HashMap::new());
}

fn history_slot(browser: &Browser) -> Rc<WindowHistorySlot> {
    let key = Rc::as_ptr(&browser.0) as usize;
    if let Some(existing) = HISTORY_PREVIEWS.with(|map| {
        map.borrow().get(&key).and_then(|slot| {
            slot.browser
                .upgrade()
                .filter(|b| Rc::ptr_eq(&b.0, &browser.0))
                .map(|_| Rc::clone(slot))
        })
    }) {
        return existing;
    }

    let created = Rc::new(WindowHistorySlot {
        browser: browser.downgrade(),
        preview: RefCell::new(HistoryPreview::default()),
    });
    browser.0.owner.with(|| {
        on_cleanup_local(move || {
            HISTORY_PREVIEWS.with(|map| {
                map.borrow_mut().remove(&key);
            });
        });
    });
    HISTORY_PREVIEWS.with(|map| {
        map.borrow_mut().insert(key, Rc::clone(&created));
    });
    created
}

#[derive(Clone)]
struct PasteContext {
    cut: bool,
    record_history: bool,
    snapshot: Option<clipboard::Snapshot>,
    drop_handle: Rc<RefCell<Option<DropHandle>>>,
}

impl PasteContext {
    fn new(
        cut: bool,
        record_history: bool,
        snapshot: Option<clipboard::Snapshot>,
        drop_handle: Option<DropHandle>,
    ) -> Self {
        Self {
            cut,
            record_history,
            snapshot,
            drop_handle: Rc::new(RefCell::new(drop_handle)),
        }
    }

    fn settle_drop(&self, succeeded: bool) {
        let Some(handle) = self.drop_handle.borrow_mut().take() else {
            return;
        };
        if succeeded {
            handle.complete();
        } else {
            handle.reject();
        }
    }
}

fn sanitize_preview_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_control() {
            for esc in ch.escape_default() {
                out.push(esc);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn freeze_name_preview(names: impl IntoIterator<Item = String>) -> (Vec<String>, usize) {
    let mut preview = Vec::with_capacity(NAME_PREVIEW_LIMIT);
    let mut total = 0usize;
    for name in names {
        total += 1;
        if preview.len() < NAME_PREVIEW_LIMIT {
            preview.push(sanitize_preview_name(&name));
        }
    }
    let overflow = total.saturating_sub(preview.len());
    (preview, overflow)
}

fn next_free_remote_name(default_name: &str, reserved: &HashSet<String>) -> String {
    if !reserved.contains(default_name) {
        return default_name.to_owned();
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{default_name} ({index})");
        if !reserved.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

impl Browser {
    fn record_history(&self, entry: history::Entry) {
        let kind = entry.kind;
        let recorded = self.update(|state| {
            let non_empty = !entry.steps.is_empty();
            state.history.record(entry);
            non_empty
        });
        if recorded {
            let slot = history_slot(self);
            let mut preview = slot.preview.borrow_mut();
            preview.redo.clear();
            preview.undo.push(kind);
        }
    }

    fn clear_history(&self) {
        self.update(|state| state.history.clear());
        let slot = history_slot(self);
        let mut preview = slot.preview.borrow_mut();
        preview.undo.clear();
        preview.redo.clear();
    }

    fn peek_history_kind(&self, redo: bool) -> Option<history::Kind> {
        let slot = history_slot(self);
        let preview = slot.preview.borrow();
        if redo {
            preview.redo.last().copied()
        } else {
            preview.undo.last().copied()
        }
    }

    pub(super) fn history_available(&self, redo: bool) -> bool {
        !self.is_background_task_busy_untracked(FILES_TASK_KEY)
            && !self.read_untracked(|state| state.operation.busy())
            && self.peek_history_kind(redo).is_some()
    }

    pub(super) fn history_label(&self, redo: bool) -> String {
        if let Some(kind) = self.peek_history_kind(redo) {
            let op_name = i18n::text_untracked(kind.label_id());
            if redo {
                gds::t_untracked!("redo-operation", operation = op_name)
            } else {
                gds::t_untracked!("undo-operation", operation = op_name)
            }
        } else {
            i18n::text_untracked(if redo { "redo" } else { "undo" })
        }
    }

    pub(super) fn cancel_clipboard_cut(&self) -> bool {
        let Some(snapshot) = Clipboard::current()
            .snapshot_untracked()
            .filter(|snap| snap.can_cancel())
        else {
            return false;
        };
        let paths = snapshot.paths.clone();
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = Clipboard::current().remove(snapshot, paths).await;
                if let Some(browser) = weak.upgrade() {
                    if let Err(error) = result {
                        browser.report_notice(error.to_string());
                    }
                    browser.changed();
                }
            }));
        });
        true
    }

    pub(super) fn refresh_affected(&self, paths: &[PathBuf]) {
        self.refresh_after_file_operation(paths, false);
    }

    fn refresh_after_file_operation(&self, paths: &[PathBuf], trash_affected: bool) {
        let mut dir_tabs = Vec::new();
        let mut trash_tabs = Vec::new();
        let mut has_tag_tabs = false;
        let parents: HashSet<PathBuf> = paths
            .iter()
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect();
        self.read_untracked(|state| {
            for tab in &state.tabs {
                match tab.contents {
                    Contents::Directory => {
                        if let Some(dir) = tab.navigation.path()
                            && (paths.iter().any(|path| path == dir) || parents.contains(dir))
                        {
                            dir_tabs.push(tab.id);
                        }
                    }
                    Contents::Trash(_) if trash_affected => {
                        trash_tabs.push(tab.id);
                    }
                    Contents::Tags { .. } => {
                        has_tag_tabs = true;
                    }
                    _ => {}
                }
            }
        });
        for id in dir_tabs {
            self.refresh_tab(id);
        }
        for id in trash_tabs {
            self.refresh_tab(id);
        }
        if has_tag_tabs {
            self.refresh_tag_tabs();
        }
    }

    pub(super) fn begin_operation(&self, label: &str) -> Option<CancellationToken> {
        self.begin_background_task(FILES_TASK_KEY, label)
    }

    pub(super) fn end_operation(&self) {
        let _ = self.finish_background_task(FILES_TASK_KEY);
    }

    pub(super) fn open_selected(&self) {
        let in_picker = self.read_untracked(|state| state.picker.is_some());
        let selected = self.read_untracked(|state| {
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
                self.accept_picker();
            }
            return;
        }
        if selected.len() == 1 {
            let (index, row) = &selected[0];
            if in_picker
                && let Row::File(entry) = row
                && !pick_files::Projection::is_directory(entry)
            {
                self.accept_picker();
                return;
            }
            self.open_row(*index, false);
            return;
        }
        if in_picker {
            self.accept_picker();
            return;
        }
        for (idx, row) in selected {
            match row {
                Row::File(entry) if pick_files::Projection::is_directory(&entry) => {
                    let location = self.local_location(&entry.path);
                    self.new_tab(Some(location));
                }
                Row::File(entry) => {
                    self.open_file(entry.path.clone(), OpenMode::DefaultApplication);
                }
                _ => self.open_row(idx, false),
            }
        }
    }

    pub(super) fn open_row(&self, index: usize, new_tab: bool) {
        let (row, in_picker) = self
            .read_untracked(|state| (state.tab().rows.get(index).cloned(), state.picker.is_some()));
        let Some(row) = row else {
            return;
        };
        match row {
            Row::File(entry) => {
                if pick_files::Projection::is_directory(&entry) {
                    let location = self.local_location(&entry.path);
                    if new_tab && !in_picker {
                        self.new_tab(Some(location));
                    } else {
                        self.open(location);
                    }
                } else if in_picker {
                    self.activate_picker_path(entry.path.clone());
                } else {
                    self.open_file(entry.path.clone(), OpenMode::DefaultApplication);
                }
            }
            Row::Place(location) => {
                if new_tab && !in_picker {
                    self.new_tab(Some(location));
                } else {
                    self.open(location);
                }
            }
            Row::Tag(tag) => {
                if let Ok(address) = scheme::TagAddress::new(&tag) {
                    let location = Location::Tags(address);
                    if new_tab && !in_picker {
                        self.new_tab(Some(location));
                    } else {
                        self.open(location);
                    }
                }
            }
            Row::Remote(entry) => self.activate_remote(entry, new_tab),
            Row::Volume(volume) => self.open_volume(&volume.id, new_tab),
            Row::Trash(_) => {}
        }
    }

    pub(super) fn open_file(&self, path: PathBuf, mode: OpenMode) {
        let cancel = self.0.tasks.token();
        self.work(
            "Open file",
            async move {
                let desktop = Desktop::default();
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => Ok(OpenOutcome::Cancelled),
                    result = desktop.open_file(&path, mode) => {
                        result.map_err(|error| error.to_string())
                    }
                }
            },
            |browser, result| match result {
                Ok(OpenOutcome::Opened | OpenOutcome::Cancelled) => {}
                Err(error) => browser.report_notice(error),
            },
        );
    }

    pub(super) fn new_folder(&self) {
        enum NewFolderTarget {
            Local {
                directory: PathBuf,
                folder_label: String,
                reserved: HashSet<OsString>,
            },
            Remote {
                tab_id: u64,
                parent_address: gds::service::remote::Address,
                session: gds::service::remote::Session,
                folder_label: String,
                reserved: HashSet<String>,
            },
        }

        let target = self.read_untracked(|state| {
            let tab = state.tab();
            if tab.navigation.is_loading() {
                return None;
            }
            match &tab.contents {
                Contents::Directory => {
                    if state.operation.busy() {
                        return None;
                    }
                    let directory = tab.navigation.path()?.to_path_buf();
                    let folder_label = self.location_title(tab.navigation.location());
                    let reserved: HashSet<OsString> = tab
                        .navigation
                        .directory()
                        .map(|snap| {
                            snap.entries
                                .iter()
                                .map(|entry| entry.name.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(NewFolderTarget::Local {
                        directory,
                        folder_label,
                        reserved,
                    })
                }
                Contents::Remote { listing, session } => {
                    if state.operation.busy()
                        || self.is_background_task_busy_untracked(&remote_task_key(tab.id))
                    {
                        return None;
                    }
                    let folder_label = self.location_title(tab.navigation.location());
                    let reserved: HashSet<String> = listing
                        .entries
                        .iter()
                        .map(|entry| entry.name.clone())
                        .collect();
                    Some(NewFolderTarget::Remote {
                        tab_id: tab.id,
                        parent_address: listing.address.clone(),
                        session: session.clone(),
                        folder_label,
                        reserved,
                    })
                }
                _ => None,
            }
        });

        let Some(target) = target else {
            return;
        };

        match target {
            NewFolderTarget::Local {
                directory,
                folder_label,
                reserved,
            } => {
                let default_name = i18n::text_untracked("new-folder-default-name");
                let suggestion = names::free_name(&directory, OsStr::new(&default_name), &reserved)
                    .unwrap_or_else(|_| OsString::from(&default_name));
                let title = gds::t_untracked!("new-folder-title", folder = folder_label);
                let form = NameForm::new(NamePurpose::NewFolder, title, suggestion);
                let browser = self.clone();
                self.name_form(form, move |name| {
                    let Some(cancel) =
                        browser.begin_operation(&i18n::text_untracked("new-folder-working"))
                    else {
                        return;
                    };
                    let parent = directory.clone();
                    let request = new_folder::Request {
                        directory: parent.clone(),
                        name,
                    };
                    let task = request.spawn(&browser.scope(), cancel);
                    let weak = browser.downgrade();
                    browser.0.owner.with(|| {
                        drop(spawn_local(async move {
                            let result = task.await;
                            let Some(browser) = weak.upgrade() else {
                                return;
                            };
                            browser.end_operation();
                            match result {
                                Ok(Ok(created)) => {
                                    let label = created
                                        .path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    if let Some(expected) = created.snapshot {
                                        browser.record_history(history::Entry::new(
                                            history::Kind::NewFolder,
                                            vec![history::Step::trash_empty_directory(
                                                created.path.clone(),
                                                expected,
                                            )],
                                        ));
                                    }
                                    browser.reveal_paths(vec![created.path.clone()]);
                                    browser.refresh_affected(&[parent]);
                                    browser.report_success(gds::t_untracked!(
                                        "new-folder-done",
                                        name = label
                                    ));
                                    browser.focus_content();
                                }
                                Ok(Err(error)) => {
                                    let message = match error {
                                        new_folder::Error::Exists { name, .. } => {
                                            gds::t_untracked!("new-folder-exists", name = name)
                                        }
                                        other => gds::t_untracked!(
                                            "new-folder-failed",
                                            error = other.to_string()
                                        ),
                                    };
                                    browser.report_notice(message);
                                }
                                Err(error) => {
                                    browser.report_notice(gds::t_untracked!(
                                        "new-folder-failed",
                                        error = error.to_string()
                                    ));
                                }
                            }
                        }));
                    });
                });
            }
            NewFolderTarget::Remote {
                tab_id,
                parent_address,
                session,
                folder_label,
                reserved,
            } => {
                let default_name = i18n::text_untracked("new-folder-default-name");
                let suggestion = next_free_remote_name(&default_name, &reserved);
                let title = gds::t_untracked!("new-folder-title", folder = folder_label);
                let form = NameForm::new(NamePurpose::NewFolder, title, OsString::from(suggestion));
                let browser = self.clone();
                self.name_form(form, move |os_name| {
                    let Some(name_str) = os_name.to_str() else {
                        browser.report_notice(i18n::text_untracked("rename-not-text"));
                        return;
                    };
                    let Ok(child_address) = parent_address.child(name_str) else {
                        browser.report_notice(i18n::text_untracked("rename-error-invalid"));
                        return;
                    };
                    let task_key = remote_task_key(tab_id);
                    let Some(_cancel) = browser.begin_background_task(
                        task_key.clone(),
                        i18n::text_untracked("new-folder-working"),
                    ) else {
                        return;
                    };
                    let session = session.clone();
                    let parent_location = Location::Remote(parent_address.clone());
                    let display_name = name_str.to_owned();
                    let (tx, mut rx) = mpsc::unbounded_channel();
                    drop(
                        browser
                            .scope()
                            .spawn_named("Create remote folder", async move {
                                let result = session.mkdir(&child_address).await;
                                let _ = tx.send(result);
                            }),
                    );
                    let weak = browser.downgrade();
                    browser.0.owner.with(|| {
                        drop(spawn_local(async move {
                            let Some(result) = rx.recv().await else {
                                return;
                            };
                            let Some(browser) = weak.upgrade() else {
                                return;
                            };
                            let _ = browser.finish_background_task(&task_key);
                            match result {
                                Ok(()) => {
                                    browser.refresh_location(&parent_location);
                                    browser.report_success(gds::t_untracked!(
                                        "new-folder-done",
                                        name = display_name
                                    ));
                                }
                                Err(error) => {
                                    browser.report_notice(gds::t_untracked!(
                                        "new-folder-failed",
                                        error = error.to_string()
                                    ));
                                }
                            }
                            browser.focus_content();
                        }));
                    });
                });
            }
        }
    }

    pub(super) fn rename(&self) {
        let target = self.read_untracked(|state| {
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
            Row::Remote(entry) => self.rename_remote(entry),
            Row::Volume(volume) => self.rename_volume(volume),
            Row::File(entry) => {
                let source = entry.path.clone();
                let current_name = entry.name.clone();
                let display = current_name.to_string_lossy().into_owned();
                let form = NameForm::new(
                    NamePurpose::Rename,
                    gds::t_untracked!("rename-title", name = display),
                    current_name.clone(),
                );
                let browser = self.clone();
                self.name_form(form, move |name| {
                    if name == current_name {
                        browser.focus_content();
                        return;
                    }
                    let Some(cancel) =
                        browser.begin_operation(&i18n::text_untracked("rename-working"))
                    else {
                        return;
                    };
                    let src = source.clone();
                    let request = rename::Request {
                        source: src.clone(),
                        name,
                    };
                    let task = request.spawn(&browser.scope(), cancel);
                    let weak = browser.downgrade();
                    browser.0.owner.with(|| {
                        drop(spawn_local(async move {
                            let result = task.await;
                            let Some(browser) = weak.upgrade() else {
                                return;
                            };
                            browser.end_operation();
                            match result {
                                Ok(Ok(renamed)) => {
                                    let display_new = renamed
                                        .destination
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    if let Some(expected) = renamed.snapshot {
                                        browser.record_history(history::Entry::new(
                                            history::Kind::Rename,
                                            vec![history::Step::Move {
                                                source: renamed.destination.clone(),
                                                destination: src.clone(),
                                                expected,
                                            }],
                                        ));
                                    }
                                    browser.reveal_paths(vec![renamed.destination.clone()]);
                                    browser.refresh_affected(&[src, renamed.destination]);
                                    browser.report_success(gds::t_untracked!(
                                        "rename-done",
                                        name = display_new
                                    ));
                                    browser.focus_content();
                                }
                                Ok(Err(error)) => {
                                    let message = match error {
                                        rename::Error::Exists { name, .. } => {
                                            gds::t_untracked!("rename-target-exists", name = name)
                                        }
                                        rename::Error::Name(err) => {
                                            i18n::text_untracked(err.label_id())
                                        }
                                        other => gds::t_untracked!(
                                            "rename-failed",
                                            error = other.to_string()
                                        ),
                                    };
                                    browser.report_notice(message);
                                }
                                Err(error) => {
                                    browser.report_notice(gds::t_untracked!(
                                        "rename-failed",
                                        error = error.to_string()
                                    ));
                                }
                            }
                        }));
                    });
                });
            }
            Row::Place(_) | Row::Trash(_) | Row::Tag(_) => {}
        }
    }

    pub(super) fn delete(&self, permanent: bool) {
        enum DeleteTarget {
            None,
            Favorites(Vec<Location>),
            Remote(Vec<Row>),
            Trash(bool),
            Tags(Vec<Row>),
            Local(Vec<PathBuf>),
        }
        let target = self.read_untracked(|state| {
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
                return if self.is_background_task_busy_untracked(TRASH_TASK_KEY) {
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
                Contents::Directory | Contents::Tags { .. } => {
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
                self.update(|state| {
                    for loc in &bookmarks {
                        state.places.set_favorite(loc.clone(), false);
                    }
                    let view = state.tab_mut().navigation.view_mut();
                    for loc in bookmarks {
                        view.selection.remove(&RowKey::Place(loc));
                    }
                });
                self.save_places();
                self.refresh_location(&Location::Favorites);
                self.focus_content();
            }
            DeleteTarget::Remote(rows) => self.delete_remote(rows),
            DeleteTarget::Trash(all) => self.restore_trash(true, all),
            DeleteTarget::Tags(rows) => self.delete_tags(rows),
            DeleteTarget::Local(paths) => {
                if paths.is_empty() {
                    return;
                }
                let batch = match Batch::new(paths) {
                    Ok(batch) => batch,
                    Err(error) => {
                        self.report_notice(error.to_string());
                        return;
                    }
                };
                let actually_permanent = permanent || !recycle::supported();
                if actually_permanent {
                    let count = batch.paths().len();
                    let (items, overflow_count) =
                        freeze_name_preview(batch.paths().iter().map(|path| {
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string())
                        }));
                    let title = gds_i18n::count_untracked("delete-confirm-title", count);
                    let message = i18n::text_untracked("delete-confirm-message");
                    let primary = gds_i18n::count_untracked("delete-selected", count);
                    let options = ConfirmOptions {
                        icon: Some(IconName::Trash),
                        primary_icon: Some(IconName::Trash),
                        destructive: true,
                        items,
                        overflow_count,
                    };
                    let browser = self.clone();
                    let paths = batch.paths().to_vec();
                    self.confirm_with_options(title, message, primary, options, move || {
                        if let Ok(batch) = Batch::new(paths.clone()) {
                            browser.execute_delete(batch, DeleteKind::PermanentDelete);
                        }
                    });
                } else {
                    self.execute_delete(batch, DeleteKind::Trash);
                }
            }
        }
    }

    fn execute_delete(&self, batch: Batch, kind: DeleteKind) {
        let count = batch.paths().len();
        let label = gds_i18n::count_untracked(
            if kind == DeleteKind::Trash {
                "trash-working"
            } else {
                "delete-working"
            },
            count,
        );
        let Some(cancel) = self.begin_operation(&label) else {
            return;
        };
        let affected = batch.paths().to_vec();
        let task = self.scope().spawn_blocking_named("Delete files", move || {
            batch.execute_cancellable(kind, &cancel)
        });
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                browser.end_operation();
                let Ok(report) = result else {
                    if let Err(error) = result {
                        browser.report_notice(error.to_string());
                    }
                    return;
                };
                let succeeded = report.succeeded_paths();
                let failed: Vec<String> = report
                    .failed_items()
                    .map(|item| {
                        let name = item.path.display().to_string();
                        match &item.result {
                            Ok(()) => name,
                            Err(err) => format!("{name}: {err}"),
                        }
                    })
                    .collect();
                if report.kind == DeleteKind::Trash && !report.receipts.is_empty() {
                    let steps = report
                        .receipts
                        .into_iter()
                        .map(history::Step::Restore)
                        .collect();
                    browser.record_history(history::Entry::new(history::Kind::Trash, steps));
                }
                browser.refresh_after_file_operation(&affected, report.kind == DeleteKind::Trash);
                if !failed.is_empty() {
                    let headline = if report.kind == DeleteKind::Trash {
                        gds::t_untracked!(
                            "trash-partial",
                            succeeded = succeeded.len() as i64,
                            failed = failed.len() as i64
                        )
                    } else {
                        gds::t_untracked!(
                            "delete-partial",
                            succeeded = succeeded.len() as i64,
                            failed = failed.len() as i64
                        )
                    };
                    let detail = failed.join("\n");
                    browser.report_notice(format!("{headline}\n{detail}"));
                } else {
                    let key = if report.kind == DeleteKind::Trash {
                        "trash-done"
                    } else {
                        "delete-done"
                    };
                    browser.report_success(gds_i18n::count_untracked(key, succeeded.len()));
                }
                if !report.history_errors.is_empty() {
                    browser.report_notice(report.history_errors.join("; "));
                }
                browser.focus_content();
            }));
        });
    }

    pub(super) fn replay(&self, redo: bool) {
        if self.is_background_task_busy_untracked(FILES_TASK_KEY)
            || self.read_untracked(|state| state.operation.busy())
        {
            return;
        }
        let direction = if redo {
            history::Direction::Redo
        } else {
            history::Direction::Undo
        };
        let entry = self.update(|state| state.history.take(direction));
        let slot = history_slot(self);
        {
            let mut preview = slot.preview.borrow_mut();
            let stack = if redo {
                &mut preview.redo
            } else {
                &mut preview.undo
            };
            if entry.is_some() {
                let _ = stack.pop();
            } else {
                stack.clear();
            }
        }
        let Some(entry) = entry else {
            return;
        };
        let op_name = i18n::text_untracked(entry.kind.label_id());
        let label = if redo {
            gds::t_untracked!("redo-operation", operation = op_name)
        } else {
            gds::t_untracked!("undo-operation", operation = op_name)
        };
        let Some(cancel) = self.begin_operation(&label) else {
            if redo {
                self.clear_history();
            }
            return;
        };
        let task = self
            .scope()
            .spawn_blocking_named("Replay file operation", move || {
                entry.execute_cancellable(&cancel)
            });
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                browser.end_operation();
                let Ok(mut report) = result else {
                    if let Err(error) = result {
                        browser.report_notice(error.to_string());
                    }
                    return;
                };
                let changed = report.changed.clone();
                let errors = report.errors.clone();
                let kind = report.kind;
                let has_remaining = !report.remaining.is_empty();
                let has_inverse = !report.inverse.is_empty();
                let trash_affected = matches!(
                    kind,
                    history::Kind::Trash | history::Kind::Restore | history::Kind::NewFolder
                );
                browser.update(|state| {
                    state.history.finish(direction, &mut report);
                });
                {
                    let finish_slot = history_slot(&browser);
                    let mut preview = finish_slot.preview.borrow_mut();
                    if has_remaining {
                        if redo {
                            preview.redo.push(kind);
                        } else {
                            preview.undo.push(kind);
                        }
                    }
                    if has_inverse {
                        if redo {
                            preview.undo.push(kind);
                        } else {
                            preview.redo.push(kind);
                        }
                    }
                }
                browser.refresh_after_file_operation(&changed, trash_affected);
                if errors.is_empty() {
                    browser.report_success(i18n::text_untracked(if redo {
                        "redo-done"
                    } else {
                        "undo-done"
                    }));
                } else {
                    let headline =
                        i18n::text_untracked(if redo { "redo-failed" } else { "undo-failed" });
                    browser.report_notice(format!("{headline}: {}", errors.join("; ")));
                }
                browser.focus_content();
            }));
        });
    }

    pub(super) fn copy(&self, cut: bool) {
        enum CopyTarget {
            Empty,
            Remote(Vec<String>),
            Local(Vec<PathBuf>),
        }
        let target = self.read_untracked(|state| {
            let tab = state.tab();
            let selected = tab.selected();
            if selected.is_empty() {
                return CopyTarget::Empty;
            }
            if matches!(tab.contents, Contents::Remote { .. }) {
                let uris = selected
                    .into_iter()
                    .filter_map(|row| match row {
                        Row::Remote(entry) => Some(entry.address.uri().to_owned()),
                        _ => None,
                    })
                    .collect();
                return CopyTarget::Remote(uris);
            }
            let paths = selected
                .into_iter()
                .filter_map(|row| row.path().map(Path::to_path_buf))
                .collect();
            CopyTarget::Local(paths)
        });
        match target {
            CopyTarget::Empty => {}
            CopyTarget::Remote(uris) => {
                if cut {
                    self.report_notice(i18n::text_untracked("remote-cut-unavailable"));
                    return;
                }
                if uris.is_empty() {
                    return;
                }
                let count = uris.len();
                let weak = self.downgrade();
                self.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let result = Clipboard::current().write_uris(uris).await;
                        let Some(browser) = weak.upgrade() else {
                            return;
                        };
                        match result {
                            Ok(()) => {
                                browser.report_success(gds_i18n::count_untracked(
                                    "copy-prepared",
                                    count,
                                ));
                                browser.changed();
                            }
                            Err(error) => browser.report_notice(error.to_string()),
                        }
                    }));
                });
            }
            CopyTarget::Local(paths) => {
                let batch = match Batch::new(paths) {
                    Ok(batch) => batch,
                    Err(error) => {
                        self.report_notice(error.to_string());
                        return;
                    }
                };
                let paths = batch.paths().to_vec();
                let count = paths.len();
                let mode = if cut {
                    clipboard::Mode::Cut
                } else {
                    clipboard::Mode::Copy
                };
                let weak = self.downgrade();
                self.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let result = Clipboard::current().write(mode, paths).await;
                        let Some(browser) = weak.upgrade() else {
                            return;
                        };
                        match result {
                            Ok(_) => {
                                browser.report_success(gds_i18n::count_untracked(
                                    "copy-prepared",
                                    count,
                                ));
                                browser.changed();
                            }
                            Err(error) => browser.report_notice(error.to_string()),
                        }
                    }));
                });
            }
        }
    }

    pub(super) fn paste(&self) {
        let destination = self.read_untracked(|state| {
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
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let payload = Clipboard::current().read_payload().await;
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                match payload {
                    Ok(Payload::Empty) => {}
                    Ok(Payload::Uris { mode, uris }) => {
                        if mode == clipboard::Mode::Cut {
                            browser.report_notice(i18n::text_untracked("remote-cut-unavailable"));
                        } else {
                            browser.paste_uri_payload(uris, destination, false);
                        }
                    }
                    Ok(Payload::Local(snapshot)) => {
                        let cut = snapshot.mode == clipboard::Mode::Cut;
                        let mode = if cut {
                            transfer::Mode::Move
                        } else {
                            transfer::Mode::Copy
                        };
                        let job = PasteJob::new(destination, mode, snapshot.paths.clone());
                        browser.start_paste_job(
                            job,
                            PasteContext::new(cut, true, Some(snapshot), None),
                        );
                    }
                    Err(error) => browser.report_notice(error.to_string()),
                }
            }));
        });
    }

    pub(super) fn handle_external_file_drop(&self, destination: PathBuf, drop: ExternalFileDrop) {
        let ExternalFileDrop {
            paths,
            action,
            handle,
            ..
        } = drop;
        if paths.is_empty() {
            if let Some(handle) = handle {
                handle.reject();
            }
            self.report_notice(i18n::text_untracked("drop-no-local-files"));
            return;
        }
        let mode = if action == Some(DragAction::Move) {
            transfer::Mode::Move
        } else {
            transfer::Mode::Copy
        };
        let invalid_self_target = paths
            .iter()
            .any(|src| destination == *src || destination.starts_with(src));
        if invalid_self_target {
            if let Some(handle) = handle {
                handle.reject();
            }
            self.report_notice(i18n::text_untracked("drop-into-self"));
            return;
        }
        let same_dir = mode == transfer::Mode::Move
            && paths
                .iter()
                .all(|path| path.parent() == Some(destination.as_path()));
        if same_dir {
            if let Some(handle) = handle {
                handle.reject();
            }
            return;
        }
        let job = PasteJob::new(destination, mode, paths);
        self.start_paste_job(job, PasteContext::new(false, true, None, handle));
    }

    pub(super) fn inspect_paste(
        &self,
        transfer: paste::PasteJob,
        cut: bool,
        history: bool,
        finish: Option<Vec<String>>,
    ) {
        let snapshot = if cut && finish.is_some() {
            Clipboard::current()
                .snapshot_untracked()
                .map(|snap| (*snap).clone())
        } else {
            None
        };
        self.start_paste_job(transfer, PasteContext::new(cut, history, snapshot, None));
    }

    fn start_paste_job(&self, job: PasteJob, context: PasteContext) {
        let Some(cancel) = self.begin_operation(&i18n::text_untracked("paste-working")) else {
            context.settle_drop(false);
            self.report_notice(i18n::text_untracked("file-operation-busy"));
            return;
        };
        self.inspect_paste_step(job, context, cancel);
    }

    fn inspect_paste_step(&self, job: PasteJob, context: PasteContext, cancel: CancellationToken) {
        let task = self
            .scope()
            .spawn_blocking_named("Inspect paste conflicts", move || job.inspect());
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(browser) = weak.upgrade() else {
                    context.settle_drop(false);
                    return;
                };
                let Ok(inspection) = result else {
                    context.settle_drop(false);
                    browser.end_operation();
                    if let Err(error) = result {
                        browser.report_notice(error.to_string());
                    }
                    return;
                };
                match inspection {
                    Inspection::Ready(ready_job) => {
                        browser.execute_paste_step(ready_job, context, cancel);
                    }
                    Inspection::Failed { job, error } => {
                        if job.succeeded.is_empty() && job.failures.is_empty() {
                            context.settle_drop(false);
                            browser.end_operation();
                            browser.report_notice(error.to_string());
                        } else {
                            let mut failed_job = job;
                            failed_job.record_abort(error.to_string());
                            browser.finish_paste_job(failed_job, context, true);
                        }
                    }
                    Inspection::Conflict { job, conflict } => {
                        browser.ask_paste_conflict(job, conflict, context, cancel);
                    }
                }
            }));
        });
    }

    fn ask_paste_conflict(
        &self,
        job: PasteJob,
        conflict: paste::PasteConflict,
        context: PasteContext,
        cancel: CancellationToken,
    ) {
        let title = gds::t_untracked!(
            "transfer-conflict-title",
            name = conflict.collision.target_label()
        );
        let form = NameForm::new(NamePurpose::Conflict, title, conflict.suggestion);
        let source = conflict.collision.source;
        let browser_submit = self.clone();
        let browser_cancel = self.clone();
        let shared_job = std::rc::Rc::new(std::cell::RefCell::new(Some(job)));
        let submit_job = std::rc::Rc::clone(&shared_job);
        let cancel_job = shared_job;
        let submit_context = context.clone();
        let cancel_context = context;
        self.name_form_with_cancel(
            form,
            move |chosen_name| {
                let Some(mut next_job) = submit_job.borrow_mut().take() else {
                    return;
                };
                next_job.resolve_conflict(source.clone(), chosen_name);
                browser_submit.inspect_paste_step(
                    next_job,
                    submit_context.clone(),
                    cancel.clone(),
                );
            },
            move || {
                let Some(mut cancelled_job) = cancel_job.borrow_mut().take() else {
                    return;
                };
                // Cancelling a conflict prompt means the batch did not finish cleanly; reject any
                // pending Wayland drop offer immediately so a move source never deletes files.
                cancel_context.settle_drop(false);
                cancelled_job.pending.clear();
                if cancelled_job.succeeded.is_empty() && cancelled_job.failures.is_empty() {
                    browser_cancel.end_operation();
                    browser_cancel.report_notice(i18n::text_untracked("paste-cancelled"));
                } else {
                    browser_cancel.finish_paste_job(cancelled_job, cancel_context.clone(), false);
                }
            },
        );
    }

    fn execute_paste_step(
        &self,
        mut job: PasteJob,
        context: PasteContext,
        cancel: CancellationToken,
    ) {
        let buffer_bytes = self
            .0
            .preferences
            .snapshot()
            .files
            .transfer
            .local_buffer_bytes();
        let batch = match job
            .prepare_batch()
            .map(|batch| batch.with_buffer_size(buffer_bytes))
        {
            Ok(batch) => batch,
            Err(error) => {
                if job.succeeded.is_empty() && job.failures.is_empty() {
                    context.settle_drop(false);
                    self.end_operation();
                    self.report_notice(error.to_string());
                } else {
                    job.record_abort(error.to_string());
                    self.finish_paste_job(job, context, true);
                }
                return;
            }
        };

        let task_cancel = cancel.clone();
        let task = self
            .scope()
            .spawn_blocking_named("Transfer pasted files", move || {
                batch.execute_cancellable(task_cancel)
            });
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(browser) = weak.upgrade() else {
                    context.settle_drop(false);
                    return;
                };
                let Ok(report) = result else {
                    if job.succeeded.is_empty() && job.failures.is_empty() {
                        context.settle_drop(false);
                        browser.end_operation();
                        if let Err(error) = result {
                            browser.report_notice(error.to_string());
                        }
                    } else {
                        if let Err(error) = result {
                            job.record_abort(error.to_string());
                        }
                        browser.finish_paste_job(job, context, true);
                    }
                    return;
                };
                match job.apply_pass(report) {
                    PassOutcome::Completed(done) => {
                        browser.finish_paste_job(done, context, false);
                    }
                    PassOutcome::RetryConflict(next_job) => {
                        browser.inspect_paste_step(next_job, context, cancel);
                    }
                }
            }));
        });
    }

    fn finish_paste_job(&self, job: PasteJob, context: PasteContext, aborted: bool) {
        let destination = job.destination.clone();
        if context.cut
            && !job.succeeded.is_empty()
            && let Some(snap) = context.snapshot.clone()
        {
            let moved = job.succeeded.clone();
            let weak = self.downgrade();
            self.0.owner.with(|| {
                drop(spawn_local(async move {
                    let clipboard_error = Clipboard::current()
                        .finish_cut(snap, moved)
                        .await
                        .err()
                        .map(|error| error.to_string());
                    let Some(browser) = weak.upgrade() else {
                        context.settle_drop(false);
                        return;
                    };
                    browser.complete_paste_summary(
                        job.into_summary(clipboard_error),
                        destination,
                        context,
                        aborted,
                    );
                }));
            });
            return;
        }
        self.complete_paste_summary(job.into_summary(None), destination, context, aborted);
    }

    fn complete_paste_summary(
        &self,
        summary: paste::Summary,
        destination: PathBuf,
        context: PasteContext,
        aborted: bool,
    ) {
        let whole_batch_succeeded = !aborted
            && summary.failures.is_empty()
            && summary.clipboard_error.is_none()
            && !summary.succeeded.is_empty();
        context.settle_drop(whole_batch_succeeded);
        self.end_operation();
        let mut changed_paths = vec![destination];
        changed_paths.extend(summary.succeeded.iter().cloned());
        changed_paths.extend(summary.committed.iter().cloned());
        changed_paths.extend(summary.undo.iter().flat_map(history::Step::paths));
        if context.record_history {
            let kind = summary.history_kind();
            let reset = summary.requires_history_reset();
            if reset {
                self.clear_history();
            }
            self.record_history(history::Entry::new(kind, summary.undo));
        }
        self.refresh_affected(&changed_paths);
        if !summary.committed.is_empty() {
            self.reveal_paths(summary.committed);
        }

        let failed_count = summary.failures.len();
        if failed_count > 0 || summary.clipboard_error.is_some() || aborted {
            let headline = if aborted && failed_count == 0 {
                gds::t_untracked!(
                    "paste-aborted-partial",
                    completed = summary.succeeded.len() as i64
                )
            } else {
                gds::t_untracked!(
                    "transfer-partial",
                    succeeded = summary.succeeded.len() as i64,
                    failed = failed_count as i64
                )
            };
            let mut details = summary.failures;
            if let Some(err) = summary.clipboard_error {
                details.push(gds::t_untracked!("clipboard-update-failed", error = err));
            }
            let message = if details.is_empty() {
                headline
            } else {
                format!("{headline}\n{}", details.join("\n"))
            };
            self.report_notice(message);
        } else {
            let key = if summary.mode == transfer::Mode::Move {
                "move-done"
            } else {
                "copy-done"
            };
            self.report_success(gds_i18n::count_untracked(key, summary.succeeded.len()));
        }
        self.focus_content();
    }

    pub(super) fn restore_trash(&self, purge: bool, all: bool) {
        if self.is_background_task_busy_untracked(TRASH_TASK_KEY) {
            return;
        }
        let entries: Vec<recycle::Entry> = self.read_untracked(|state| {
            let tab = state.tab();
            if !matches!(tab.contents, Contents::Trash(_)) || tab.navigation.is_loading() {
                return Vec::new();
            }
            let view = tab.navigation.view();
            tab.rows
                .iter()
                .filter_map(|row| match row {
                    Row::Trash(entry) if all || view.selection.contains(&row.key()) => {
                        Some(entry.clone())
                    }
                    _ => None,
                })
                .collect()
        });
        if entries.is_empty() {
            return;
        }
        if purge {
            let count = entries.len();
            let (items, overflow_count) = freeze_name_preview(
                entries
                    .iter()
                    .map(|entry| entry.name.to_string_lossy().into_owned()),
            );
            let (title, message, primary, icon) = if all {
                (
                    i18n::text_untracked("recycle-empty-confirm-title"),
                    gds_i18n::count_untracked("recycle-empty-confirm-message", count),
                    i18n::text_untracked("recycle-empty-confirm-ok"),
                    IconName::Broom,
                )
            } else {
                (
                    gds_i18n::count_untracked("recycle-purge-confirm-title", count),
                    i18n::text_untracked("recycle-purge-confirm-message"),
                    i18n::text_untracked("recycle-purge-confirm-ok"),
                    IconName::Trash,
                )
            };
            let options = ConfirmOptions {
                icon: Some(icon),
                primary_icon: Some(icon),
                destructive: true,
                items,
                overflow_count,
            };
            let browser = self.clone();
            self.confirm_with_options(title, message, primary, options, move || {
                browser.execute_trash_action(recycle::Action::Purge, entries.clone());
            });
        } else {
            self.execute_trash_action(recycle::Action::Restore, entries);
        }
    }

    fn execute_trash_action(&self, action: recycle::Action, entries: Vec<recycle::Entry>) {
        let count = entries.len();
        let label = gds_i18n::count_untracked(
            match action {
                recycle::Action::Restore => "recycle-restore-working",
                recycle::Action::Purge => "recycle-purge-working",
            },
            count,
        );
        let Some(cancel) = self.begin_background_task(TRASH_TASK_KEY, label) else {
            return;
        };
        let task = self
            .scope()
            .spawn_blocking_named("Trash operation", move || {
                recycle::perform_cancellable(action, entries, &cancel)
            });
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                let _ = browser.finish_background_task(TRASH_TASK_KEY);
                let Ok(outcomes) = result else {
                    if let Err(error) = result {
                        browser.report_notice(error.to_string());
                    }
                    return;
                };
                let mut succeeded = 0usize;
                let mut failed = Vec::new();
                let mut undo_steps = Vec::new();
                let mut changed_paths = Vec::new();
                for outcome in outcomes {
                    match outcome.result {
                        Ok(()) => {
                            succeeded += 1;
                            if matches!(action, recycle::Action::Restore) {
                                changed_paths.push(outcome.entry.original_path.clone());
                                if let Some(snapshot) = outcome.restored {
                                    undo_steps.push(history::Step::undo_restore(
                                        &outcome.entry,
                                        snapshot,
                                    ));
                                }
                            }
                        }
                        Err(error) => {
                            failed
                                .push(format!("{}: {error}", outcome.entry.name.to_string_lossy()));
                        }
                    }
                }
                if !undo_steps.is_empty() {
                    browser.record_history(history::Entry::new(history::Kind::Restore, undo_steps));
                }
                browser.refresh_after_file_operation(&changed_paths, true);
                if !failed.is_empty() {
                    let headline = match action {
                        recycle::Action::Restore => gds::t_untracked!(
                            "recycle-restore-partial",
                            succeeded = succeeded as i64,
                            failed = failed.len() as i64
                        ),
                        recycle::Action::Purge => gds::t_untracked!(
                            "recycle-purge-partial",
                            succeeded = succeeded as i64,
                            failed = failed.len() as i64
                        ),
                    };
                    browser.report_notice(format!("{headline}\n{}", failed.join("\n")));
                } else {
                    let key = match action {
                        recycle::Action::Restore => "recycle-restore-done",
                        recycle::Action::Purge => "recycle-purge-done",
                    };
                    browser.report_success(gds_i18n::count_untracked(key, succeeded));
                }
                browser.focus_content();
            }));
        });
    }

    pub(super) fn open_volume(&self, id: &volumes::VolumeId, new_tab: bool) {
        let Some(volume) = self.0.volumes.snapshot_untracked().find(id).cloned() else {
            self.report_notice(i18n::text_untracked("device-reconnect-missing"));
            return;
        };
        if !volume.mount_points.is_empty() {
            let location = Location::Volume {
                identity: Self::volume_identity(&volume),
                relative: OsString::new(),
            };
            if new_tab {
                self.new_tab(Some(location));
            } else {
                self.open(location);
            }
            return;
        }
        if volume.locked {
            self.report_notice(i18n::text_untracked("device-unlock-unavailable"));
            return;
        }
        if !volume.mountable {
            self.report_notice(i18n::text_untracked("device-not-mounted"));
            return;
        }
        let Ok(future) = self
            .0
            .volumes
            .operate(DeviceAction::Mount(volume.id.clone()))
        else {
            self.report_notice(i18n::text_untracked("device-path-unavailable"));
            return;
        };
        let identity = Self::volume_identity(&volume);
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = future.await;
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(_) => {
                        let location = Location::Volume {
                            identity,
                            relative: OsString::new(),
                        };
                        if new_tab {
                            browser.new_tab(Some(location));
                        } else {
                            browser.open(location);
                        }
                    }
                    Err(error) => browser.report_notice(error.to_string()),
                }
            }));
        });
    }

    pub(super) fn reconnect_volume(&self) {
        let location = self.read_untracked(|state| state.tab().navigation.location().clone());
        let Location::Volume { ref identity, .. } = location else {
            self.refresh();
            return;
        };
        self.0.volumes.refresh();
        if let Some(volume) = self.volume_at_location_untracked(identity) {
            if !volume.mount_points.is_empty() {
                self.refresh();
            } else {
                self.open_volume(&volume.id, false);
            }
        } else {
            self.report_notice(i18n::text_untracked("device-reconnect-missing"));
        }
    }

    pub(super) fn select_context(&self, index: usize) {
        let changed = self.update(|state| {
            let tab = state.tab_mut();
            let Some(row) = tab.rows.get(index) else {
                return false;
            };
            let key = row.key();
            let view = tab.navigation.view_mut();
            if view.selection.contains(&key) {
                view.cursor = Some(key);
                return false;
            }
            view.selection.replace([key.clone()]);
            view.anchor = Some(key.clone());
            view.cursor = Some(key);
            true
        });
        if changed {
            self.update_selection(|_| {});
        }
    }

    pub(super) fn column_sort(&self, column: Column) {
        self.sort_by(column);
    }
}

trait SnapshotExt {
    fn find(&self, id: &volumes::VolumeId) -> Option<&volumes::Volume>;
}

impl SnapshotExt for volumes::Snapshot {
    fn find(&self, id: &volumes::VolumeId) -> Option<&volumes::Volume> {
        self.volumes.iter().find(|volume| &volume.id == id)
    }
}
