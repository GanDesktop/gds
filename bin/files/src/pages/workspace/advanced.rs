//! Remote sessions, catalogue/device operations, and desktop requests.
use super::{
    model::{Contents, Operation, Row},
    name_form::{NameForm, NamePurpose},
    ui::Browser,
};
use crate::{
    entities::{
        file::directory,
        location::{
            Location,
            navigation::{LoadOutcome, LoadRequest},
            volumes,
        },
    },
    features::{
        connect_server::Intent,
        file_operations::{history, remote as operations},
        pick_files,
    },
    shared::lib::clipboard::{self, Clipboard},
};
use anti_webui::prelude::*;
use gds::{
    i18n,
    service::{
        desktop::{
            Desktop, OpenMode,
            file_manager::{Command, Request},
        },
        remote,
    },
};
use std::{path::PathBuf, rc::Rc};
use tokio::sync::watch;

impl Browser {
    pub(super) fn connect_server(&self) {
        let weak = self.downgrade();
        self.0.connector.prompt(None, Intent::Add, move |result| {
            let Some(browser) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(address) => {
                    browser.update(|state| state.places.set_remote_favorite(address, true));
                    browser.save_places();
                    browser.refresh_location(&Location::Network);
                }
                Err(error) => browser.report_notice(error),
            }
        });
    }

    pub(super) fn load_remote(&self, id: u64, request: LoadRequest, address: remote::Address) {
        if self.read(|state| state.picker.is_some()) {
            self.update(|state| {
                if let Some(tab) = state.tab_by_id(id) {
                    tab.navigation.cancel_request(&request);
                }
            });
            self.report_notice(i18n::text("remote-picker-local"));
            return;
        }
        let Some(session) = self.0.connector.session(&address) else {
            let weak = self.downgrade();
            self.0
                .connector
                .prompt(Some(address), Intent::Open, move |result| {
                    let Some(browser) = weak.upgrade() else {
                        return;
                    };
                    let current = browser.read(|state| {
                        state
                            .tabs
                            .iter()
                            .find(|tab| tab.id == id)
                            .is_some_and(|tab| tab.navigation.is_current_request(&request))
                    });
                    if !current {
                        return;
                    }
                    match result {
                        Ok(address) => browser.load_remote(
                            id,
                            request.clone().resolve_remote(address.clone()),
                            address,
                        ),
                        Err(error) => {
                            browser.update(|state| {
                                if let Some(tab) = state.tab_by_id(id) {
                                    tab.navigation.cancel_request(&request);
                                }
                            });
                            browser.report_notice(error);
                        }
                    }
                });
            return;
        };
        let tasks = self.scope().named_child("Read remote directory");
        let scope = (*tasks).clone();
        self.update(|state| {
            if let Some(tab) = state.tab_by_id(id) {
                tab.cancel();
                tab.work = Some(tasks);
            }
        });
        let worker = session.clone();
        let work = scope.spawn_cancellable_named("List remote directory", async move {
            worker.list(&address).await
        });
        self.work(
            "Deliver remote directory",
            async move {
                work.await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())
            },
            move |browser, result| {
                let applied = browser.update(|state| {
                    let Some(tab) = state.tab_by_id(id) else {
                        return Ok(false);
                    };
                    if !tab.navigation.is_current_request(&request) {
                        return Ok(false);
                    }
                    let listing = match result {
                        Ok(listing) => listing,
                        Err(error) => {
                            tab.navigation.cancel_request(&request);
                            return Err(error);
                        }
                    };
                    match tab
                        .navigation
                        .finish_load(&request, Ok(None))
                        .map_err(|error| error.to_string())?
                    {
                        LoadOutcome::Applied => {
                            tab.contents = Contents::Remote { listing, session }
                        }
                        LoadOutcome::Stale => return Ok(false),
                    }
                    if request.records_recent() {
                        state.places.record(request.location().clone());
                    }
                    Ok(true)
                });
                match applied {
                    Ok(true) => {
                        browser.reproject(id);
                        if browser.read(|state| state.tab().id == id) {
                            browser.sync_address();
                        }
                        browser.save_places();
                    }
                    Ok(false) => {}
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }

    pub(super) fn activate_remote(&self, entry: remote::Entry, new_tab: bool) {
        if entry.kind == remote::EntryKind::Directory {
            let location = Location::Remote(entry.address);
            if new_tab {
                self.new_tab(Some(location));
            } else {
                self.open(location);
            }
        } else {
            self.download_remote(vec![entry.address], true);
        }
    }

    pub(super) fn download_remote(&self, addresses: Vec<remote::Address>, open: bool) {
        if addresses.is_empty() {
            return;
        }
        let picker = pick_files::Options {
            directory: true,
            ..Default::default()
        };
        self.open_transfer_picker("remote-download", picker, move |browser, paths| {
            let Some(target) = paths.into_iter().next() else {
                return;
            };
            let sources = addresses
                .iter()
                .map(|address| browser.remote_resource(address))
                .collect::<Result<Vec<_>, _>>();
            match sources {
                Ok(sources) => {
                    let open = open.then(|| target.join(addresses[0].name()));
                    browser.copy_resources(sources, operations::Resource::Local(target), open);
                }
                Err(error) => browser.report_notice(error),
            }
        });
    }

    pub(super) fn upload_remote(&self) {
        let destination = self.read(|state| match &state.tab().contents {
            Contents::Remote { listing, .. } if !state.operation.busy() => {
                Some(listing.address.clone())
            }
            _ => None,
        });
        let Some(address) = destination else { return };
        let options = pick_files::Options {
            allow_mixed: true,
            multiple: true,
            ..Default::default()
        };
        self.open_transfer_picker(
            "remote-upload",
            options,
            move |browser, paths| match browser.remote_resource(&address) {
                Ok(destination) => browser.copy_resources(
                    paths.into_iter().map(operations::Resource::Local).collect(),
                    destination,
                    None,
                ),
                Err(error) => browser.report_notice(error),
            },
        );
    }

    fn open_transfer_picker(
        &self,
        title: &'static str,
        picker: pick_files::Options,
        picked: impl Fn(&Browser, Vec<PathBuf>) + 'static,
    ) {
        let weak = self.downgrade();
        let home = self.read(|state| state.home.clone());
        let volumes = self.0.volumes.clone();
        let new_window = self.0.new_window.clone();
        self.0.owner.with(|| {
            open_window(
                WindowOptions::new()
                    .title(i18n::text(title))
                    .size(900, 640)
                    .modal(true)
                    .decorations(DecorationMode::Client),
                move || {
                    let launch = super::ui::LaunchOptions {
                        places: Ok(crate::entities::location::places::Places::default()),
                        start: home.clone(),
                        home,
                        picker: Some(picker),
                    };
                    Browser::new_with_picker_result(
                        launch,
                        volumes,
                        new_window,
                        Rc::new(move |paths| {
                            let Some(browser) = weak.upgrade() else {
                                return;
                            };
                            browser.0.owner.with(|| picked(&browser, paths));
                        }),
                    )
                    .view()
                },
            )
        });
    }

    pub(super) fn download_selected_remote(&self) {
        let addresses = self.read(|state| {
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

    pub(super) fn disconnect_remote(&self) {
        if self.read(|state| state.operation.busy()) {
            self.report_notice(i18n::text("remote-task-running"));
            return;
        }
        let address = self.read(|state| match state.tab().navigation.location() {
            Location::Remote(address) => Some(address.clone()),
            _ => None,
        });
        let Some(address) = address else { return };
        let key = address.connection_key();
        let tabs=self.read(|state|state.tabs.iter().filter(|tab|matches!(tab.navigation.location(),Location::Remote(address) if address.connection_key()==key)).map(|tab|tab.id).collect::<Vec<_>>());
        self.update(|state| {
            for id in &tabs {
                if let Some(tab) = state.tab_by_id(*id) {
                    tab.cancel();
                    tab.navigation.cancel_pending();
                }
            }
        });
        let weak = self.downgrade();
        self.0.connector.disconnect(address, move |result| {
            let Some(browser) = weak.upgrade() else {
                return;
            };
            if let Err(error) = result {
                browser.report_notice(error);
            }
            let options = directory::ReadOptions {
                show_hidden: browser.0.preferences.snapshot().files.show_hidden,
            };
            for id in &tabs {
                browser.update(|state| {
                    if let Some(tab) = state.tab_by_id(*id) {
                        let request = tab
                            .navigation
                            .prepare_open(Location::Network, None, options);
                        // Network has no filesystem snapshot; the following refresh reloads its catalogue.
                        if let Err(error) = tab.navigation.finish_load(&request, Ok(None)) {
                            tracing::warn!(%error,"cannot return disconnected tab to Network");
                        }
                        tab.contents = Contents::LoadingCollection;
                        tab.rows.clear();
                    }
                });
                browser.refresh_tab(*id);
            }
            browser.sync_address();
            browser.refresh_location(&Location::Network);
        });
    }

    pub(super) fn rename_remote(&self, entry: remote::Entry) {
        let Ok(operations::Resource::Remote { session, .. }) = self.remote_resource(&entry.address)
        else {
            self.report_notice(i18n::text("remote-connect-source"));
            return;
        };
        let browser = self.clone();
        self.name_form(
            NameForm::new(
                NamePurpose::Rename,
                gds::t!("rename-title", name = entry.name.clone()),
                entry.name.clone().into(),
            ),
            move |name| {
                let Some(parent) = entry.address.parent() else {
                    return;
                };
                let target = match parent.child(name.to_str().expect("remote names are valid text"))
                {
                    Ok(target) => target,
                    Err(error) => {
                        browser.report_notice(error.to_string());
                        return;
                    }
                };
                let Some(cancel) = browser.begin_operation("rename") else {
                    return;
                };
                let location = Location::Remote(parent);
                let source = entry.address.clone();
                let session = session.clone();
                browser.work(
                    "Rename remote entry",
                    async move {
                        if cancel.is_cancelled() {
                            return Err("operation cancelled".into());
                        }
                        session
                            .rename(&source, &target)
                            .await
                            .map_err(|error| error.to_string())
                    },
                    move |browser, result| {
                        browser.end_operation();
                        match result {
                            Ok(()) => browser.refresh_location(&location),
                            Err(error) => browser.report_notice(error),
                        }
                    },
                );
            },
        );
    }

    fn remote_resource(&self, address: &remote::Address) -> Result<operations::Resource, String> {
        let session = self
            .0
            .connector
            .session(address)
            .ok_or_else(|| i18n::text("remote-connect-source"))?;
        Ok(operations::Resource::Remote {
            address: address.clone(),
            session,
        })
    }
    fn uri_resource(&self, uri: &str) -> Result<operations::Resource, String> {
        match Location::from_uri(uri).map_err(|error| error.to_string())? {
            Location::Directory(path) => Ok(operations::Resource::Local(path.into())),
            Location::Remote(address) => self.remote_resource(&address),
            _ => Err(format!("Unsupported file URI: {uri}")),
        }
    }
    pub(super) fn paste_uris(&self) {
        let destination = self.read(|state| match &state.tab().contents {
            Contents::Remote { listing, session } => Some(operations::Resource::Remote {
                address: listing.address.clone(),
                session: session.clone(),
            }),
            _ => state
                .tab()
                .navigation
                .path()
                .map(|path| operations::Resource::Local(path.to_owned())),
        });
        let Some(destination) = destination else {
            return;
        };
        let weak = self.downgrade();
        self.0.owner.with(|| {
            Clipboard::current().read_uris().then(move |result| {
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(Some((mode, uris))) => browser.paste_uri_payload(mode, uris, destination),
                    Ok(None) => browser.report_notice(i18n::text("clipboard-no-files")),
                    Err(error) => browser.report_notice(error.to_string()),
                }
            })
        });
    }
    pub(super) fn paste_uri_payload(
        &self,
        mode: clipboard::Mode,
        uris: Vec<String>,
        destination: operations::Resource,
    ) {
        if mode == clipboard::Mode::Cut {
            self.report_notice(i18n::text("remote-cut-unavailable"));
            return;
        }
        let sources = uris
            .iter()
            .map(|uri| self.uri_resource(uri))
            .collect::<Result<Vec<_>, _>>();
        match sources {
            Ok(sources) => self.copy_resources(sources, destination, None),
            Err(error) => self.report_notice(error),
        }
    }

    fn copy_resources(
        &self,
        sources: Vec<operations::Resource>,
        destination: operations::Resource,
        open: Option<PathBuf>,
    ) {
        if sources.is_empty() {
            return;
        }
        let Some(cancel) = self.begin_operation("copy-files") else {
            return;
        };
        let location = match &destination {
            operations::Resource::Local(path) => Location::from(path.as_path()),
            operations::Resource::Remote { address, .. } => Location::Remote(address.clone()),
        };
        let local = matches!(destination, operations::Resource::Local(_));
        let scope = self.scope();
        let options = operations::CopyOptions {
            buffer_size: self
                .0
                .preferences
                .snapshot()
                .files
                .transfer
                .remote_buffer_bytes(),
        };
        let progress = self.transfer_progress(cancel.clone());
        self.work(
            "Copy remote files",
            async move {
                Ok(operations::copy(scope, sources, destination, options, cancel, progress).await)
            },
            move |browser, result| {
                browser.end_operation();
                match result {
                    Ok(report) => {
                        let successful = report.failures.is_empty() && !report.cancelled;
                        if local {
                            browser.update(|state| {
                                state
                                    .history
                                    .record(history::Entry::new(history::Kind::Copy, report.undo))
                            });
                        }
                        browser.report_transfer(report.failures, report.cancelled);
                        browser.refresh_location(&location);
                        if successful && let Some(path) = open {
                            browser.work(
                                "Open downloaded file",
                                async move {
                                    Desktop::default()
                                        .open_file(&path, OpenMode::DefaultApplication)
                                        .await
                                        .map_err(|error| error.to_string())
                                },
                                |browser, result| {
                                    if let Err(error) = result {
                                        browser.report_notice(error)
                                    }
                                },
                            );
                        }
                    }
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }
    fn transfer_progress(
        &self,
        cancel: tokio_util::sync::CancellationToken,
    ) -> watch::Sender<operations::Progress> {
        let (sender, mut receiver) = watch::channel(operations::Progress::default());
        let weak = self.downgrade();
        let delivery = self.0.owner.with(|| {
            ui_channel(move |progress: operations::Progress| {
                if cancel.is_cancelled() {
                    return;
                }
                if let Some(browser) = weak.upgrade() {
                    browser.update(|state| {
                        if let Operation::Running { label, .. } = &mut state.operation {
                            *label = format!(
                                "{} · {} · {}",
                                progress.current,
                                progress.files,
                                crate::entities::file::ui::EntryPresentation::format_size(
                                    progress.bytes
                                )
                            );
                        }
                    });
                }
            })
        });
        self.scope()
            .spawn_named("Report remote transfer", async move {
                while receiver.changed().await.is_ok() {
                    delivery.send(receiver.borrow_and_update().clone());
                }
            });
        sender
    }
    fn report_transfer(&self, failures: Vec<(String, String)>, cancelled: bool) {
        let mut errors = failures
            .into_iter()
            .map(|(path, error)| format!("{path}: {error}"))
            .collect::<Vec<_>>();
        if cancelled {
            errors.push(i18n::text("remote-cancelled"));
        }
        if !errors.is_empty() {
            self.report_notice(errors.join("\n"));
        }
    }

    pub(super) fn delete_remote(&self, rows: Vec<Row>) {
        let locations = rows
            .iter()
            .filter_map(|row| match row {
                Row::Remote(entry) => entry.address.parent().map(Location::Remote),
                _ => None,
            })
            .collect::<Vec<_>>();
        let sources = rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Remote(entry) => Some(self.remote_resource(&entry.address)),
                _ => None,
            })
            .collect::<Result<Vec<_>, _>>();
        let sources = match sources {
            Ok(sources) if !sources.is_empty() => sources,
            Ok(_) => return,
            Err(error) => {
                self.report_notice(error);
                return;
            }
        };
        let browser = self.clone();
        self.confirm(
            i18n::text("delete-permanently"),
            i18n::text("remote-delete-warning"),
            move || {
                let Some(cancel) = browser.begin_operation("delete") else {
                    return;
                };
                let progress = browser.transfer_progress(cancel.clone());
                let sources = sources.clone();
                let locations = locations.clone();
                browser.work(
                    "Delete remote files",
                    async move { Ok(operations::delete(sources, cancel, progress).await) },
                    move |browser, result| {
                        browser.end_operation();
                        match result {
                            Ok(report) => {
                                browser.report_transfer(report.failures, report.cancelled);
                                for location in locations {
                                    browser.refresh_location(&location);
                                }
                            }
                            Err(error) => browser.report_notice(error),
                        }
                    },
                );
            },
        );
    }
    pub(super) fn delete_tags(&self, rows: Vec<Row>) {
        let tags = rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Tag(tag) => Some(tag),
                _ => None,
            })
            .collect::<Vec<_>>();
        if tags.is_empty() || !self.0.preferences.snapshot().tags.enabled {
            return;
        }
        let backend = self.read(|state| match &state.tab().contents {
            Contents::Tags { backend, .. } => Some(backend.clone()),
            _ => None,
        });
        let Some(backend) = backend else { return };
        let browser = self.clone();
        self.confirm(
            i18n::text("tags-delete-title"),
            format!("{}\n{}", i18n::text("tags-delete-message"), tags.join("\n")),
            move || {
                let Some(cancel) = browser.begin_operation("tags-delete") else {
                    return;
                };
                let backend = backend.clone();
                let tags = tags.clone();
                browser.work(
                    "Delete tag associations",
                    async move {
                        for tag in tags {
                            if cancel.is_cancelled() {
                                return Err("operation cancelled".into());
                            }
                            backend.delete_tag(&tag).await?;
                        }
                        Ok(())
                    },
                    |browser, result| {
                        browser.end_operation();
                        browser.refresh_tag_tabs();
                        if let Err(error) = result {
                            browser.report_notice(error);
                        }
                    },
                );
            },
        );
    }
    pub(super) fn refresh_tag_tabs(&self) {
        let tabs = self.read(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| matches!(tab.navigation.location(), Location::Tags(_)))
                .map(|tab| tab.id)
                .collect::<Vec<_>>()
        });
        for id in tabs {
            self.refresh_tab(id);
        }
    }
    pub(super) fn refresh_location(&self, location: &Location) {
        let tabs = self.read(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| tab.navigation.location() == location)
                .map(|tab| tab.id)
                .collect::<Vec<_>>()
        });
        for id in tabs {
            self.refresh_tab(id);
        }
    }

    pub(super) fn rename_volume(&self, volume: volumes::Volume) {
        if !volume.can_set_label() {
            return;
        }
        let browser = self.clone();
        self.prompt_name(i18n::text("volume-rename"), volume.label, move |label| {
            let action = volumes::DeviceAction::SetLabel {
                volume: volume.id.clone(),
                label,
            };
            let weak = browser.downgrade();
            browser
                .0
                .owner
                .with(|| match browser.0.volumes.operate(action) {
                    Ok(task) => task.then(move |result| {
                        if let Some(browser) = weak.upgrade() {
                            match result {
                                Ok(_) => browser.refresh_location(&Location::Volumes),
                                Err(error) => browser.report_notice(error.to_string()),
                            }
                        }
                    }),
                    Err(error) => browser.report_notice(error.to_string()),
                });
        });
    }

    pub fn apply_desktop_command(&self, command: Command) {
        if self.read(|state| state.picker.is_some()) {
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
        for target in targets {
            let existing = self.read(|state| {
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
            self.update(|state| state.tab_mut().navigation.view_mut().filter.clear());
            self.reveal_paths(target.selection);
            self.sync_address();
            self.refresh();
            if properties {
                self.0.details.set(true);
            }
        }
        self.focus_content();
        if !command.startup_id.is_empty() {
            activate_window(command.startup_id).then(|result| {
                if let Err(error) = result {
                    tracing::warn!(%error,"cannot activate Files with the supplied token");
                }
            });
        }
    }
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
