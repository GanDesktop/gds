//! Local filesystem mutations: folder/file/symlink creation, rename, delete, undo/redo, copy, recent, and trash.

use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use gds::i18n as gds_i18n;
use gds::ui::IconName;
use tokio::sync::mpsc;
use zgui::reactive::spawn_local;

use super::super::{
    navigation::Navigator,
    projection::Rows,
    state::{
        context::Context,
        model::{ConfirmOptions, Contents, FILES_TASK_KEY, Row, TRASH_TASK_KEY, remote_task_key},
        name_form::{NameForm, NamePurpose},
    },
};
use super::lifecycle::{Lifecycle, freeze_name_preview, next_free_remote_name};
use crate::{
    entities::{file::directory, location::Location},
    features::file_operations::{
        Batch, Kind as DeleteKind, history, names, new_folder, recycle, rename, templates,
    },
    shared::{
        i18n,
        lib::clipboard::{self, Clipboard},
    },
};

pub(in crate::pages::workspace) struct LocalOps<'a>(pub &'a Context);

impl LocalOps<'_> {
    pub(in crate::pages::workspace) fn remove_from_recent(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.0.blocking(
            "Remove recent files",
            move || {
                for path in &paths {
                    gds::service::desktop::recent_files::remove_default(path)
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            },
            |ctx, result| match result {
                Ok(()) => Navigator(ctx).refresh_location(&Location::Recent),
                Err(error) => ctx.report_notice(error),
            },
        );
    }

    pub(in crate::pages::workspace) fn clear_recent(&self) {
        let title = i18n::text_untracked("clear-recent-confirm-title");
        let message = i18n::text_untracked("clear-recent-confirm-message");
        let primary = i18n::text_untracked("clear-recent-confirm-ok");
        let options = ConfirmOptions {
            icon: Some(IconName::Broom),
            primary_icon: Some(IconName::Broom),
            destructive: true,
            items: Vec::new(),
            overflow_count: 0,
        };
        let ctx = self.0.clone();
        self.0
            .confirm_with_options(title, message, primary, options, move || {
                let recent_tabs: Vec<u64> = ctx.update(|state| {
                    state.places.clear_recent();
                    state
                        .tabs
                        .iter()
                        .filter(|tab| matches!(tab.navigation.location(), Location::Recent))
                        .map(|tab| tab.id)
                        .collect()
                });
                ctx.save_places();
                ctx.blocking(
                    "Clear recent files",
                    || {
                        gds::service::desktop::recent_files::clear_default()
                            .map_err(|error| error.to_string())
                    },
                    move |ctx, result| {
                        let navigator = Navigator(ctx);
                        for id in &recent_tabs {
                            navigator.refresh_tab(*id);
                        }
                        match result {
                            Ok(()) => {
                                ctx.report_success(i18n::text_untracked("clear-recent-done"));
                            }
                            Err(error) => ctx.report_notice(error),
                        }
                    },
                );
            });
    }

    pub(in crate::pages::workspace) fn new_folder(&self) {
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

        let rows = Rows(self.0);
        let target = self.0.read_untracked(|state| {
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
                    let folder_label = rows.location_title(tab.navigation.location());
                    let reserved: HashSet<OsString> = tab
                        .navigation
                        .directory_entries()
                        .map(|entries| entries.iter().map(|entry| entry.name.clone()).collect())
                        .unwrap_or_default();
                    Some(NewFolderTarget::Local {
                        directory,
                        folder_label,
                        reserved,
                    })
                }
                Contents::Remote { listing, session } => {
                    if state.operation.busy()
                        || self
                            .0
                            .is_background_task_busy_untracked(&remote_task_key(tab.id))
                    {
                        return None;
                    }
                    let folder_label = rows.location_title(tab.navigation.location());
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
                let ctx = self.0.clone();
                self.0.name_form(form, move |name| {
                    let Some(cancel) = Lifecycle(&ctx)
                        .begin_operation(&i18n::text_untracked("new-folder-working"))
                    else {
                        return;
                    };
                    let parent = directory.clone();
                    let request = new_folder::Request {
                        directory: parent.clone(),
                        name,
                    };
                    let task = request.spawn(&ctx.scope(), cancel);
                    let weak = ctx.downgrade();
                    ctx.0.owner.with(|| {
                        drop(spawn_local(async move {
                            let result = task.await;
                            let Some(ctx) = weak.upgrade() else {
                                return;
                            };
                            let lifecycle = Lifecycle(&ctx);
                            lifecycle.end_operation();
                            match result {
                                Ok(Ok(created)) => {
                                    let label = created
                                        .path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    if let Some(expected) = created.snapshot {
                                        lifecycle.record_history(history::Entry::new(
                                            history::Kind::NewFolder,
                                            vec![history::Step::trash_empty_directory(
                                                created.path.clone(),
                                                expected,
                                            )],
                                        ));
                                    }
                                    Navigator(&ctx).reveal_paths(vec![created.path.clone()]);
                                    lifecycle.refresh_affected(&[parent]);
                                    ctx.report_success(gds::t_untracked!(
                                        "new-folder-done",
                                        name = label
                                    ));
                                    ctx.focus_content();
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
                                    ctx.report_notice(message);
                                }
                                Err(error) => {
                                    ctx.report_notice(gds::t_untracked!(
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
                let ctx = self.0.clone();
                self.0.name_form(form, move |os_name| {
                    let Some(name_str) = os_name.to_str() else {
                        ctx.report_notice(i18n::text_untracked("rename-not-text"));
                        return;
                    };
                    let Ok(child_address) = parent_address.child(name_str) else {
                        ctx.report_notice(i18n::text_untracked("rename-error-invalid"));
                        return;
                    };
                    let task_key = remote_task_key(tab_id);
                    let Some(_cancel) = ctx.begin_background_task(
                        task_key.clone(),
                        i18n::text_untracked("new-folder-working"),
                    ) else {
                        return;
                    };
                    let session = session.clone();
                    let parent_location = Location::Remote(parent_address.clone());
                    let display_name = name_str.to_owned();
                    let (tx, mut rx) = mpsc::unbounded_channel();
                    drop(ctx.scope().spawn_named("Create remote folder", async move {
                        let result = session.mkdir(&child_address).await;
                        let _ = tx.send(result);
                    }));
                    let weak = ctx.downgrade();
                    ctx.0.owner.with(|| {
                        drop(spawn_local(async move {
                            let Some(result) = rx.recv().await else {
                                return;
                            };
                            let Some(ctx) = weak.upgrade() else {
                                return;
                            };
                            let _ = ctx.finish_background_task(&task_key);
                            match result {
                                Ok(()) => {
                                    Navigator(&ctx).refresh_location(&parent_location);
                                    ctx.report_success(gds::t_untracked!(
                                        "new-folder-done",
                                        name = display_name
                                    ));
                                }
                                Err(error) => {
                                    ctx.report_notice(gds::t_untracked!(
                                        "new-folder-failed",
                                        error = error.to_string()
                                    ));
                                }
                            }
                            ctx.focus_content();
                        }));
                    });
                });
            }
        }
    }

    pub(in crate::pages::workspace) fn create_from_template(
        &self,
        directory: PathBuf,
        template: PathBuf,
    ) {
        if self.0.0.volumes.removal_in_progress(&directory)
            || self.0.read_untracked(|state| {
                state.operation.busy() || state.tab().navigation.is_loading()
            })
        {
            return;
        }
        let Some(cancel) =
            Lifecycle(self.0).begin_operation(&i18n::text_untracked("template-create-working"))
        else {
            return;
        };
        let parent = directory.clone();
        let request = templates::Request {
            template,
            directory,
        };
        let task = request.spawn(&self.0.scope(), cancel);
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                let lifecycle = Lifecycle(&ctx);
                lifecycle.end_operation();
                match result {
                    Ok(Ok(created)) => {
                        let label = created
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if recycle::supported()
                            && let Some(expected) = created.snapshot
                        {
                            lifecycle.record_history(history::Entry::new(
                                history::Kind::NewFile,
                                vec![history::Step::trash(created.path.clone(), expected)],
                            ));
                        }
                        Navigator(&ctx).reveal_paths(vec![created.path.clone()]);
                        lifecycle.refresh_affected(&[parent]);
                        ctx.report_success(gds::t_untracked!("template-create-done", name = label));
                        ctx.focus_content();
                    }
                    Ok(Err(error)) => {
                        ctx.report_notice(gds::t_untracked!(
                            "template-create-failed",
                            error = error.to_string()
                        ));
                    }
                    Err(error) => {
                        ctx.report_notice(gds::t_untracked!(
                            "template-create-failed",
                            error = error.to_string()
                        ));
                    }
                }
            }));
        });
    }

    #[cfg(unix)]
    pub(in crate::pages::workspace) fn create_symlink(&self, target: PathBuf) {
        let directory = self.0.read_untracked(|state| {
            if state.operation.busy() {
                return None;
            }
            let tab = state.tab();
            if !matches!(tab.contents, Contents::Directory) || tab.navigation.is_loading() {
                return None;
            }
            tab.navigation.path().map(Path::to_path_buf)
        });
        let Some(directory) = directory else {
            return;
        };
        if self.0.0.volumes.removal_in_progress(&directory) {
            return;
        }
        let Some(cancel) =
            Lifecycle(self.0).begin_operation(&i18n::text_untracked("create-symlink-working"))
        else {
            return;
        };
        let parent = directory.clone();
        let request = crate::features::file_operations::symlink::Request {
            target,
            directory,
            link_suffix: i18n::text_untracked("create-symlink-suffix"),
        };
        let task = request.spawn(&self.0.scope(), cancel);
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                let lifecycle = Lifecycle(&ctx);
                lifecycle.end_operation();
                match result {
                    Ok(Ok(created)) => {
                        let label = created
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if let Some(expected) = created.snapshot {
                            lifecycle.record_history(history::Entry::new(
                                history::Kind::CreateLink,
                                vec![history::Step::RemoveSymlink {
                                    path: created.path.clone(),
                                    target: created.target,
                                    expected,
                                }],
                            ));
                        }
                        Navigator(&ctx).reveal_paths(vec![created.path.clone()]);
                        lifecycle.refresh_affected(&[parent]);
                        ctx.report_success(gds::t_untracked!("create-symlink-done", name = label));
                        ctx.focus_content();
                    }
                    Ok(Err(error)) => {
                        ctx.report_notice(gds::t_untracked!(
                            "create-symlink-failed",
                            error = error.to_string()
                        ));
                    }
                    Err(error) => {
                        ctx.report_notice(gds::t_untracked!(
                            "create-symlink-failed",
                            error = error.to_string()
                        ));
                    }
                }
            }));
        });
    }

    pub(super) fn rename_local(&self, entry: directory::Entry) {
        let source = entry.path.clone();
        let current_name = entry.name.clone();
        let display = current_name.to_string_lossy().into_owned();
        let form = NameForm::new(
            NamePurpose::Rename,
            gds::t_untracked!("rename-title", name = display),
            current_name.clone(),
        );
        let ctx = self.0.clone();
        self.0.name_form(form, move |name| {
            if name == current_name {
                ctx.focus_content();
                return;
            }
            let Some(cancel) =
                Lifecycle(&ctx).begin_operation(&i18n::text_untracked("rename-working"))
            else {
                return;
            };
            let src = source.clone();
            let request = rename::Request {
                source: src.clone(),
                name,
            };
            let task = request.spawn(&ctx.scope(), cancel);
            let weak = ctx.downgrade();
            ctx.0.owner.with(|| {
                drop(spawn_local(async move {
                    let result = task.await;
                    let Some(ctx) = weak.upgrade() else {
                        return;
                    };
                    let lifecycle = Lifecycle(&ctx);
                    lifecycle.end_operation();
                    match result {
                        Ok(Ok(renamed)) => {
                            let display_new = renamed
                                .destination
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            if let Some(expected) = renamed.snapshot {
                                lifecycle.record_history(history::Entry::new(
                                    history::Kind::Rename,
                                    vec![history::Step::Move {
                                        source: renamed.destination.clone(),
                                        destination: src.clone(),
                                        expected,
                                    }],
                                ));
                            }
                            Navigator(&ctx).reveal_paths(vec![renamed.destination.clone()]);
                            lifecycle.refresh_affected(&[src, renamed.destination]);
                            ctx.report_success(gds::t_untracked!(
                                "rename-done",
                                name = display_new
                            ));
                            ctx.focus_content();
                        }
                        Ok(Err(error)) => {
                            let message = match error {
                                rename::Error::Exists { name, .. } => {
                                    gds::t_untracked!("rename-target-exists", name = name)
                                }
                                rename::Error::Name(err) => i18n::text_untracked(err.label_id()),
                                other => {
                                    gds::t_untracked!("rename-failed", error = other.to_string())
                                }
                            };
                            ctx.report_notice(message);
                        }
                        Err(error) => {
                            ctx.report_notice(gds::t_untracked!(
                                "rename-failed",
                                error = error.to_string()
                            ));
                        }
                    }
                }));
            });
        });
    }

    pub(super) fn delete_local(&self, paths: Vec<PathBuf>, permanent: bool) {
        if paths.is_empty() {
            return;
        }
        let batch = match Batch::new(paths) {
            Ok(batch) => batch,
            Err(error) => {
                self.0.report_notice(error.to_string());
                return;
            }
        };
        let actually_permanent = permanent || !recycle::supported();
        if actually_permanent {
            let count = batch.paths().len();
            let (items, overflow_count) = freeze_name_preview(batch.paths().iter().map(|path| {
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
            let ctx = self.0.clone();
            let paths = batch.paths().to_vec();
            self.0
                .confirm_with_options(title, message, primary, options, move || {
                    if let Ok(batch) = Batch::new(paths.clone()) {
                        LocalOps(&ctx).execute_delete(batch, DeleteKind::PermanentDelete);
                    }
                });
        } else {
            self.execute_delete(batch, DeleteKind::Trash);
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
        let Some(cancel) = Lifecycle(self.0).begin_operation(&label) else {
            return;
        };
        let inhibit = gds::service::desktop::InhibitGuard::delayed(
            &self.0.scope(),
            gds::service::desktop::DEFAULT_INHIBIT_DELAY,
            i18n::text_untracked("transfer-inhibit-reason"),
        );
        let affected = batch.paths().to_vec();
        let task = self
            .0
            .scope()
            .spawn_blocking_named("Delete files", move || {
                batch.execute_cancellable(kind, &cancel)
            });
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                drop(inhibit);
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                let lifecycle = Lifecycle(&ctx);
                lifecycle.end_operation();
                let Ok(report) = result else {
                    if let Err(error) = result {
                        ctx.report_notice(error.to_string());
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
                    lifecycle.record_history(history::Entry::new(history::Kind::Trash, steps));
                }
                lifecycle.refresh_after_file_operation(&affected, report.kind == DeleteKind::Trash);
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
                    ctx.report_notice(format!("{headline}\n{detail}"));
                } else {
                    let key = if report.kind == DeleteKind::Trash {
                        "trash-done"
                    } else {
                        "delete-done"
                    };
                    ctx.report_success(gds_i18n::count_untracked(key, succeeded.len()));
                }
                if !report.history_errors.is_empty() {
                    ctx.report_notice(report.history_errors.join("; "));
                }
                ctx.focus_content();
            }));
        });
    }

    pub(in crate::pages::workspace) fn replay(&self, redo: bool) {
        if self.0.is_background_task_busy_untracked(FILES_TASK_KEY)
            || self.0.read_untracked(|state| state.operation.busy())
        {
            return;
        }
        let lifecycle = Lifecycle(self.0);
        let Some(entry) = lifecycle.take_history_entry(redo) else {
            return;
        };
        let op_name = i18n::text_untracked(entry.kind.label_id());
        let label = if redo {
            gds::t_untracked!("redo-operation", operation = op_name)
        } else {
            gds::t_untracked!("undo-operation", operation = op_name)
        };
        let Some(cancel) = lifecycle.begin_operation(&label) else {
            if redo {
                lifecycle.clear_history();
            }
            return;
        };
        let task = self
            .0
            .scope()
            .spawn_blocking_named("Replay file operation", move || {
                entry.execute_cancellable(&cancel)
            });
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                let lifecycle = Lifecycle(&ctx);
                lifecycle.end_operation();
                let Ok(mut report) = result else {
                    if let Err(error) = result {
                        ctx.report_notice(error.to_string());
                    }
                    return;
                };
                let changed = report.changed.clone();
                let errors = report.errors.clone();
                let kind = report.kind;
                let trash_affected = matches!(
                    kind,
                    history::Kind::Trash
                        | history::Kind::Restore
                        | history::Kind::NewFolder
                        | history::Kind::NewFile
                );
                lifecycle.finish_history_replay(redo, &mut report);
                lifecycle.refresh_after_file_operation(&changed, trash_affected);
                if errors.is_empty() {
                    ctx.report_success(i18n::text_untracked(if redo {
                        "redo-done"
                    } else {
                        "undo-done"
                    }));
                } else {
                    let headline =
                        i18n::text_untracked(if redo { "redo-failed" } else { "undo-failed" });
                    ctx.report_notice(format!("{headline}: {}", errors.join("; ")));
                }
                ctx.focus_content();
            }));
        });
    }

    pub(in crate::pages::workspace) fn copy(&self, cut: bool) {
        enum CopyTarget {
            Empty,
            Remote(Vec<String>),
            Local(Vec<PathBuf>),
        }
        let target = self.0.read_untracked(|state| {
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
                    self.0
                        .report_notice(i18n::text_untracked("remote-cut-unavailable"));
                    return;
                }
                if uris.is_empty() {
                    return;
                }
                let count = uris.len();
                let weak = self.0.downgrade();
                self.0.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let result = Clipboard::current().write_uris(uris).await;
                        let Some(ctx) = weak.upgrade() else {
                            return;
                        };
                        match result {
                            Ok(()) => {
                                ctx.report_success(gds_i18n::count_untracked(
                                    "copy-prepared",
                                    count,
                                ));
                                ctx.changed();
                            }
                            Err(error) => ctx.report_notice(error.to_string()),
                        }
                    }));
                });
            }
            CopyTarget::Local(paths) => {
                let batch = match Batch::new(paths) {
                    Ok(batch) => batch,
                    Err(error) => {
                        self.0.report_notice(error.to_string());
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
                let weak = self.0.downgrade();
                self.0.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let result = Clipboard::current().write(mode, paths).await;
                        let Some(ctx) = weak.upgrade() else {
                            return;
                        };
                        match result {
                            Ok(_) => {
                                ctx.report_success(gds_i18n::count_untracked(
                                    "copy-prepared",
                                    count,
                                ));
                                ctx.changed();
                            }
                            Err(error) => ctx.report_notice(error.to_string()),
                        }
                    }));
                });
            }
        }
    }

    pub(in crate::pages::workspace) fn restore_trash(&self, purge: bool, all: bool) {
        if self.0.is_background_task_busy_untracked(TRASH_TASK_KEY) {
            return;
        }
        let entries: Vec<recycle::Entry> = self.0.read_untracked(|state| {
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
            let ctx = self.0.clone();
            self.0
                .confirm_with_options(title, message, primary, options, move || {
                    LocalOps(&ctx).execute_trash_action(recycle::Action::Purge, entries.clone());
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
        let Some(cancel) = self.0.begin_background_task(TRASH_TASK_KEY, label) else {
            return;
        };
        let task = self
            .0
            .scope()
            .spawn_blocking_named("Trash operation", move || {
                recycle::perform_cancellable(action, entries, &cancel)
            });
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                let _ = ctx.finish_background_task(TRASH_TASK_KEY);
                let Ok(outcomes) = result else {
                    if let Err(error) = result {
                        ctx.report_notice(error.to_string());
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
                let lifecycle = Lifecycle(&ctx);
                if !undo_steps.is_empty() {
                    lifecycle
                        .record_history(history::Entry::new(history::Kind::Restore, undo_steps));
                }
                lifecycle.refresh_after_file_operation(&changed_paths, true);
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
                    ctx.report_notice(format!("{headline}\n{}", failed.join("\n")));
                } else {
                    let key = match action {
                        recycle::Action::Restore => "recycle-restore-done",
                        recycle::Action::Purge => "recycle-purge-done",
                    };
                    ctx.report_success(gds_i18n::count_untracked(key, succeeded));
                }
                ctx.focus_content();
            }));
        });
    }

    pub(in crate::pages::workspace) fn select_context(&self, index: usize) {
        let changed = self.0.update(|state| {
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
            self.0.update_selection(|_| {});
        }
    }
}
