//! Remote server connections, transfers, rename, URI paste, and deletion workflows.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use gds::i18n as gds_i18n;
use gds::service::{
    desktop::OpenMode,
    remote::{self, EntryKind},
};
use gds::ui::IconName;
use tokio::sync::{mpsc, watch};
use zgui::reactive::spawn_local;

use super::super::{
    navigation::Navigator,
    state::{
        context::{Context, LaunchOptions},
        model::{ConfirmOptions, Contents, FILES_TASK_KEY, Row, Tab, remote_task_key},
        name_form::{NameForm, NamePurpose},
    },
};
use super::{
    lifecycle::{Lifecycle, preview_slice},
    paste::PasteOps,
};
use crate::{
    entities::{
        file::ui::EntryPresentation,
        location::{Location, places::Places, scheme},
    },
    features::{
        connect_server::Intent,
        file_operations::{
            history,
            paste::PasteJob,
            remote::{self as remote_transfer, Resource},
            transfer,
        },
        pick_files,
    },
    shared::i18n,
};

pub(super) fn server_affected_tab_ids(tabs: &[Tab], connection_key: &str) -> Vec<u64> {
    tabs.iter()
        .filter(|tab| {
            let current_matches = matches!(
                tab.navigation.location(),
                Location::Remote(addr) if addr.connection_key() == connection_key
            );
            let pending_matches = matches!(
                tab.navigation.pending_location(),
                Some(Location::Remote(addr)) if addr.connection_key() == connection_key
            );
            current_matches || pending_matches
        })
        .map(|tab| tab.id)
        .collect()
}

pub(super) struct UriPayload {
    pub(super) local: Vec<PathBuf>,
    pub(super) remote: Vec<remote::Address>,
}

pub(super) fn classify_uri_payload(uris: &[String]) -> UriPayload {
    let mut local = Vec::new();
    let mut remote = Vec::new();
    for raw in uris {
        match scheme::parse(raw) {
            Ok(Location::Directory(path)) => local.push(PathBuf::from(path)),
            Ok(Location::Remote(address)) => remote.push(address),
            _ => {
                let candidate = Path::new(raw);
                if candidate.is_absolute() {
                    local.push(candidate.to_path_buf());
                }
            }
        }
    }
    UriPayload { local, remote }
}

pub(super) fn renamed_remote_address(
    address: &remote::Address,
    new_name: &str,
) -> Result<remote::Address, remote::Error> {
    remote::Address::validate_name(new_name, address.protocol())?;
    if let Some(parent) = address.parent() {
        parent.child(new_name)
    } else {
        address.child(new_name)
    }
}

fn remote_preview_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("gds-remote-{stamp}"))
}

pub(in crate::pages::workspace) struct RemoteOps<'a>(pub &'a Context);

impl RemoteOps<'_> {
    pub(in crate::pages::workspace) fn connect_server(&self) {
        self.0.0.menus.close();
        let ctx = self.0.clone();
        self.0
            .0
            .connector
            .prompt(None, Intent::Open, move |result| match result {
                Ok(address) => Navigator(&ctx).open(Location::Remote(address)),
                Err(error) if !error.is_empty() => ctx.report_notice(error),
                Err(_) => {}
            });
    }

    pub(super) fn activate_remote(&self, entry: remote::Entry, new_tab: bool) {
        let in_picker = self.0.read_untracked(|state| state.picker.is_some());
        if in_picker {
            self.0
                .report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        match entry.kind {
            EntryKind::Directory => {
                let location = Location::Remote(entry.address);
                let navigator = Navigator(self.0);
                if new_tab {
                    navigator.new_tab(Some(location));
                } else {
                    navigator.open(location);
                }
            }
            EntryKind::File | EntryKind::SymbolicLink | EntryKind::Other => {
                self.download_remote(vec![entry.address], true);
            }
        }
    }

    fn download_remote(&self, addresses: Vec<remote::Address>, open: bool) {
        if addresses.is_empty() {
            return;
        }
        let tab_id = self.0.read_untracked(|state| state.tab().id);
        let task_key = remote_task_key(tab_id);
        if self.0.is_background_task_busy_untracked(&task_key) {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }
        let Some(first) = addresses.first() else {
            return;
        };
        let session = self
            .0
            .read_untracked(|state| match &state.tab().contents {
                Contents::Remote { session, .. } => Some(session.clone()),
                _ => None,
            })
            .or_else(|| self.0.0.connector.session(first));
        let Some(session) = session else {
            self.0
                .report_notice(i18n::text_untracked("remote-connect-source"));
            return;
        };

        if open && addresses.len() == 1 {
            let addr = addresses[0].clone();
            let preview_dir = remote_preview_dir();
            if let Err(error) = std::fs::create_dir_all(&preview_dir) {
                self.0.report_notice(error.to_string());
                return;
            }
            let expected_local = preview_dir.join(addr.name());
            self.run_remote_copy(
                task_key,
                Some(tab_id),
                vec![Resource::Remote {
                    address: addr,
                    session,
                }],
                Resource::Local(preview_dir),
                false,
                Some(expected_local),
            );
            return;
        }

        let ctx = self.0.clone();
        self.open_transfer_picker(
            i18n::text_untracked("remote-download"),
            pick_files::Options {
                directory: true,
                multiple: false,
                ..pick_files::Options::new(Vec::new(), false)
            },
            move |paths| {
                let Some(dest) = paths.into_iter().next() else {
                    return;
                };
                let sources: Vec<Resource> = addresses
                    .iter()
                    .cloned()
                    .map(|address| Resource::Remote {
                        address,
                        session: session.clone(),
                    })
                    .collect();
                RemoteOps(&ctx).run_remote_copy(
                    task_key.clone(),
                    Some(tab_id),
                    sources,
                    Resource::Local(dest),
                    false,
                    None,
                );
            },
        );
    }

    pub(in crate::pages::workspace) fn upload_remote(&self) {
        let target = self.0.read_untracked(|state| {
            let tab = state.tab();
            match &tab.contents {
                Contents::Remote { listing, session } => {
                    Some((tab.id, listing.address.clone(), session.clone()))
                }
                _ => None,
            }
        });
        let Some((tab_id, dest_address, session)) = target else {
            return;
        };
        let task_key = remote_task_key(tab_id);
        if self.0.is_background_task_busy_untracked(&task_key) {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }
        let ctx = self.0.clone();
        self.open_transfer_picker(
            i18n::text_untracked("remote-upload"),
            pick_files::Options {
                allow_mixed: true,
                multiple: true,
                ..pick_files::Options::new(Vec::new(), false)
            },
            move |paths| {
                if paths.is_empty() {
                    return;
                }
                let sources = paths.into_iter().map(Resource::Local).collect();
                RemoteOps(&ctx).run_remote_copy(
                    task_key.clone(),
                    Some(tab_id),
                    sources,
                    Resource::Remote {
                        address: dest_address.clone(),
                        session: session.clone(),
                    },
                    false,
                    None,
                );
            },
        );
    }

    pub(in crate::pages::workspace) fn download_selected_remote(&self) {
        let addresses: Vec<remote::Address> = self.0.read_untracked(|state| {
            state
                .tab()
                .selected()
                .into_iter()
                .filter_map(|row| match row {
                    Row::Remote(entry) => Some(entry.address),
                    _ => None,
                })
                .collect()
        });
        self.download_remote(addresses, false);
    }

    pub(in crate::pages::workspace) fn disconnect_remote_address(&self, address: remote::Address) {
        let key = address.connection_key();
        let affected_tabs = self
            .0
            .read_untracked(|state| server_affected_tab_ids(&state.tabs, &key));
        if affected_tabs.iter().any(|id| {
            self.0
                .is_background_task_busy_untracked(&remote_task_key(*id))
        }) {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }

        let ctx = self.0.clone();
        self.0.0.connector.disconnect(address, move |result| {
            if let Err(error) = result {
                ctx.report_notice(error);
            }
            ctx.changed();
        });

        let navigator = Navigator(self.0);
        for tab_id in &affected_tabs {
            navigator.open_in_tab(*tab_id, Location::Network);
        }
        if affected_tabs.is_empty() {
            navigator.refresh_location(&Location::Network);
        }
        self.0.changed();
    }

    pub(in crate::pages::workspace) fn disconnect_remote(&self) {
        let address = self.0.read_untracked(|state| {
            let tab = state.tab();
            match tab.navigation.location() {
                Location::Remote(address) => Some(address.clone()),
                _ => tab.selected().into_iter().find_map(|row| match row {
                    Row::Place(Location::Remote(address)) => Some(address),
                    _ => None,
                }),
            }
        });
        if let Some(address) = address {
            self.disconnect_remote_address(address);
        }
    }

    pub(super) fn rename_remote(&self, entry: remote::Entry) {
        let target_info = self.0.read_untracked(|state| {
            let tab = state.tab();
            match &tab.contents {
                Contents::Remote { session, .. } => Some((tab.id, session.clone())),
                _ => None,
            }
        });
        let Some((tab_id, session)) = target_info else {
            return;
        };
        let task_key = remote_task_key(tab_id);
        if self.0.is_background_task_busy_untracked(&task_key) {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }
        let from = entry.address.clone();
        let current_name = entry.name.clone();
        let form = NameForm::new(
            NamePurpose::Rename,
            gds::t_untracked!("rename-title", name = current_name.clone()),
            OsString::from(&current_name),
        );
        let ctx = self.0.clone();
        self.0.name_form(form, move |new_os_name| {
            let Some(new_name) = new_os_name.to_str() else {
                ctx.report_notice(i18n::text_untracked("rename-not-text"));
                return;
            };
            if new_name == current_name {
                return;
            }
            let Ok(target) = renamed_remote_address(&from, new_name) else {
                ctx.report_notice(i18n::text_untracked("rename-error-invalid"));
                return;
            };
            let Some(_cancel) =
                ctx.begin_background_task(task_key.clone(), i18n::text_untracked("rename-working"))
            else {
                return;
            };
            let finish_key = task_key.clone();
            let session = session.clone();
            let from = from.clone();
            let display_name = new_name.to_owned();
            let (tx, mut rx) = mpsc::unbounded_channel();
            drop(ctx.scope().spawn_named("Rename remote item", async move {
                let result = session.rename(&from, &target).await;
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
                    let _ = ctx.finish_background_task(&finish_key);
                    match result {
                        Ok(()) => {
                            Navigator(&ctx).refresh_tab(tab_id);
                            ctx.report_success(gds::t_untracked!(
                                "rename-done",
                                name = display_name
                            ));
                        }
                        Err(error) => {
                            ctx.report_notice(gds::t_untracked!(
                                "rename-failed",
                                error = error.to_string()
                            ));
                        }
                    }
                    ctx.focus_content();
                }));
            });
        });
    }

    pub(super) fn paste_uri_payload(&self, uris: Vec<String>, destination: PathBuf, cut: bool) {
        if cut {
            self.0
                .report_notice(i18n::text_untracked("remote-cut-unavailable"));
            return;
        }
        let parsed = classify_uri_payload(&uris);
        if !parsed.local.is_empty() {
            let job = PasteJob::new(destination.clone(), transfer::Mode::Copy, parsed.local);
            PasteOps(self.0).inspect_paste(job, false, true, None);
        }
        if !parsed.remote.is_empty() {
            let mut resources = Vec::with_capacity(parsed.remote.len());
            for address in parsed.remote {
                let Some(session) = self.0.0.connector.session(&address) else {
                    self.0
                        .report_notice(i18n::text_untracked("remote-connect-source"));
                    return;
                };
                resources.push(Resource::Remote { address, session });
            }
            self.run_remote_copy(
                FILES_TASK_KEY.to_owned(),
                None,
                resources,
                Resource::Local(destination),
                true,
                None,
            );
        }
    }

    pub(super) fn delete_remote(&self, rows: Vec<Row>) {
        let target_info = self.0.read_untracked(|state| {
            let tab = state.tab();
            match &tab.contents {
                Contents::Remote { session, .. } => Some((tab.id, session.clone())),
                _ => None,
            }
        });
        let Some((tab_id, session)) = target_info else {
            return;
        };
        let task_key = remote_task_key(tab_id);
        if self.0.is_background_task_busy_untracked(&task_key) {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }
        let mut names = Vec::new();
        let mut resources = Vec::new();
        for row in rows {
            if let Row::Remote(entry) = row {
                names.push(entry.name);
                resources.push(Resource::Remote {
                    address: entry.address,
                    session: session.clone(),
                });
            }
        }
        if resources.is_empty() {
            return;
        }
        let count = resources.len();
        let (items, overflow_count) = preview_slice(names);
        let title = gds_i18n::count_untracked("delete-confirm-title", count);
        let message = i18n::text_untracked("remote-delete-warning");
        let primary = gds_i18n::count_untracked("delete-selected", count);
        let options = ConfirmOptions {
            icon: Some(IconName::Trash),
            primary_icon: Some(IconName::Trash),
            destructive: true,
            items,
            overflow_count,
        };
        let ctx = self.0.clone();
        self.0
            .confirm_with_options(title, message, primary, options, move || {
                let Some(cancel) = ctx.begin_background_task(
                    task_key.clone(),
                    gds_i18n::count_untracked("delete-working", count),
                ) else {
                    return;
                };
                let finish_key = task_key.clone();
                let progress_key = task_key.clone();
                let to_delete = resources.clone();
                let inhibit_scope = ctx.scope();
                let inhibit_reason = i18n::text_untracked("transfer-inhibit-reason");
                let (tx, mut rx) = mpsc::unbounded_channel();
                let (updates_tx, mut updates_rx) =
                    watch::channel(remote_transfer::Progress::default());
                drop(ctx.scope().spawn_named("Delete remote files", async move {
                    let _inhibit = gds::service::desktop::InhibitGuard::delayed(
                        &inhibit_scope,
                        gds::service::desktop::DEFAULT_INHIBIT_DELAY,
                        inhibit_reason,
                    );
                    let report = remote_transfer::delete(to_delete, cancel, updates_tx).await;
                    let _ = tx.send(report);
                }));
                let progress_weak = ctx.downgrade();
                let weak = ctx.downgrade();
                ctx.0.owner.with(|| {
                    drop(spawn_local(async move {
                        while updates_rx.changed().await.is_ok() {
                            let Some(ctx) = progress_weak.upgrade() else {
                                break;
                            };
                            let detail = updates_rx.borrow().current.clone();
                            ctx.set_background_task_detail(&progress_key, Some(detail));
                        }
                    }));
                    drop(spawn_local(async move {
                        let Some(report) = rx.recv().await else {
                            return;
                        };
                        let Some(ctx) = weak.upgrade() else {
                            return;
                        };
                        let _ = ctx.finish_background_task(&finish_key);
                        Navigator(&ctx).refresh_tab(tab_id);
                        if report.cancelled {
                            ctx.report_notice(i18n::text_untracked("remote-cancelled"));
                        } else if !report.failures.is_empty() {
                            let headline = gds::t_untracked!(
                                "delete-partial",
                                succeeded = report.completed.len() as i64,
                                failed = report.failures.len() as i64
                            );
                            let details: Vec<String> = report
                                .failures
                                .into_iter()
                                .map(|(item, err)| format!("{item}: {err}"))
                                .collect();
                            ctx.report_notice(format!("{headline}\n{}", details.join("\n")));
                        } else {
                            ctx.report_success(gds_i18n::count_untracked(
                                "delete-done",
                                report.completed.len(),
                            ));
                        }
                        ctx.focus_content();
                    }));
                });
            });
    }

    fn open_transfer_picker(
        &self,
        title: String,
        options: pick_files::Options,
        on_picked: impl Fn(Vec<PathBuf>) + 'static,
    ) {
        let (snapshot, home) = self
            .0
            .read_untracked(|state| ((*state.places).clone(), state.home.clone()));
        let mut places = Places::default();
        places.accept(snapshot);
        let launch = LaunchOptions {
            places: Ok(places),
            start: home.clone(),
            home,
            picker: Some(options),
        };
        let callback: Rc<dyn Fn(Vec<PathBuf>)> = Rc::new(on_picked);
        (self.0.0.open_picker_window)(title, launch, callback);
    }

    fn run_remote_copy(
        &self,
        task_key: String,
        origin_tab: Option<u64>,
        sources: Vec<Resource>,
        destination: Resource,
        record_history: bool,
        open_after: Option<PathBuf>,
    ) {
        let count = sources.len();
        let label = gds_i18n::count_untracked("transfer-working", count);
        let Some(cancel) = self.0.begin_background_task(task_key.clone(), label) else {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            return;
        };
        let local_dest = match &destination {
            Resource::Local(path) => Some(path.clone()),
            Resource::Remote { .. } => None,
        };
        let buffer_size = self
            .0
            .0
            .preferences
            .snapshot()
            .files
            .transfer
            .remote_buffer_bytes();
        let scope = self.0.scope();
        let spawn_scope = scope.clone();
        let inhibit_scope = scope.clone();
        let inhibit_reason = i18n::text_untracked("transfer-inhibit-reason");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (updates_tx, mut updates_rx) = watch::channel(remote_transfer::Progress::default());
        drop(spawn_scope.spawn_named("Remote file transfer", async move {
            let _inhibit = gds::service::desktop::InhibitGuard::delayed(
                &inhibit_scope,
                gds::service::desktop::DEFAULT_INHIBIT_DELAY,
                inhibit_reason,
            );
            let report = remote_transfer::copy(
                scope,
                sources,
                destination,
                remote_transfer::CopyOptions { buffer_size },
                cancel,
                updates_tx,
            )
            .await;
            let _ = tx.send(report);
        }));

        let progress_key = task_key.clone();
        let progress_weak = self.0.downgrade();
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                while updates_rx.changed().await.is_ok() {
                    let Some(ctx) = progress_weak.upgrade() else {
                        break;
                    };
                    let detail = {
                        let p = updates_rx.borrow();
                        format!(
                            "{} · {} · {}",
                            p.files,
                            EntryPresentation::format_size(p.bytes),
                            p.current
                        )
                    };
                    ctx.set_background_task_detail(&progress_key, Some(detail));
                }
            }));
            drop(spawn_local(async move {
                let Some(report) = rx.recv().await else {
                    return;
                };
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                let _ = ctx.finish_background_task(&task_key);
                if record_history && !report.undo.is_empty() {
                    ctx.update(|state| {
                        state
                            .history
                            .record(history::Entry::new(history::Kind::Copy, report.undo));
                    });
                }
                let navigator = Navigator(&ctx);
                if let Some(tab_id) = origin_tab {
                    navigator.refresh_tab(tab_id);
                } else {
                    navigator.refresh();
                }
                let lifecycle = Lifecycle(&ctx);
                if let Some(dest_dir) = local_dest {
                    lifecycle.refresh_affected(&[dest_dir]);
                }
                if report.cancelled {
                    ctx.report_notice(i18n::text_untracked("remote-cancelled"));
                } else if !report.failures.is_empty() {
                    let headline = gds::t_untracked!(
                        "transfer-partial",
                        succeeded = report.completed.len() as i64,
                        failed = report.failures.len() as i64
                    );
                    let details: Vec<String> = report
                        .failures
                        .into_iter()
                        .map(|(item, err)| format!("{item}: {err}"))
                        .collect();
                    ctx.report_notice(format!("{headline}\n{}", details.join("\n")));
                    ctx.notify_if_unfocused(
                        "remote-transfer",
                        i18n::text_untracked("transfer-notify-failed-title"),
                        headline,
                        true,
                    );
                } else if let Some(path) = open_after {
                    lifecycle.open_file(path, OpenMode::DefaultApplication);
                } else {
                    let message = gds_i18n::count_untracked("copy-done", report.completed.len());
                    ctx.report_success(message.clone());
                    ctx.notify_if_unfocused(
                        "remote-transfer",
                        i18n::text_untracked("transfer-notify-title"),
                        message,
                        false,
                    );
                }
                ctx.focus_content();
            }));
        });
    }
}
