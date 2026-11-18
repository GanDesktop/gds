use super::model::Row;
use super::name_form::{NameForm, NamePurpose};
use super::{model::*, ui::Browser};
use crate::{
    entities::location::{Location, scheme::TagAddress, volumes},
    features::file_operations::{
        self as operations, history, names, new_folder, paste, recycle, rename, transfer,
    },
    shared::lib::clipboard::{self, Clipboard},
};
use anti_webui::prelude::*;
use gds::{
    i18n,
    service::desktop::{Desktop, OpenMode},
};
use std::{cell::RefCell, ffi::OsString, path::PathBuf, rc::Rc};
use tokio_util::sync::CancellationToken;

impl Browser {
    pub(super) fn origin(&self) -> Origin {
        self.read(|state| Origin {
            tab: state.tab().id,
            location: state.tab().navigation.location().clone(),
        })
    }
    fn operation_origin(&self) -> Option<Origin> {
        self.read(|state| match &state.operation {
            Operation::Running { origin, .. } => Some(origin.clone()),
            Operation::Idle => None,
        })
    }
    fn refresh_origin(&self, origin: &Origin, reveal: Vec<PathBuf>) {
        let current = self.update(|state| {
            let Some(tab) = state
                .tab_by_id(origin.tab)
                .filter(|tab| tab.navigation.location() == &origin.location)
            else {
                return false;
            };
            tab.reveal = reveal
                .into_iter()
                .filter(|path| path.parent() == tab.navigation.path())
                .collect();
            true
        });
        if current {
            self.refresh_tab(origin.tab);
        }
    }
    pub(super) fn refresh_affected(&self, paths: &[PathBuf]) {
        let ids = self.read(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| {
                    matches!(tab.contents, Contents::Trash(_))
                        || tab.navigation.path().is_some_and(|directory| {
                            paths.iter().any(|path| {
                                path.parent() == Some(directory) || directory.starts_with(path)
                            })
                        })
                })
                .map(|tab| tab.id)
                .collect::<Vec<_>>()
        });
        for id in ids {
            self.refresh_tab(id);
        }
        self.0.volumes.refresh_capacity();
    }
    pub(super) fn begin_operation(&self, label: &str) -> Option<CancellationToken> {
        self.begin_operation_at(label, self.origin())
    }
    fn begin_operation_at(&self, label: &str, origin: Origin) -> Option<CancellationToken> {
        self.begin_operation_message(i18n::text(label), origin)
    }
    fn begin_operation_message(&self, label: String, origin: Origin) -> Option<CancellationToken> {
        if self.read(|state| state.operation.busy()) {
            return None;
        }
        let cancel = self.0.tasks.token().child_token();
        self.update(|state| {
            state.operation = Operation::Running {
                label,
                cancel: cancel.clone(),
                origin,
            }
        });
        Some(cancel)
    }
    pub(super) fn end_operation(&self) {
        self.update(|state| {
            state.operation.cancel();
            state.operation = Operation::Idle;
        });
    }
    pub(super) fn open_selected(&self) {
        let index = self.read(|state| {
            state
                .tab()
                .navigation
                .view()
                .cursor
                .as_ref()
                .and_then(|cursor| state.tab().rows.iter().position(|row| row.key() == *cursor))
                .or_else(|| {
                    state.tab().rows.iter().position(|row| {
                        state.tab().navigation.view().selection.contains(&row.key())
                    })
                })
        });
        if let Some(index) = index {
            self.open_row(index, false);
        }
    }
    pub(super) fn open_row(&self, index: usize, new_tab: bool) {
        let row = self.read(|state| state.tab().rows.get(index).cloned());
        let Some(row) = row else { return };
        match row {
            Row::File(entry) => {
                if self.read(|state| state.picker.is_some()) {
                    self.activate_picker_path(entry.path.clone());
                    return;
                }
                if crate::entities::file::ui::FileIcon::is_directory(&entry) {
                    if new_tab {
                        self.new_tab(Some(Location::from(entry.path.as_path())));
                    } else {
                        self.open(Location::from(entry.path.as_path()));
                    }
                } else {
                    self.open_file(entry.path.clone(), OpenMode::DefaultApplication);
                }
            }
            Row::Place(location) => {
                if new_tab {
                    self.new_tab(Some(location));
                } else {
                    self.open(location);
                }
            }
            Row::Tag(tag) => match TagAddress::new(&tag) {
                Ok(tag) => self.open(Location::Tags(tag)),
                Err(error) => self.report_notice(error.to_string()),
            },
            Row::Remote(entry) => self.activate_remote(entry, new_tab),
            Row::Volume(volume) => self.open_volume(&volume.id, new_tab),
            Row::Trash(_) => {}
        }
    }
    pub(super) fn open_file(&self, path: PathBuf, mode: OpenMode) {
        self.work(
            "Open file",
            async move {
                Desktop::default()
                    .open_file(&path, mode)
                    .await
                    .map_err(|error| error.to_string())
            },
            |browser, result| {
                if let Err(error) = result {
                    browser.report_notice(error);
                }
            },
        );
    }
    pub(super) fn new_folder(&self) {
        let target = self.read(|state| match &state.tab().contents {
            Contents::Remote { listing, session } => Some(operations::remote::Resource::Remote {
                address: listing.address.clone(),
                session: session.clone(),
            }),
            _ => state
                .tab()
                .navigation
                .path()
                .map(|path| operations::remote::Resource::Local(path.to_owned())),
        });
        let Some(target) = target else { return };
        let origin = self.origin();
        if let operations::remote::Resource::Local(directory) = &target {
            if self.0.volumes.removal_in_progress(directory) {
                self.report_notice(i18n::text("device-path-unavailable"));
                return;
            }
            let Some(cancel) = self.begin_operation_at("new-folder-working", origin.clone()) else {
                return;
            };
            let directory = directory.clone();
            let default = OsString::from(i18n::text("new-folder-default-name"));
            self.blocking(
                "Suggest folder name",
                move || {
                    names::free_name(&directory, &default, &Default::default())
                        .map_err(|error| error.to_string())
                },
                move |browser, result| {
                    let cancelled = cancel.is_cancelled();
                    browser.end_operation();
                    if !cancelled
                        && browser.origin().tab == origin.tab
                        && browser.origin().location == origin.location
                    {
                        match result {
                            Ok(suggestion) => browser.open_new_folder(target, origin, suggestion),
                            Err(error) => browser.report_notice(error),
                        }
                    }
                },
            );
        } else {
            self.open_new_folder(target, origin, OsString::new());
        }
    }
    fn open_new_folder(
        &self,
        target: operations::remote::Resource,
        origin: Origin,
        suggestion: OsString,
    ) {
        let (purpose, title) = match &target {
            operations::remote::Resource::Local(directory) => (
                NamePurpose::NewFolder,
                gds::t!(
                    "new-folder-title",
                    folder = directory
                        .file_name()
                        .unwrap_or(directory.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                ),
            ),
            operations::remote::Resource::Remote { .. } => {
                (NamePurpose::NewFolder, i18n::text("new-folder"))
            }
        };
        let browser = self.clone();
        self.name_form(NameForm::new(purpose, title, suggestion), move |name| {
            if let operations::remote::Resource::Local(directory) = &target
                && browser.0.volumes.removal_in_progress(directory)
            {
                browser.report_notice(i18n::text("device-path-unavailable"));
                return;
            }
            let Some(cancel) = browser.begin_operation_at("new-folder-working", origin.clone())
            else {
                return;
            };
            match &target {
                operations::remote::Resource::Local(directory) => {
                    let request = new_folder::Request {
                        directory: directory.clone(),
                        name,
                    };
                    browser.blocking(
                        "Create folder",
                        move || {
                            if cancel.is_cancelled() {
                                return Err("operation cancelled".into());
                            }
                            Ok(request.execute())
                        },
                        |browser, result| {
                            let origin = browser.operation_origin();
                            browser.end_operation();
                            match result {
                                Ok(Ok(created)) => {
                                    browser.report_success(gds::i18n::t!(
                                        "new-folder-done",
                                        name = created
                                            .path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned()
                                    ));
                                    browser.update(|state| {
                                        if let Some(snapshot) = created.snapshot {
                                            state.history.record(history::Entry::new(
                                                history::Kind::NewFolder,
                                                vec![history::Step::trash_empty_directory(
                                                    created.path.clone(),
                                                    snapshot,
                                                )],
                                            ));
                                        } else {
                                            state.history.clear();
                                        }
                                    });
                                    if let Some(origin) = origin {
                                        browser.refresh_origin(&origin, vec![created.path]);
                                    }
                                }
                                Ok(Err(error)) => browser.report_notice(match error {
                                    new_folder::Error::Name(error) => i18n::text(error.label_id()),
                                    new_folder::Error::Exists { name, .. } => {
                                        gds::t!("new-folder-exists", name = name)
                                    }
                                    error => {
                                        gds::t!("new-folder-failed", error = error.to_string())
                                    }
                                }),
                                Err(error) => browser.report_notice(error),
                            }
                        },
                    );
                }
                operations::remote::Resource::Remote { address, session } => {
                    let target =
                        address.child(name.to_str().expect("name form validates remote names"));
                    let session = session.clone();
                    browser.work(
                        "Create remote folder",
                        async move {
                            let target = target.map_err(|error| error.to_string())?;
                            session
                                .mkdir(&target)
                                .await
                                .map_err(|error| error.to_string())
                        },
                        |browser, result| {
                            let origin = browser.operation_origin();
                            browser.end_operation();
                            match result {
                                Ok(()) => {
                                    if let Some(origin) = origin {
                                        browser.refresh_origin(&origin, Vec::new());
                                    }
                                }
                                Err(error) => browser.report_notice(error),
                            }
                        },
                    );
                }
            }
        });
    }
    pub(super) fn rename(&self) {
        let selected = self.read(|state| state.tab().selected());
        let [row] = selected.as_slice() else { return };
        match row {
            Row::File(entry) => {
                let source = entry.path.clone();
                let browser = self.clone();
                let origin = self.origin();
                self.name_form(
                    NameForm::new(
                        NamePurpose::Rename,
                        gds::t!("rename-title", name = entry.display_name.clone()),
                        entry.name.clone(),
                    ),
                    move |name| {
                        if browser.0.volumes.removal_in_progress(&source) {
                            browser.report_notice(i18n::text("device-path-unavailable"));
                            return;
                        }
                        let Some(cancel) =
                            browser.begin_operation_at("rename-working", origin.clone())
                        else {
                            return;
                        };
                        let source = source.clone();
                        let original = source.clone();
                        browser.blocking(
                            "Rename file",
                            move || {
                                if cancel.is_cancelled() {
                                    return Err("operation cancelled".into());
                                }
                                Ok(rename::Request { source, name }.execute())
                            },
                            move |browser, result| {
                                let origin = browser.operation_origin();
                                browser.end_operation();
                                match result {
                                    Ok(Ok(renamed)) => {
                                        browser.report_success(gds::i18n::t!(
                                            "rename-done",
                                            name = renamed
                                                .destination
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .into_owned()
                                        ));
                                        browser.update(|state| {
                                            if let Some(expected) = renamed.snapshot {
                                                state.history.record(history::Entry::new(
                                                    history::Kind::Rename,
                                                    vec![history::Step::Move {
                                                        source: renamed.destination.clone(),
                                                        destination: original,
                                                        expected,
                                                    }],
                                                ));
                                            } else {
                                                state.history.clear();
                                            }
                                        });
                                        if let Some(origin) = origin {
                                            browser
                                                .refresh_origin(&origin, vec![renamed.destination]);
                                        }
                                    }
                                    Ok(Err(error)) => browser.report_notice(match error {
                                        rename::Error::Name(error) => i18n::text(error.label_id()),
                                        rename::Error::Exists { name, .. } => {
                                            gds::t!("rename-target-exists", name = name)
                                        }
                                        error => {
                                            gds::t!("rename-failed", error = error.to_string())
                                        }
                                    }),
                                    Err(error) => browser.report_notice(error),
                                }
                            },
                        );
                    },
                );
            }
            Row::Remote(entry) => self.rename_remote(entry.clone()),
            Row::Volume(volume) => self.rename_volume(volume.clone()),
            _ => {}
        }
    }
    pub(super) fn delete(&self, permanent: bool) {
        let rows = self.read(|state| state.tab().selected());
        if rows.is_empty() {
            return;
        }
        if rows.iter().all(|row| matches!(row, Row::Trash(_))) {
            self.restore_trash(true, false);
            return;
        }
        if rows.iter().all(|row| matches!(row, Row::Tag(_))) {
            self.delete_tags(rows);
            return;
        }
        if rows.iter().any(|row| matches!(row, Row::Remote(_))) {
            self.delete_remote(rows);
            return;
        }
        let paths = rows
            .iter()
            .filter_map(|row| row.path().map(|path| path.to_owned()))
            .collect();
        let batch = match operations::Batch::new(paths) {
            Ok(batch) => batch,
            Err(error) => {
                self.report_notice(error.to_string());
                return;
            }
        };
        let origin = self.origin();
        if permanent {
            let browser = self.clone();
            self.confirm_labelled(
                i18n::count("delete-confirm-title", batch.paths().len()),
                i18n::text("delete-confirm-message"),
                i18n::text("delete-permanently"),
                move || browser.execute_delete(batch.clone(), true, origin.clone()),
            );
        } else {
            self.execute_delete(batch, false, origin);
        }
    }
    fn execute_delete(&self, batch: operations::Batch, permanent: bool, origin: Origin) {
        let Some(cancel) = self.begin_operation_at(
            if permanent {
                "delete-permanently"
            } else {
                "move-to-trash"
            },
            origin,
        ) else {
            return;
        };
        self.blocking(
            "Delete Files selection",
            move || {
                Ok(batch.execute_cancellable(
                    if permanent {
                        operations::Kind::PermanentDelete
                    } else {
                        operations::Kind::Trash
                    },
                    &cancel,
                ))
            },
            |browser, result| {
                let origin = browser.operation_origin();
                browser.end_operation();
                match result {
                    Ok(report) => {
                        let succeeded = report
                            .items
                            .iter()
                            .filter(|item| item.result.is_ok())
                            .count();
                        if succeeded > 0 {
                            browser.report_success(i18n::count(
                                if report.kind == operations::Kind::Trash {
                                    "trash-done"
                                } else {
                                    "delete-done"
                                },
                                succeeded,
                            ));
                        }
                        let failures = report
                            .failed_items()
                            .filter_map(|item| item.result.as_ref().err().map(ToString::to_string))
                            .chain(report.history_errors.iter().cloned())
                            .collect::<Vec<_>>();
                        browser.update(|state| {
                            if report.kind == operations::Kind::Trash {
                                state.history.record(history::Entry::new(
                                    history::Kind::Trash,
                                    report
                                        .receipts
                                        .into_iter()
                                        .map(history::Step::Restore)
                                        .collect(),
                                ));
                            } else {
                                state.history.clear();
                            }
                        });
                        if !failures.is_empty() {
                            browser.report_notice(failures.join("\n"));
                        }
                        if let Some(origin) = origin {
                            browser.refresh_origin(&origin, Vec::new());
                        }
                    }
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }
    pub(super) fn replay(&self, redo: bool) {
        let direction = if redo {
            history::Direction::Redo
        } else {
            history::Direction::Undo
        };
        if self.read(|state| state.history.peek(direction).is_none()) {
            return;
        }
        let Some(cancel) = self.begin_operation(if redo { "redo" } else { "undo" }) else {
            return;
        };
        let entry = self
            .update(|state| state.history.take(direction))
            .expect("history entry checked before starting replay");
        self.blocking(
            "Replay file operation",
            move || Ok(entry.execute_cancellable(&cancel)),
            move |browser, result| {
                browser.end_operation();
                match result {
                    Ok(mut report) => {
                        let errors = report.errors.clone();
                        let changed = report.changed.clone();
                        browser.update(|state| state.history.finish(direction, &mut report));
                        if !errors.is_empty() {
                            browser.report_notice(errors.join("\n"));
                        } else {
                            browser.report_success(i18n::text(if redo {
                                "redo-done"
                            } else {
                                "undo-done"
                            }));
                        }
                        browser.refresh_affected(&changed);
                    }
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }
    pub(super) fn copy(&self, cut: bool) {
        let selected = self.read(|state| state.tab().selected());
        let clipboard = Clipboard::current();
        let browser = self.clone();
        if selected.iter().any(|row| matches!(row, Row::Remote(_))) {
            if cut {
                self.report_notice(i18n::text("remote-cut-unavailable"));
                return;
            }
            let uris = selected
                .iter()
                .filter_map(|row| match row {
                    Row::Remote(entry) => Some(entry.address.uri().to_owned()),
                    Row::File(entry) => Location::from(entry.path.as_path()).uri().ok(),
                    _ => None,
                })
                .collect();
            self.0.owner.with(|| {
                clipboard.write_uris(uris).then(move |result| {
                    if let Err(error) = result {
                        browser.report_notice(error.to_string());
                    }
                })
            });
        } else {
            let paths = selected
                .iter()
                .filter_map(|row| match row {
                    Row::Place(location) => location.local_path(),
                    _ => row.path(),
                })
                .map(ToOwned::to_owned)
                .collect();
            self.0.owner.with(|| {
                clipboard
                    .write(
                        if cut {
                            clipboard::Mode::Cut
                        } else {
                            clipboard::Mode::Copy
                        },
                        paths,
                    )
                    .then(move |result| {
                        if let Err(error) = result {
                            browser.report_notice(error.to_string());
                        }
                        browser.changed();
                    })
            });
        }
    }
    pub(super) fn paste(&self) {
        let target = self.read(|state| state.tab().navigation.path().map(PathBuf::from));
        let Some(target) = target else {
            self.paste_uris();
            return;
        };
        let origin = self.origin();
        let browser = self.clone();
        self.0.owner.with(|| {
            Clipboard::current()
                .read_payload()
                .then(move |result| match result {
                    Ok(clipboard::Payload::Local(snapshot)) => {
                        browser.paste_snapshot(snapshot, target, origin)
                    }
                    Ok(clipboard::Payload::Uris { mode, uris }) => browser.paste_uri_payload(
                        mode,
                        uris,
                        operations::remote::Resource::Local(target),
                    ),
                    Ok(clipboard::Payload::Empty) => {
                        browser.report_notice(i18n::text("clipboard-no-files"))
                    }
                    Err(error) => browser.report_notice(error.to_string()),
                })
        });
    }
    fn paste_snapshot(&self, snapshot: clipboard::Snapshot, target: PathBuf, origin: Origin) {
        if self.0.volumes.removal_in_progress(&target)
            || snapshot
                .paths
                .iter()
                .any(|path| self.0.volumes.removal_in_progress(path))
        {
            self.report_notice(i18n::text("device-path-unavailable"));
            return;
        }
        let Some(cancel) = self.begin_operation_at("paste-working", origin) else {
            return;
        };
        let mode = match snapshot.mode {
            clipboard::Mode::Copy => transfer::Mode::Copy,
            clipboard::Mode::Cut => transfer::Mode::Move,
        };
        let job = paste::PasteJob::new(target, mode, snapshot.paths.clone());
        self.inspect_paste(job, Some(snapshot), cancel);
    }
    pub(super) fn inspect_paste(
        &self,
        job: paste::PasteJob,
        snapshot: Option<clipboard::Snapshot>,
        cancel: CancellationToken,
    ) {
        self.blocking(
            "Inspect paste destinations",
            move || Ok(job.inspect()),
            move |browser, result| match result {
                Ok(paste::Inspection::Ready(job)) => browser.run_paste(job, snapshot, cancel),
                Ok(paste::Inspection::Conflict { job, conflict }) => {
                    let held = Rc::new(RefCell::new(Some(job)));
                    let apply = browser.clone();
                    let drop_browser = browser.clone();
                    let cancel_job = held.clone();
                    let cancel_snapshot = snapshot.clone();
                    let name = conflict
                        .collision
                        .source
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    browser.name_form_with_cancel(
                        NameForm::new(
                            NamePurpose::Conflict,
                            gds::t!("transfer-conflict-title", name = name),
                            conflict.suggestion,
                        ),
                        move |name| {
                            if let Some(mut job) = held.borrow_mut().take() {
                                job.resolve_conflict(conflict.collision.source.clone(), name);
                                apply.inspect_paste(job, snapshot.clone(), cancel.clone());
                            }
                        },
                        move || {
                            if let Some(job) = cancel_job.borrow_mut().take() {
                                drop_browser.finish_paste(job, cancel_snapshot.clone());
                            }
                        },
                    );
                }
                Ok(paste::Inspection::Failed { mut job, error }) => {
                    job.record_abort(error.to_string());
                    browser.finish_paste(job, snapshot);
                }
                Err(error) => {
                    browser.end_operation();
                    browser.report_notice(error);
                }
            },
        );
    }
    fn run_paste(
        &self,
        job: paste::PasteJob,
        snapshot: Option<clipboard::Snapshot>,
        cancel: CancellationToken,
    ) {
        let batch = match job.prepare_batch() {
            Ok(batch) => batch.with_buffer_size(
                self.0
                    .preferences
                    .snapshot()
                    .files
                    .transfer
                    .local_buffer_bytes(),
            ),
            Err(error) => {
                self.end_operation();
                self.report_notice(error.to_string());
                return;
            }
        };
        let worker_cancel = cancel.clone();
        self.blocking(
            "Transfer files",
            move || Ok(batch.execute_cancellable(worker_cancel)),
            move |browser, result| match result {
                Ok(report) => match job.apply_pass(report) {
                    paste::PassOutcome::Completed(job) => browser.finish_paste(job, snapshot),
                    paste::PassOutcome::RetryConflict(job) => {
                        browser.inspect_paste(job, snapshot, cancel)
                    }
                },
                Err(error) => {
                    let mut job = job;
                    job.record_abort(error);
                    browser.finish_paste(job, snapshot);
                }
            },
        );
    }
    fn finish_paste(&self, job: paste::PasteJob, snapshot: Option<clipboard::Snapshot>) {
        let origin = self.operation_origin();
        let summary = job.into_summary(None);
        let kind = summary.history_kind();
        let reset_history = summary.requires_history_reset();
        let committed = summary.committed;
        let succeeded = summary.succeeded;
        let failures = summary.failures;
        if !succeeded.is_empty() {
            self.report_success(i18n::count(
                if kind == history::Kind::Move {
                    "move-done"
                } else {
                    "copy-done"
                },
                succeeded.len(),
            ));
        }
        self.update(|state| {
            state.operation.cancel();
            state.operation = Operation::Idle;
            if reset_history {
                state.history.clear();
            } else {
                state
                    .history
                    .record(history::Entry::new(kind, summary.undo));
            }
        });
        if !failures.is_empty() {
            self.report_notice(failures.join("\n"));
        }
        if let Some(snapshot) = snapshot {
            let browser = self.clone();
            self.0.owner.with(|| {
                Clipboard::current()
                    .finish_cut(snapshot, succeeded)
                    .then(move |result| {
                        if let Err(error) = result {
                            browser.report_notice(error.to_string());
                        }
                    })
            });
        }
        if let Some(origin) = origin {
            self.refresh_origin(&origin, committed);
        }
    }
    pub(super) fn restore_trash(&self, purge: bool, all: bool) {
        let entries = self.read(|state| {
            if all {
                match &state.tab().contents {
                    Contents::Trash(entries) => entries.clone(),
                    _ => Vec::new(),
                }
            } else {
                state
                    .tab()
                    .selected()
                    .into_iter()
                    .filter_map(|row| match row {
                        Row::Trash(entry) => Some(entry),
                        _ => None,
                    })
                    .collect()
            }
        });
        if entries.is_empty() {
            return;
        }
        let origin = self.origin();
        let count = entries.len();
        if purge {
            let browser = self.clone();
            self.confirm_labelled(
                if all {
                    i18n::text("recycle-empty-confirm-title")
                } else {
                    i18n::count("recycle-purge-confirm-title", count)
                },
                if all {
                    i18n::count("recycle-empty-confirm-message", count)
                } else {
                    i18n::text("recycle-purge-confirm-message")
                },
                i18n::text(if all {
                    "recycle-empty-confirm-ok"
                } else {
                    "recycle-purge-confirm-ok"
                }),
                move || {
                    browser.execute_trash(entries.clone(), recycle::Action::Purge, origin.clone())
                },
            );
        } else {
            self.execute_trash(entries, recycle::Action::Restore, origin);
        }
    }
    fn execute_trash(&self, entries: Vec<recycle::Entry>, action: recycle::Action, origin: Origin) {
        let label = i18n::count(
            if action == recycle::Action::Restore {
                "recycle-restore-working"
            } else {
                "recycle-purge-working"
            },
            entries.len(),
        );
        let Some(cancel) = self.begin_operation_message(label, origin) else {
            return;
        };
        let affected = entries
            .iter()
            .map(|entry| entry.original_path.clone())
            .collect::<Vec<_>>();
        self.blocking(
            "Change trash entries",
            move || Ok(recycle::perform_cancellable(action, entries, &cancel)),
            move |browser, result| {
                browser.end_operation();
                match result {
                    Ok(outcomes) => {
                        let succeeded = outcomes
                            .iter()
                            .filter(|outcome| outcome.result.is_ok())
                            .count();
                        if succeeded > 0 {
                            browser.report_success(i18n::count(
                                if action == recycle::Action::Restore {
                                    "recycle-restore-done"
                                } else {
                                    "recycle-purge-done"
                                },
                                succeeded,
                            ));
                        }
                        let mut undo = Vec::new();
                        let mut failures = Vec::new();
                        for outcome in outcomes {
                            match outcome.result {
                                Ok(()) => {
                                    if let Some(expected) = outcome.restored {
                                        undo.push(history::Step::undo_restore(
                                            &outcome.entry,
                                            expected,
                                        ));
                                    }
                                }
                                Err(error) => failures.push(error.to_string()),
                            }
                        }
                        browser.update(|state| {
                            if action == recycle::Action::Restore {
                                state
                                    .history
                                    .record(history::Entry::new(history::Kind::Restore, undo));
                            } else {
                                state.history.clear();
                            }
                        });
                        if !failures.is_empty() {
                            browser.report_notice(failures.join("\n"));
                        }
                        browser.refresh_affected(&affected);
                    }
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }
    pub(super) fn open_volume(&self, id: &volumes::VolumeId, new_tab: bool) {
        let snapshot = self.0.volumes.snapshot();
        let Some(volume) = snapshot.volume(id) else {
            self.report_notice(i18n::text("device-disconnected"));
            return;
        };
        if self.0.volumes.is_busy(&volume.id)
            || (volume.mount_points.is_empty() && (!volume.mountable || volume.locked))
        {
            self.report_notice(i18n::text(if volume.locked {
                "device-unlock-unavailable"
            } else {
                "device-not-browsable"
            }));
            return;
        }
        let location = Location::Volume {
            identity: Self::volume_identity(&snapshot, &volume.id),
            relative: Default::default(),
        };
        if !volume.mount_points.is_empty() {
            if new_tab {
                self.new_tab(Some(location));
            } else {
                self.open(location);
            }
            return;
        }
        let tab = self.read(|state| state.tab().id);
        let browser = self.downgrade();
        self.0.owner.with(|| {
            match self
                .0
                .volumes
                .operate(volumes::DeviceAction::Mount(volume.id.clone()))
            {
                Ok(task) => task.then(move |result| {
                    let Some(browser) = browser.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(_) if new_tab => browser.new_tab(Some(location)),
                        Ok(_) => browser.open_in_tab(tab, location),
                        Err(error) => browser.report_notice(error.to_string()),
                    }
                }),
                Err(error) => self.report_notice(error.to_string()),
            }
        });
    }
    pub(super) fn reconnect_volume(&self) {
        let origin = self.origin();
        let Location::Volume { identity, .. } = &origin.location else {
            return;
        };
        let Some(volume) = self.volume_at_location(identity) else {
            self.0.volumes.refresh();
            self.report_notice(i18n::text("device-reconnect-missing"));
            return;
        };
        if !volume.mount_points.is_empty() {
            self.refresh_tab(origin.tab);
            return;
        }
        if volume.locked || !volume.mountable {
            self.report_notice(i18n::text(if volume.locked {
                "device-unlock-unavailable"
            } else {
                "device-not-browsable"
            }));
            return;
        }
        let weak = self.downgrade();
        self.0.owner.with(|| {
            match self
                .0
                .volumes
                .operate(volumes::DeviceAction::Mount(volume.id))
            {
                Ok(task) => task.then(move |result| {
                    let Some(browser) = weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(_) => {
                            if browser.read(|state| {
                                state.tabs.iter().any(|tab| {
                                    tab.id == origin.tab
                                        && tab.navigation.location() == &origin.location
                                })
                            }) {
                                browser.refresh_tab(origin.tab);
                            }
                        }
                        Err(error) => browser.report_notice(error.to_string()),
                    }
                }),
                Err(error) => self.report_notice(error.to_string()),
            }
        });
    }
    pub(super) fn sort_by(&self, column: crate::entities::file::columns::Column) {
        let id = self.update(|state| {
            let tab = state.tab_mut();
            tab.sort = tab.sort.next(column);
            let id = tab.id;
            let location = tab.navigation.location().clone();
            let sort = tab.sort;
            state.places.set_sort(location, sort);
            id
        });
        self.reproject(id);
        self.save_places();
        self.enrich_directory(id);
    }
    pub(super) fn select_context(&self, index: usize) {
        if self.read(|state| {
            state
                .tab()
                .rows
                .get(index)
                .is_some_and(|row| !state.tab().navigation.view().selection.contains(&row.key()))
        }) {
            self.select_row(index, Modifiers::default());
        }
    }
}
