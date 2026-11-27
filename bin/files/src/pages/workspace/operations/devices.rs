//! Removable volume mount, unmount, eject, power-off, rename, and drain-queue workflows.

use std::{cell::RefCell, collections::HashSet, ffi::OsString, rc::Rc};

use gds::i18n as gds_i18n;
use gds::ui::IconName;
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, on_cleanup_local, spawn_local};

use super::super::{
    navigation::Navigator,
    projection::Rows,
    state::{context::Context, model::ConfirmOptions},
};
use crate::{
    entities::location::{
        Location,
        volumes::{self, DeviceAction},
    },
    shared::i18n,
};

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

thread_local! {
    static DEVICE_DRAIN_WINDOWS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

fn ensure_device_drain_observer(ctx: &Context) {
    let window_key = Rc::as_ptr(&ctx.0) as usize;
    let first_time = DEVICE_DRAIN_WINDOWS.with(|slot| slot.borrow_mut().insert(window_key));
    if !first_time {
        return;
    }
    let weak = ctx.downgrade();
    ctx.0.owner.with(|| {
        let fx = RenderEffect::new(move |_| {
            let Some(ctx) = weak.upgrade() else {
                return;
            };
            let _ = ctx.background_tasks_revision().get();
            let _ = ctx.0.revision.get();
            let has_pending = ctx.read_untracked(|state| !state.pending_device_removals.is_empty());
            if has_pending {
                let drain_weak = ctx.downgrade();
                drop(spawn_local(async move {
                    if let Some(ctx) = drain_weak.upgrade() {
                        DeviceOps(&ctx).drain_device_removals();
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

pub(super) fn pending_removals_include(
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

trait SnapshotExt {
    fn find(&self, id: &volumes::VolumeId) -> Option<&volumes::Volume>;
}

impl SnapshotExt for volumes::Snapshot {
    fn find(&self, id: &volumes::VolumeId) -> Option<&volumes::Volume> {
        self.volumes.iter().find(|volume| &volume.id == id)
    }
}

pub(in crate::pages::workspace) struct DeviceOps<'a>(pub &'a Context);

impl DeviceOps<'_> {
    pub(in crate::pages::workspace) fn device_is_busy(&self, id: &volumes::VolumeId) -> bool {
        let service_busy = self.0.0.volumes.is_busy(id);
        let snapshot = self.0.0.volumes.snapshot();
        service_busy
            || self.0.read(|state| {
                pending_removals_include(&snapshot, &state.pending_device_removals, id)
            })
    }

    pub(in crate::pages::workspace) fn device_is_busy_untracked(
        &self,
        id: &volumes::VolumeId,
    ) -> bool {
        let service_busy = self.0.0.volumes.is_busy_untracked(id);
        let snapshot = self.0.0.volumes.snapshot_untracked();
        service_busy
            || self.0.read_untracked(|state| {
                pending_removals_include(&snapshot, &state.pending_device_removals, id)
            })
    }

    fn device_has_tasks_untracked(&self, action: &DeviceAction) -> bool {
        action.removes()
            && (self.0.has_background_tasks_untracked()
                || self.0.read_untracked(|state| state.operation.busy()))
    }

    pub(in crate::pages::workspace) fn request_device_action(&self, action: DeviceAction) {
        if !action.removes() {
            self.start_device_operation(action);
            return;
        }
        if self.device_is_busy_untracked(action.volume()) {
            return;
        }
        let count = self
            .0
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
        let ctx = self.0.clone();
        self.0
            .confirm_with_options(title, message, primary, options, move || {
                let devices = DeviceOps(&ctx);
                if devices.device_has_tasks_untracked(&action) {
                    ensure_device_drain_observer(&ctx);
                    ctx.update(|state| {
                        if !state.pending_device_removals.contains(&action) {
                            state.pending_device_removals.push(action.clone());
                        }
                    });
                } else {
                    devices.start_device_operation(action.clone());
                }
            });
    }

    fn drain_device_removals(&self) {
        let queued = self
            .0
            .read_untracked(|state| state.pending_device_removals.clone());
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
        self.0.update(|state| {
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
        let task = match self.0.0.volumes.operate(action) {
            Ok(task) => task,
            Err(error) => {
                self.report_device_error(error);
                return;
            }
        };
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(()) => {
                        if let Some(id) = &mounted_volume {
                            DeviceOps(&ctx).refresh_mounted_volume_tabs(id);
                        }
                        ctx.report_success(i18n::text_untracked(success_key));
                    }
                    Err(error) => DeviceOps(&ctx).report_device_error(error),
                }
                Navigator(&ctx).refresh();
            }));
        });
    }

    fn refresh_mounted_volume_tabs(&self, id: &volumes::VolumeId) {
        let rows = Rows(self.0);
        let ids: Vec<u64> = self.0.read_untracked(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| match tab.navigation.location() {
                    Location::Volume { identity, .. } => rows
                        .volume_at_location_untracked(identity)
                        .is_some_and(|vol| &vol.id == id),
                    _ => false,
                })
                .map(|tab| tab.id)
                .collect()
        });
        let navigator = Navigator(self.0);
        for tab_id in ids {
            navigator.refresh_tab(tab_id);
        }
    }

    fn report_device_error(&self, error: volumes::Error) {
        let headline =
            i18n::text_untracked(if matches!(error, volumes::Error::PartialRemoval { .. }) {
                "device-operation-incomplete"
            } else {
                "device-operation-failed"
            });
        self.0.report_notice(format!("{headline}: {error}"));
    }

    pub(in crate::pages::workspace) fn rename_volume(&self, volume: volumes::Volume) {
        let ctx = self.0.clone();
        let volume_id = volume.id.clone();
        self.0.prompt_name(
            i18n::text_untracked("volume-rename"),
            volume.label,
            move |new_label| {
                DeviceOps(&ctx).request_device_action(DeviceAction::SetLabel {
                    volume: volume_id.clone(),
                    label: new_label,
                });
            },
        );
    }

    pub(in crate::pages::workspace) fn open_volume(&self, id: &volumes::VolumeId, new_tab: bool) {
        let Some(volume) = self.0.0.volumes.snapshot_untracked().find(id).cloned() else {
            self.0
                .report_notice(i18n::text_untracked("device-reconnect-missing"));
            return;
        };
        if !volume.mount_points.is_empty() {
            let location = Location::Volume {
                identity: Rows::volume_identity(&volume),
                relative: OsString::new(),
            };
            let navigator = Navigator(self.0);
            if new_tab {
                navigator.new_tab(Some(location));
            } else {
                navigator.open(location);
            }
            return;
        }
        if volume.locked {
            self.0
                .report_notice(i18n::text_untracked("device-unlock-unavailable"));
            return;
        }
        if !volume.mountable {
            self.0
                .report_notice(i18n::text_untracked("device-not-mounted"));
            return;
        }
        let Ok(future) = self
            .0
            .0
            .volumes
            .operate(DeviceAction::Mount(volume.id.clone()))
        else {
            self.0
                .report_notice(i18n::text_untracked("device-path-unavailable"));
            return;
        };
        let identity = Rows::volume_identity(&volume);
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = future.await;
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(_) => {
                        let location = Location::Volume {
                            identity,
                            relative: OsString::new(),
                        };
                        let navigator = Navigator(&ctx);
                        if new_tab {
                            navigator.new_tab(Some(location));
                        } else {
                            navigator.open(location);
                        }
                    }
                    Err(error) => ctx.report_notice(error.to_string()),
                }
            }));
        });
    }

    pub(in crate::pages::workspace) fn reconnect_volume(&self) {
        let location = self
            .0
            .read_untracked(|state| state.tab().navigation.location().clone());
        let Location::Volume { ref identity, .. } = location else {
            Navigator(self.0).refresh();
            return;
        };
        self.0.0.volumes.refresh();
        if let Some(volume) = Rows(self.0).volume_at_location_untracked(identity) {
            if !volume.mount_points.is_empty() {
                Navigator(self.0).refresh();
            } else {
                self.open_volume(&volume.id, false);
            }
        } else {
            self.0
                .report_notice(i18n::text_untracked("device-reconnect-missing"));
        }
    }
}
