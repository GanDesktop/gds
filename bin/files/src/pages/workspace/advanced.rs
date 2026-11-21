//! Remote sessions, catalogue/device operations, and desktop requests.

use std::{
    cell::RefCell,
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use gds::app::Gds;
use gds::i18n as gds_i18n;
use gds::service::{
    desktop::{
        OpenMode,
        file_manager::{Command, Request},
    },
    remote::{self, EntryKind},
};
use gds::ui::IconName;
use tokio::sync::{mpsc, watch};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, on_cleanup_local, spawn_local};
use zgui::view;

use super::{
    model::{ConfirmOptions, Contents, FILES_TASK_KEY, Row, remote_task_key},
    name_form::{NameForm, NamePurpose},
    navigation::{Loaded, sync_pinned_navigation},
    ui::{Browser, LaunchOptions},
};
use crate::{
    entities::{
        file::ui::EntryPresentation,
        location::{
            Location,
            navigation::{LoadOutcome, LoadRequest},
            places::Places,
            volumes::{self, DeviceAction},
        },
        tags,
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

const NAME_PREVIEW_LIMIT: usize = 5;

fn preview_slice(names: Vec<String>) -> (Vec<String>, usize) {
    let total = names.len();
    let overflow = total.saturating_sub(NAME_PREVIEW_LIMIT);
    let items = names.into_iter().take(NAME_PREVIEW_LIMIT).collect();
    (items, overflow)
}

fn device_action_label(action: &DeviceAction) -> &'static str {
    match action {
        DeviceAction::Mount(_) => "device-mount",
        DeviceAction::SetLabel { .. } => "volume-rename",
        DeviceAction::Unmount(_) => "device-unmount",
        DeviceAction::PowerOff(_) => "device-power-off",
        DeviceAction::Eject(_) => "device-eject",
    }
}

fn device_action_success_label(action: &DeviceAction) -> &'static str {
    match action {
        DeviceAction::Mount(_) => "device-mounted",
        DeviceAction::SetLabel { .. } => "volume-renamed",
        DeviceAction::Unmount(_) => "device-unmounted",
        DeviceAction::PowerOff(_) => "device-powered-off",
        DeviceAction::Eject(_) => "device-ejected",
    }
}

fn selection_has_dotfile(paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| {
        path.file_name()
            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
    })
}

fn server_affected_tab_ids(tabs: &[super::model::Tab], connection_key: &str) -> Vec<u64> {
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

thread_local! {
    static DEVICE_DRAIN_WINDOWS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

fn ensure_device_drain_observer(browser: &Browser) {
    let window_key = Rc::as_ptr(&browser.0) as usize;
    let first_time = DEVICE_DRAIN_WINDOWS.with(|slot| slot.borrow_mut().insert(window_key));
    if !first_time {
        return;
    }
    let weak = browser.downgrade();
    browser.0.owner.with(|| {
        let fx = RenderEffect::new(move |_| {
            let Some(browser) = weak.upgrade() else {
                return;
            };
            let _ = browser.background_tasks_revision().get();
            let _ = browser.0.revision.get();
            let has_pending =
                browser.read_untracked(|state| !state.pending_device_removals.is_empty());
            if has_pending {
                let drain_weak = browser.downgrade();
                drop(spawn_local(async move {
                    if let Some(browser) = drain_weak.upgrade() {
                        browser.drain_device_removals();
                    }
                }));
            }
        });
        on_cleanup_local(move || {
            drop(fx);
            DEVICE_DRAIN_WINDOWS.with(|slot| {
                slot.borrow_mut().remove(&window_key);
            });
        });
    });
}

impl Browser {
    pub(super) fn connect_server(&self) {
        self.0.menus.close();
        let browser = self.clone();
        self.0
            .connector
            .prompt(None, Intent::Open, move |result| match result {
                Ok(address) => browser.open(Location::Remote(address)),
                Err(error) if !error.is_empty() => browser.report_notice(error),
                Err(_) => {}
            });
    }

    pub(super) fn load_remote(&self, id: u64, request: LoadRequest, address: remote::Address) {
        if self.read_untracked(|state| state.picker.is_some()) {
            self.update(|state| {
                if let Some(tab) = state.tab_by_id(id) {
                    if tab.navigation.is_current_request(&request) {
                        tab.navigation.cancel_pending();
                    }
                    tab.work = None;
                }
            });
            self.report_notice(i18n::text_untracked("remote-picker-local"));
            self.sync_address();
            return;
        }

        let Some(session) = self.0.connector.session(&address) else {
            let prompt_browser = self.clone();
            let prompt_request = request.clone();
            self.0
                .connector
                .prompt(Some(address), Intent::Open, move |result| match result {
                    Ok(connected) => {
                        prompt_browser.load_remote(id, prompt_request.clone(), connected);
                    }
                    Err(error) if error.is_empty() => {
                        let unopened_remote = prompt_browser.update(|state| {
                            let Some(tab) = state.tab_by_id(id) else {
                                return false;
                            };
                            if tab.navigation.is_current_request(&prompt_request) {
                                tab.navigation.cancel_pending();
                            }
                            tab.work = None;
                            matches!(tab.navigation.location(), Location::Remote(_))
                                && !matches!(tab.contents, Contents::Remote { .. })
                        });
                        if unopened_remote {
                            prompt_browser.open_in_tab(id, Location::Network);
                        } else {
                            prompt_browser.sync_address();
                            prompt_browser.changed();
                        }
                    }
                    Err(error) => {
                        prompt_browser.finish_collection_load(
                            id,
                            &prompt_request,
                            Loaded::Unavailable(error),
                        );
                    }
                });
            return;
        };

        let work = self.0.tasks.named_child("Load remote folder");
        let scope = (*work).clone();
        {
            let mut state = self.0.state.borrow_mut();
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            if !matches!(tab.contents, Contents::Remote { .. }) {
                tab.contents = Contents::LoadingCollection;
                tab.rows.clear();
            }
            tab.work = Some(work);
        }
        self.changed();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let list_address = address.clone();
        let list_session = session.clone();
        drop(scope.spawn_named("List remote directory", async move {
            let result = list_session.list(&list_address).await;
            let _ = tx.send(result);
        }));

        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let Some(result) = rx.recv().await else {
                    return;
                };
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(listing) => {
                        let mut need_save = false;
                        let applied_loc = browser.update(|state| {
                            let tab = state.tab_by_id(id)?;
                            let prior_pin = tab.pinned.then(|| tab.navigation.location().clone());
                            match tab.navigation.finish_load(&request, Ok(None)) {
                                Ok(LoadOutcome::Applied) => {
                                    tab.work = None;
                                    tab.enrichment = None;
                                    tab.contents = Contents::Remote { listing, session };
                                    let loc = Location::Remote(address);
                                    if request.records_recent() {
                                        state.places.record(loc.clone());
                                        need_save = true;
                                    }
                                    if let Some(old_loc) = prior_pin
                                        && sync_pinned_navigation(state, &old_loc, &loc)
                                    {
                                        need_save = true;
                                    }
                                    Some(loc)
                                }
                                Ok(LoadOutcome::Stale) | Err(_) => None,
                            }
                        });
                        if let Some(loc) = applied_loc {
                            if need_save {
                                browser.save_places();
                            }
                            browser.apply_location_view_settings(id, &loc);
                            browser.reproject(id);
                            browser.restore_active_scroll();
                        }
                    }
                    Err(error) => {
                        browser.finish_collection_load(
                            id,
                            &request,
                            Loaded::Unavailable(error.to_string()),
                        );
                    }
                }
            }));
        });
    }

    pub(super) fn activate_remote(&self, entry: remote::Entry, new_tab: bool) {
        let in_picker = self.read_untracked(|state| state.picker.is_some());
        if in_picker {
            self.report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        match entry.kind {
            EntryKind::Directory => {
                let location = Location::Remote(entry.address);
                if new_tab {
                    self.new_tab(Some(location));
                } else {
                    self.open(location);
                }
            }
            EntryKind::File | EntryKind::SymbolicLink | EntryKind::Other => {
                self.download_remote(vec![entry.address], true);
            }
        }
    }

    pub(super) fn download_remote(&self, addresses: Vec<remote::Address>, open: bool) {
        if addresses.is_empty() {
            return;
        }
        let tab_id = self.read_untracked(|state| state.tab().id);
        let task_key = remote_task_key(tab_id);
        if self.is_background_task_busy_untracked(&task_key) {
            self.report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }
        let Some(first) = addresses.first() else {
            return;
        };
        let session = self
            .read_untracked(|state| match &state.tab().contents {
                Contents::Remote { session, .. } => Some(session.clone()),
                _ => None,
            })
            .or_else(|| self.0.connector.session(first));
        let Some(session) = session else {
            self.report_notice(i18n::text_untracked("remote-connect-source"));
            return;
        };

        if open && addresses.len() == 1 {
            let addr = addresses[0].clone();
            let preview_dir = remote_preview_dir();
            if let Err(error) = std::fs::create_dir_all(&preview_dir) {
                self.report_notice(error.to_string());
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

        let browser = self.clone();
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
                browser.run_remote_copy(
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

    pub(super) fn upload_remote(&self) {
        let target = self.read_untracked(|state| {
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
        if self.is_background_task_busy_untracked(&task_key) {
            self.report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }
        let browser = self.clone();
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
                browser.run_remote_copy(
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

    pub(super) fn download_selected_remote(&self) {
        let addresses: Vec<remote::Address> = self.read_untracked(|state| {
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
        let affected_tabs = self.read_untracked(|state| server_affected_tab_ids(&state.tabs, &key));
        if affected_tabs
            .iter()
            .any(|id| self.is_background_task_busy_untracked(&remote_task_key(*id)))
        {
            self.report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }

        let browser = self.clone();
        self.0.connector.disconnect(address, move |result| {
            if let Err(error) = result {
                browser.report_notice(error);
            }
            browser.changed();
        });

        for tab_id in &affected_tabs {
            self.open_in_tab(*tab_id, Location::Network);
        }
        if affected_tabs.is_empty() {
            self.refresh_location(&Location::Network);
        }
        self.changed();
    }

    pub(super) fn disconnect_remote(&self) {
        let address = self.read_untracked(|state| {
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
        let target_info = self.read_untracked(|state| {
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
        if self.is_background_task_busy_untracked(&task_key) {
            self.report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }
        let from = entry.address.clone();
        let current_name = entry.name.clone();
        let form = NameForm::new(
            NamePurpose::Rename,
            gds::t_untracked!("rename-title", name = current_name.clone()),
            OsString::from(&current_name),
        );
        let browser = self.clone();
        self.name_form(form, move |new_os_name| {
            let Some(new_name) = new_os_name.to_str() else {
                browser.report_notice(i18n::text_untracked("rename-not-text"));
                return;
            };
            if new_name == current_name {
                return;
            }
            let Ok(target) = renamed_remote_address(&from, new_name) else {
                browser.report_notice(i18n::text_untracked("rename-error-invalid"));
                return;
            };
            let Some(_cancel) = browser
                .begin_background_task(task_key.clone(), i18n::text_untracked("rename-working"))
            else {
                return;
            };
            let finish_key = task_key.clone();
            let session = session.clone();
            let from = from.clone();
            let display_name = new_name.to_owned();
            let (tx, mut rx) = mpsc::unbounded_channel();
            drop(
                browser
                    .scope()
                    .spawn_named("Rename remote item", async move {
                        let result = session.rename(&from, &target).await;
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
                    let _ = browser.finish_background_task(&finish_key);
                    match result {
                        Ok(()) => {
                            browser.refresh_tab(tab_id);
                            browser.report_success(gds::t_untracked!(
                                "rename-done",
                                name = display_name
                            ));
                        }
                        Err(error) => {
                            browser.report_notice(gds::t_untracked!(
                                "rename-failed",
                                error = error.to_string()
                            ));
                        }
                    }
                    browser.focus_content();
                }));
            });
        });
    }

    pub(super) fn paste_uri_payload(&self, uris: Vec<String>, destination: PathBuf, cut: bool) {
        if cut {
            self.report_notice(i18n::text_untracked("remote-cut-unavailable"));
            return;
        }
        let parsed = classify_uri_payload(&uris);
        if !parsed.local.is_empty() {
            let job = PasteJob::new(destination.clone(), transfer::Mode::Copy, parsed.local);
            self.inspect_paste(job, false, true, None);
        }
        if !parsed.remote.is_empty() {
            let mut resources = Vec::with_capacity(parsed.remote.len());
            for address in parsed.remote {
                let Some(session) = self.0.connector.session(&address) else {
                    self.report_notice(i18n::text_untracked("remote-connect-source"));
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
        let target_info = self.read_untracked(|state| {
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
        if self.is_background_task_busy_untracked(&task_key) {
            self.report_notice(i18n::text_untracked("remote-task-running"));
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
        let browser = self.clone();
        self.confirm_with_options(title, message, primary, options, move || {
            let Some(cancel) = browser.begin_background_task(
                task_key.clone(),
                gds_i18n::count_untracked("delete-working", count),
            ) else {
                return;
            };
            let finish_key = task_key.clone();
            let progress_key = task_key.clone();
            let to_delete = resources.clone();
            let (tx, mut rx) = mpsc::unbounded_channel();
            let (updates_tx, mut updates_rx) = watch::channel(remote_transfer::Progress::default());
            drop(
                browser
                    .scope()
                    .spawn_named("Delete remote files", async move {
                        let report = remote_transfer::delete(to_delete, cancel, updates_tx).await;
                        let _ = tx.send(report);
                    }),
            );
            let progress_weak = browser.downgrade();
            let weak = browser.downgrade();
            browser.0.owner.with(|| {
                drop(spawn_local(async move {
                    while updates_rx.changed().await.is_ok() {
                        let Some(browser) = progress_weak.upgrade() else {
                            break;
                        };
                        let detail = updates_rx.borrow().current.clone();
                        browser.set_background_task_detail(&progress_key, Some(detail));
                    }
                }));
                drop(spawn_local(async move {
                    let Some(report) = rx.recv().await else {
                        return;
                    };
                    let Some(browser) = weak.upgrade() else {
                        return;
                    };
                    let _ = browser.finish_background_task(&finish_key);
                    browser.refresh_tab(tab_id);
                    if report.cancelled {
                        browser.report_notice(i18n::text_untracked("remote-cancelled"));
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
                        browser.report_notice(format!("{headline}\n{}", details.join("\n")));
                    } else {
                        browser.report_success(gds_i18n::count_untracked(
                            "delete-done",
                            report.completed.len(),
                        ));
                    }
                    browser.focus_content();
                }));
            });
        });
    }

    pub(super) fn delete_tags(&self, rows: Vec<Row>) {
        if self.read_untracked(|state| state.operation.busy()) {
            return;
        }
        let tags_to_delete: Vec<String> = rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Tag(tag) if !tag.is_empty() => Some(tag),
                _ => None,
            })
            .collect();
        if tags_to_delete.is_empty() {
            return;
        }
        let backend = self.read_untracked(|state| match &state.tab().contents {
            Contents::Tags { backend, .. } => Some(backend.clone()),
            _ => None,
        });
        let commands = self.0.preferences.snapshot().tags.commands;
        let count = tags_to_delete.len();
        let (items, overflow_count) = preview_slice(tags_to_delete.clone());
        let title = i18n::text_untracked("tags-delete-title");
        let message = i18n::text_untracked("tags-delete-message");
        let primary = i18n::text_untracked("tags-delete");
        let options = ConfirmOptions {
            icon: Some(IconName::Trash),
            primary_icon: Some(IconName::Trash),
            destructive: true,
            items,
            overflow_count,
        };
        let browser = self.clone();
        self.confirm_with_options(title, message, primary, options, move || {
            let Some(_cancel) =
                browser.begin_operation(&gds_i18n::count_untracked("delete-working", count))
            else {
                return;
            };
            let tags_list = tags_to_delete.clone();
            let existing_backend = backend.clone();
            let cmds = commands.clone();
            let (tx, mut rx) = mpsc::unbounded_channel();
            drop(browser.scope().spawn_named("Delete file tags", async move {
                let backend = match existing_backend {
                    Some(backend) => Ok(backend),
                    None => tags::connect(&cmds).await,
                };
                let outcome = match backend {
                    Ok(backend) => {
                        let mut deleted = 0usize;
                        let mut failures = Vec::new();
                        for tag in tags_list {
                            match backend.delete_tag(&tag).await {
                                Ok(()) => deleted += 1,
                                Err(err) => failures.push(format!("{tag}: {err}")),
                            }
                        }
                        Ok((deleted, failures))
                    }
                    Err(err) => Err(err),
                };
                let _ = tx.send(outcome);
            }));
            let weak = browser.downgrade();
            browser.0.owner.with(|| {
                drop(spawn_local(async move {
                    let Some(outcome) = rx.recv().await else {
                        return;
                    };
                    let Some(browser) = weak.upgrade() else {
                        return;
                    };
                    browser.end_operation();
                    browser.refresh_tag_tabs();
                    match outcome {
                        Ok((deleted, failures)) => {
                            if !failures.is_empty() {
                                let headline = gds::t_untracked!(
                                    "delete-partial",
                                    succeeded = deleted as i64,
                                    failed = failures.len() as i64
                                );
                                browser
                                    .report_notice(format!("{headline}\n{}", failures.join("\n")));
                            } else if deleted == 1 {
                                browser.report_success(i18n::text_untracked("tags-deleted"));
                            } else {
                                browser.report_success(gds_i18n::count_untracked(
                                    "delete-done",
                                    deleted,
                                ));
                            }
                        }
                        Err(err) => browser.report_notice(err),
                    }
                    browser.focus_content();
                }));
            });
        });
    }

    pub(super) fn refresh_tag_tabs(&self) {
        let ids: Vec<u64> = self.read_untracked(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| matches!(tab.navigation.location(), Location::Tags(_)))
                .map(|tab| tab.id)
                .collect()
        });
        for id in ids {
            self.refresh_tab(id);
        }
    }

    pub(super) fn refresh_location(&self, location: &Location) {
        let ids: Vec<u64> = self.read_untracked(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| tab.navigation.location() == location)
                .map(|tab| tab.id)
                .collect()
        });
        for id in ids {
            self.refresh_tab(id);
        }
    }

    pub(super) fn device_is_busy(&self, id: &volumes::VolumeId) -> bool {
        let service_busy = self.0.volumes.is_busy(id);
        let snapshot = self.0.volumes.snapshot();
        service_busy
            || self.read(|state| {
                pending_removals_include(&snapshot, &state.pending_device_removals, id)
            })
    }

    pub(super) fn device_is_busy_untracked(&self, id: &volumes::VolumeId) -> bool {
        let service_busy = self.0.volumes.is_busy_untracked(id);
        let snapshot = self.0.volumes.snapshot_untracked();
        service_busy
            || self.read_untracked(|state| {
                pending_removals_include(&snapshot, &state.pending_device_removals, id)
            })
    }

    pub(super) fn device_has_tasks_untracked(&self, action: &DeviceAction) -> bool {
        action.removes()
            && (self.has_background_tasks_untracked()
                || self.read_untracked(|state| state.operation.busy()))
    }

    pub(super) fn request_device_action(&self, action: DeviceAction) {
        if !action.removes() {
            self.start_device_operation(action);
            return;
        }
        if self.device_is_busy_untracked(action.volume()) {
            return;
        }
        let count = self
            .0
            .volumes
            .snapshot_untracked()
            .removal_volumes(&action)
            .len();
        let waiting = self.device_has_tasks_untracked(&action);
        if !waiting && (matches!(action, DeviceAction::Unmount(_)) || count <= 1) {
            self.start_device_operation(action);
            return;
        }

        let title = i18n::text_untracked(device_action_label(&action));
        let message = if waiting {
            i18n::text_untracked("device-removal-wait-message")
        } else {
            gds_i18n::count_untracked("device-removal-scope", count)
        };
        let primary = i18n::text_untracked(if waiting {
            "device-removal-wait"
        } else {
            device_action_label(&action)
        });
        let options = ConfirmOptions {
            icon: Some(IconName::HardDrive),
            primary_icon: Some(IconName::Check),
            destructive: false,
            items: Vec::new(),
            overflow_count: 0,
        };
        let browser = self.clone();
        self.confirm_with_options(title, message, primary, options, move || {
            if browser.device_has_tasks_untracked(&action) {
                ensure_device_drain_observer(&browser);
                browser.update(|state| {
                    if !state.pending_device_removals.contains(&action) {
                        state.pending_device_removals.push(action.clone());
                    }
                });
            } else {
                browser.start_device_operation(action.clone());
            }
        });
    }

    pub(super) fn drain_device_removals(&self) {
        let queued = self.read_untracked(|state| state.pending_device_removals.clone());
        if queued.is_empty() {
            return;
        }
        let mut still_waiting = Vec::new();
        let mut ready = Vec::new();
        for action in queued {
            if self.device_has_tasks_untracked(&action) {
                still_waiting.push(action);
            } else {
                ready.push(action);
            }
        }
        self.update(|state| {
            state.pending_device_removals = still_waiting;
        });
        for action in ready {
            self.start_device_operation(action);
        }
    }

    fn start_device_operation(&self, action: DeviceAction) {
        let success_key = device_action_success_label(&action);
        let mounted_volume =
            matches!(action, DeviceAction::Mount(_)).then(|| action.volume().clone());
        let task = match self.0.volumes.operate(action) {
            Ok(task) => task,
            Err(error) => {
                self.report_device_error(error);
                return;
            }
        };
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(()) => {
                        if let Some(id) = &mounted_volume {
                            browser.refresh_mounted_volume_tabs(id);
                        }
                        browser.report_success(i18n::text_untracked(success_key));
                    }
                    Err(error) => browser.report_device_error(error),
                }
                browser.refresh();
            }));
        });
    }

    fn refresh_mounted_volume_tabs(&self, id: &volumes::VolumeId) {
        let ids: Vec<u64> = self.read_untracked(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| match tab.navigation.location() {
                    Location::Volume { identity, .. } => self
                        .volume_at_location_untracked(identity)
                        .is_some_and(|vol| &vol.id == id),
                    _ => false,
                })
                .map(|tab| tab.id)
                .collect()
        });
        for tab_id in ids {
            self.refresh_tab(tab_id);
        }
    }

    fn report_device_error(&self, error: volumes::Error) {
        let headline =
            i18n::text_untracked(if matches!(error, volumes::Error::PartialRemoval { .. }) {
                "device-operation-incomplete"
            } else {
                "device-operation-failed"
            });
        self.report_notice(format!("{headline}: {error}"));
    }

    pub(super) fn rename_volume(&self, volume: volumes::Volume) {
        let browser = self.clone();
        let volume_id = volume.id.clone();
        self.prompt_name(
            i18n::text_untracked("volume-rename"),
            volume.label,
            move |new_label| {
                browser.request_device_action(DeviceAction::SetLabel {
                    volume: volume_id.clone(),
                    label: new_label,
                });
            },
        );
    }

    fn open_transfer_picker(
        &self,
        title: String,
        options: pick_files::Options,
        on_picked: impl Fn(Vec<PathBuf>) + 'static,
    ) {
        let Some(windows) = try_use_windows() else {
            return;
        };
        let (snapshot, home) =
            self.read_untracked(|state| ((*state.places).clone(), state.home.clone()));
        let mut places = Places::default();
        places.accept(snapshot);
        let launch = LaunchOptions {
            places: Ok(places),
            start: home.clone(),
            home,
            picker: Some(options),
        };
        let launch_slot = RefCell::new(Some(launch));
        let callback: Rc<dyn Fn(Vec<PathBuf>)> = Rc::new(on_picked);
        let volumes = self.0.volumes.clone();
        let new_window = Rc::clone(&self.0.new_window);
        let window_opts = gds::ui::window::workspace_options(title, None);
        windows.open(window_opts, move || {
            let _ = Gds::current().appearance().install_window_sheet();
            let Some(launch) = launch_slot.borrow_mut().take() else {
                return AnyView::new(view! { box() });
            };
            let browser = Browser::new_with_picker_result(
                launch,
                volumes.clone(),
                Rc::clone(&new_window),
                Rc::clone(&callback),
            );
            AnyView::new(browser.view())
        });
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
        let Some(cancel) = self.begin_background_task(task_key.clone(), label) else {
            self.report_notice(i18n::text_untracked("remote-task-running"));
            return;
        };
        let local_dest = match &destination {
            Resource::Local(path) => Some(path.clone()),
            Resource::Remote { .. } => None,
        };
        let buffer_size = self
            .0
            .preferences
            .snapshot()
            .files
            .transfer
            .remote_buffer_bytes();
        let scope = self.scope();
        let spawn_scope = scope.clone();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (updates_tx, mut updates_rx) = watch::channel(remote_transfer::Progress::default());
        drop(spawn_scope.spawn_named("Remote file transfer", async move {
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
        let progress_weak = self.downgrade();
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                while updates_rx.changed().await.is_ok() {
                    let Some(browser) = progress_weak.upgrade() else {
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
                    browser.set_background_task_detail(&progress_key, Some(detail));
                }
            }));
            drop(spawn_local(async move {
                let Some(report) = rx.recv().await else {
                    return;
                };
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                let _ = browser.finish_background_task(&task_key);
                if record_history && !report.undo.is_empty() {
                    browser.update(|state| {
                        state
                            .history
                            .record(history::Entry::new(history::Kind::Copy, report.undo));
                    });
                }
                if let Some(tab_id) = origin_tab {
                    browser.refresh_tab(tab_id);
                } else {
                    browser.refresh();
                }
                if let Some(dest_dir) = local_dest {
                    browser.refresh_affected(&[dest_dir]);
                }
                if report.cancelled {
                    browser.report_notice(i18n::text_untracked("remote-cancelled"));
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
                    browser.report_notice(format!("{headline}\n{}", details.join("\n")));
                } else if let Some(path) = open_after {
                    browser.open_file(path, OpenMode::DefaultApplication);
                } else {
                    browser.report_success(gds_i18n::count_untracked(
                        "copy-done",
                        report.completed.len(),
                    ));
                }
                browser.focus_content();
            }));
        });
    }

    pub(crate) fn apply_desktop_command(&self, command: Command) {
        if self.read_untracked(|state| state.picker.is_some()) {
            return;
        }
        let properties = matches!(command.request, Request::ShowProperties { .. });
        let targets = match command.request {
            Request::NewWindow { directory } => Target::folders(vec![directory]),
            Request::ShowFolders { directories } => Target::folders(directories),
            Request::ShowItems { paths } | Request::ShowProperties { paths } => {
                Target::items(paths)
            }
        };
        if properties {
            for path in targets.iter().flat_map(|target| target.selection.iter()) {
                super::inspection::Properties::open(path.clone(), self.scope());
            }
        }
        self.show_files();
        let mut details_saved = false;
        for target in targets {
            let existing = self.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .position(|tab| tab.navigation.path() == Some(target.directory.as_path()))
            });
            if let Some(index) = existing {
                self.select_tab(index);
            } else {
                self.new_tab(Some(Location::from(target.directory.as_path())));
            }
            let reveal_dotfiles = selection_has_dotfile(&target.selection);
            self.update(|state| {
                let tab = state.tab_mut();
                tab.navigation.view_mut().filter.clear();
                tab.reveal_hidden = reveal_dotfiles;
                let loc = tab.navigation.location().clone();
                if properties {
                    state.places.set_details(loc, true);
                    details_saved = true;
                }
            });
            self.0.filter.set(String::new());
            self.reveal_paths(target.selection);
            self.sync_address();
            self.refresh();
            if properties {
                self.0.details.set(true);
            }
        }
        if details_saved {
            self.save_places();
        }
        self.focus_content();
        if !command.startup_id.is_empty()
            && let Some(window) = &self.0.window
        {
            gds::ui::window::activate_with_token(window, &command.startup_id);
        }
    }
}

fn pending_removals_include(
    snapshot: &volumes::Snapshot,
    pending: &[DeviceAction],
    id: &volumes::VolumeId,
) -> bool {
    pending.iter().any(|action| {
        let scope = DeviceAction::PowerOff(action.volume().clone());
        snapshot
            .removal_volumes(&scope)
            .iter()
            .any(|volume| &volume.id == id)
    })
}

struct UriPayload {
    local: Vec<PathBuf>,
    remote: Vec<remote::Address>,
}

fn classify_uri_payload(uris: &[String]) -> UriPayload {
    let mut local = Vec::new();
    let mut remote = Vec::new();
    for raw in uris {
        match Location::from_uri(raw) {
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

fn renamed_remote_address(
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

struct Target {
    directory: PathBuf,
    selection: Vec<PathBuf>,
}

#[cfg(test)]
mod tests;

impl Target {
    fn folders(directories: Vec<PathBuf>) -> Vec<Self> {
        let mut targets = Vec::<Self>::new();
        for directory in directories {
            if !targets.iter().any(|target| target.directory == directory) {
                targets.push(Self {
                    directory,
                    selection: Vec::new(),
                });
            }
        }
        targets
    }

    fn items(paths: Vec<PathBuf>) -> Vec<Self> {
        let mut targets = Vec::<Self>::new();
        for path in paths {
            let directory = path.parent().unwrap_or(&path).to_owned();
            if let Some(target) = targets
                .iter_mut()
                .find(|target| target.directory == directory)
            {
                if !target.selection.contains(&path) {
                    target.selection.push(path);
                }
            } else {
                targets.push(Self {
                    directory,
                    selection: vec![path],
                });
            }
        }
        targets
    }
}
