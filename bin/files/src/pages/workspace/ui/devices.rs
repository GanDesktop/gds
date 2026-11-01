//! Device operations keep their initiating tab and never borrow the active tab
//! when an asynchronous mount completes.

mod interaction;
mod names;
pub(super) mod table;
#[cfg(test)]
mod tests;
mod view;

use super::*;
use crate::entities::location::volumes::{DeviceAction, Snapshot, VolumeId};
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Icon},
    prelude::*,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Availability {
    Available,
    Unmounted,
    Disconnected,
}

#[derive(Clone, Debug)]
pub(super) struct DeviceLocation {
    pub volume: VolumeId,
    pub root: PathBuf,
    pub relative: PathBuf,
    pub availability: Availability,
}

impl DeviceLocation {
    pub(super) fn capture(snapshot: &Snapshot, path: &Path) -> Option<Self> {
        let volume = snapshot.at_path(path)?;
        // System entries remain browsable when the device service is unavailable.
        if volume.system_mount {
            return None;
        }
        let root = volume
            .mount_points
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())?
            .clone();
        Some(Self {
            volume: volume.id.clone(),
            relative: path.strip_prefix(&root).ok()?.to_owned(),
            root,
            availability: Availability::Available,
        })
    }

    pub(super) fn update(&mut self, snapshot: &Snapshot) {
        if self.availability != Availability::Available {
            return;
        }
        self.availability = match snapshot.volume(&self.volume) {
            Some(volume) if volume.mount_points.contains(&self.root) => Availability::Available,
            Some(_) => Availability::Unmounted,
            None => Availability::Disconnected,
        };
    }

    pub(super) fn is_current(&self, snapshot: &Snapshot) -> bool {
        snapshot
            .volume(&self.volume)
            .is_some_and(|volume| volume.mount_points.contains(&self.root))
    }
}

impl OpenTab {
    pub(super) fn files_available(&self) -> bool {
        self.device
            .as_ref()
            .is_none_or(|location| location.availability == Availability::Available)
    }
}

#[derive(Clone)]
struct Reconnect {
    tab: u64,
    relative: PathBuf,
    location: Location,
}

impl Browser {
    pub(super) fn selected_volume(&self) -> Option<&VolumeId> {
        self.current_tab()?.navigation.view().volume.as_ref()
    }
    pub(super) fn set_selected_volume(&mut self, volume: Option<VolumeId>) {
        if let Some(slot) = self.current_tab_mut() {
            slot.navigation.view_mut().volume = volume;
        }
    }

    pub(super) fn on_storage_changed(
        &mut self,
        _: Entity<storage::Service>,
        cx: &mut Context<Self>,
    ) {
        let snapshot = &self.storage.read(cx).snapshot;
        if self
            .selected_volume()
            .is_some_and(|id| snapshot.volume(id).is_none_or(|volume| !volume.visible))
        {
            self.set_selected_volume(None);
        }
        for slot in &mut self.tabs {
            let was_available = slot.files_available();
            if let Some(location) = &mut slot.device {
                location.update(snapshot);
            } else if let Some(path) = slot.path() {
                slot.device = DeviceLocation::capture(snapshot, path);
            }
            if was_available && !slot.files_available() {
                slot.needs_refresh = false;
                slot.cursor = None;
                slot.anchor = None;
                {
                    let tab = &mut slot.navigation;
                    tab.cancel_pending();
                    tab.view_mut().selected.clear();
                }
                slot.refresh_visible();
            }
        }
        cx.notify();
    }

    pub(super) fn device_path_available(&self, path: &Path, cx: &App) -> bool {
        let service = self.storage.read(cx);
        if service.removal_in_progress(path) {
            return false;
        }
        if self
            .active()
            .and_then(|slot| slot.device.as_ref())
            .is_some_and(|location| {
                !location.root.as_os_str().is_empty()
                    && path.starts_with(&location.root)
                    && !location.is_current(&service.snapshot)
            })
        {
            return false;
        }
        if let Some(current) = service.snapshot.at_path(path)
            && self
                .active()
                .and_then(|slot| slot.device.as_ref())
                .is_some_and(|location| {
                    location.availability == Availability::Available
                        && location.volume == current.id
                        && path.starts_with(&location.root)
                })
        {
            return true;
        }
        self.tabs.iter().all(|slot| {
            slot.device.as_ref().is_none_or(|location| {
                location.availability == Availability::Available
                    || location.root.as_os_str().is_empty()
                    || !path.starts_with(&location.root)
            })
        })
    }

    pub(super) fn device_path_accepts_task(&self, path: &Path, cx: &App) -> bool {
        self.device_path_available(path, cx)
            && !self.pending_removals.iter().any(|action| {
                self.storage
                    .read(cx)
                    .snapshot
                    .removal_volumes(action)
                    .iter()
                    .any(|volume| {
                        volume
                            .mount_points
                            .iter()
                            .any(|root| path.starts_with(root))
                    })
            })
    }

    fn device_is_busy(&self, id: &VolumeId, cx: &App) -> bool {
        let service = self.storage.read(cx);
        service.is_busy(id)
            || self.pending_removals.iter().any(|action| {
                let scope = DeviceAction::PowerOff(action.volume().clone());
                service
                    .snapshot
                    .removal_volumes(&scope)
                    .iter()
                    .any(|volume| &volume.id == id)
            })
    }

    fn device_has_tasks(&self, action: &DeviceAction, cx: &App) -> bool {
        if self.file_operation_in_progress && self.file_operation_paths.is_empty() {
            return true;
        }
        self.storage
            .read(cx)
            .snapshot
            .removal_volumes(action)
            .iter()
            .any(|volume| {
                volume.mount_points.iter().any(|root| {
                    self.file_operation_paths
                        .iter()
                        .filter(|_| self.file_operation_in_progress)
                        .chain(self.trash.task_paths().iter())
                        .any(|path| path.starts_with(root))
                })
            })
    }

    fn click_volume(
        id: VolumeId,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |browser, _, window, cx| {
            if let Some(location) = browser.volume_location(&id, cx) {
                browser.activate_location_button(location, window, cx);
            }
        }
    }

    pub(in super::super) fn default_volume_root(
        &self,
        volume: &storage::Volume,
    ) -> Option<PathBuf> {
        let preferred = volume
            .id
            .stable
            .as_ref()
            .and_then(|identity| self.preferences.files.volume_mount_points.get(identity))
            .or_else(|| self.volume_defaults.get(&volume.id));
        Self::default_root(volume, preferred.map(PathBuf::as_path))
    }

    fn set_default_volume_root(&mut self, id: &VolumeId, root: PathBuf, cx: &mut Context<Self>) {
        if !self
            .storage
            .read(cx)
            .snapshot
            .volume(id)
            .is_some_and(|volume| volume.mount_points.contains(&root))
        {
            return;
        }
        if let Some(identity) = &id.stable {
            self.preferences
                .files
                .volume_mount_points
                .insert(identity.clone(), root);
            if let Err(error) = configuration::save(&self.preferences) {
                report_settings_failure("settings-save-failed", error, cx);
            }
        } else {
            self.volume_defaults.insert(id.clone(), root);
        }
        self.refresh_mounted_volume(id, cx);
        cx.notify();
    }

    pub(super) fn show_volume(
        &mut self,
        id: VolumeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(location) = self.volume_location(&id, cx) {
            self.show_location(location, window, cx);
        }
    }

    fn volume_location(&self, id: &VolumeId, cx: &App) -> Option<Location> {
        let volume = self.storage.read(cx).snapshot.volume(id)?;
        let identity = self.volume_identity(&volume.id, cx);
        Some(Location::Volume {
            identity,
            relative: Default::default(),
        })
    }

    fn default_root(volume: &storage::Volume, preferred: Option<&Path>) -> Option<PathBuf> {
        preferred.map(Path::to_owned).or_else(|| {
            volume
                .mount_points
                .iter()
                .min_by(|a, b| {
                    a.components()
                        .count()
                        .cmp(&b.components().count())
                        .then_with(|| a.cmp(b))
                })
                .cloned()
        })
    }

    fn mount_for_target(&mut self, id: VolumeId, target: Reconnect, cx: &mut Context<Self>) {
        let Some(volume) = self.storage.read(cx).snapshot.volume(&id).cloned() else {
            return;
        };
        if self.device_is_busy(&id, cx) {
            return;
        }
        if let Some(root) = self
            .default_volume_root(&volume)
            .filter(|root| volume.mount_points.contains(root))
        {
            self.finish_device_open(target, id, root, cx);
            return;
        }
        if volume.locked {
            notifications::info(i18n::text("device-unlock-unavailable", cx), cx);
            return;
        }
        if !volume.mountable {
            notifications::info(i18n::text("device-not-browsable", cx), cx);
            return;
        }
        let task = self.storage.update(cx, |service, cx| {
            service.operate(DeviceAction::Mount(id.clone()), cx)
        });
        let task = match task {
            Ok(task) => task,
            Err(error) => {
                self.report_device_error(error, cx);
                return;
            }
        };
        cx.spawn(async move |browser, cx| {
            let result = task.await;
            let _ = browser.update(cx, |browser, cx| {
                match result {
                    Ok(outcome) => {
                        if let Some(root) = outcome.mount_point {
                            browser.finish_device_open(target, id, root, cx);
                        }
                    }
                    Err(error) => browser.report_device_error(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn finish_device_open(
        &mut self,
        target: Reconnect,
        volume: VolumeId,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if !self
            .storage
            .read(cx)
            .snapshot
            .volume(&volume)
            .is_some_and(|volume| volume.mount_points.contains(&root))
        {
            self.report_device_error(storage::Error::Disconnected, cx);
            return;
        }
        let Reconnect {
            tab,
            relative,
            location,
        } = target;
        if self
            .tab_slot(tab)
            .is_some_and(|slot| slot.location() == location)
        {
            self.navigate_tab_inner(tab, root.join(relative), true, cx);
        }
        cx.notify();
    }

    fn click_reconnect(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.current_tab() else {
            return;
        };
        let Some(location) = slot.device.clone() else {
            return;
        };
        if location.availability == Availability::Disconnected && location.volume.stable.is_none() {
            notifications::info(i18n::text("device-reconnect-missing", cx), cx);
            return;
        }
        let target = Reconnect {
            tab: slot.id,
            relative: location.relative.clone(),
            location: slot.location(),
        };
        let volume = self
            .storage
            .read(cx)
            .snapshot
            .reconnect(&location.volume)
            .cloned();
        match volume {
            Some(volume) => self.mount_for_target(volume.id, target, cx),
            None => notifications::info(i18n::text("device-reconnect-missing", cx), cx),
        }
    }

    fn request_device_removal(
        &mut self,
        action: DeviceAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.device_is_busy(action.volume(), cx) {
            return;
        }
        let count = self
            .storage
            .read(cx)
            .snapshot
            .removal_volumes(&action)
            .len();
        let waiting = self.device_has_tasks(&action, cx);
        if !waiting && (matches!(action, DeviceAction::Unmount(_)) || count <= 1) {
            self.start_device_operation(action, cx);
            return;
        }
        let browser = cx.entity().downgrade();
        let title = i18n::text(action.label(), cx);
        let description = if waiting {
            i18n::text("device-removal-wait-message", cx)
        } else {
            i18n::count("device-removal-scope", count, cx)
        };
        let confirm = i18n::text(
            if waiting {
                "device-removal-wait"
            } else {
                action.label()
            },
            cx,
        );
        let cancel = i18n::text("operation-cancel", cx);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let browser = browser.clone();
            let action = action.clone();
            el! { dialog { confirm; title: ~title; description: ~description; icon: IconName::HardDrive.Icon::new;
                footer: $(ui::DialogActions::default().primary_label(confirm.clone()).primary_icon(IconName::Check).cancel_label(cancel.clone()));
                on_ok: $(move |_, _, cx| {
                    let _ = browser.update(cx, |browser, cx| {
                        if browser.device_has_tasks(&action, cx) { browser.pending_removals.push(action.clone()); cx.notify(); }
                        else { browser.start_device_operation(action.clone(), cx); }
                    });
                    true
                });
            } }
        });
    }

    pub(super) fn drain_device_removals(&mut self, cx: &mut Context<Self>) {
        for action in std::mem::take(&mut self.pending_removals) {
            if self.device_has_tasks(&action, cx) {
                self.pending_removals.push(action);
            } else {
                self.start_device_operation(action, cx);
            }
        }
    }

    fn start_device_operation(&mut self, action: DeviceAction, cx: &mut Context<Self>) {
        let label = action.success_label();
        let mounted_volume =
            matches!(action, DeviceAction::Mount(_)).then(|| action.volume().clone());
        let task = self
            .storage
            .update(cx, |service, cx| service.operate(action, cx));
        let task = match task {
            Ok(task) => task,
            Err(error) => {
                self.report_device_error(error, cx);
                return;
            }
        };
        cx.spawn(async move |browser, cx| {
            let result = task.await;
            let _ = browser.update(cx, |browser, cx| {
                match result {
                    Ok(_) => {
                        if let Some(id) = &mounted_volume {
                            browser.refresh_mounted_volume(id, cx);
                        }
                        notifications::success(i18n::text(label, cx), cx);
                    }
                    Err(error) => browser.report_device_error(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_mounted_volume(&mut self, id: &VolumeId, cx: &mut Context<Self>) {
        let ids: Vec<_> = self
            .tabs
            .iter()
            .filter(|slot| {
                slot.device
                    .as_ref()
                    .is_some_and(|device| &device.volume == id)
            })
            .map(|slot| slot.id)
            .collect();
        for id in ids {
            self.refresh_tab(id, cx);
        }
    }

    fn report_device_error(&self, error: storage::Error, cx: &mut Context<Self>) {
        notifications::Report::new(
            notifications::Level::Error,
            i18n::text(
                if matches!(error, storage::Error::PartialRemoval { .. }) {
                    "device-operation-incomplete"
                } else {
                    "device-operation-failed"
                },
                cx,
            ),
        )
        .detail(error.to_string())
        .show(cx);
    }
}

trait ActionPresentation {
    fn label(&self) -> &'static str;
    fn success_label(&self) -> &'static str;
}

impl ActionPresentation for DeviceAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Mount(_) => "device-mount",
            Self::SetLabel { .. } => "volume-rename",
            Self::Unmount(_) => "device-unmount",
            Self::PowerOff(_) => "device-power-off",
            Self::Eject(_) => "device-eject",
        }
    }
    fn success_label(&self) -> &'static str {
        match self {
            Self::Mount(_) => "device-mounted",
            Self::SetLabel { .. } => "volume-renamed",
            Self::Unmount(_) => "device-unmounted",
            Self::PowerOff(_) => "device-powered-off",
            Self::Eject(_) => "device-ejected",
        }
    }
}
