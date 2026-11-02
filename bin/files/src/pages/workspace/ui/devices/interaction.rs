//! Volume selection and commands use the same workspace routes as file lists.

use super::super::selection::Motion;
use super::*;

impl Browser {
    pub(super) fn volume_can_open(&self, id: &VolumeId, cx: &App) -> bool {
        self.devices
            .service
            .read(cx)
            .snapshot
            .volume(id)
            .is_some_and(|volume| {
                !self.device_is_busy(id, cx)
                    && self
                        .default_volume_root(volume)
                        .is_some_and(|root| volume.mount_points.contains(&root))
            })
    }

    pub(super) fn select_volume(
        &mut self,
        id: VolumeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .devices
            .service
            .read(cx)
            .snapshot
            .volume(&id)
            .is_none_or(|volume| !volume.visible)
        {
            return;
        }
        self.set_selected_volume(Some(id));
        self.window.focus.focus(window, cx);
        cx.notify();
    }

    pub(in super::super) fn volume_action_target(&self, cx: &App) -> Option<VolumeId> {
        if self.settings.open {
            return None;
        }
        let id = if self.collection() == Some(Collection::Volumes) {
            self.selected_volume()
        } else {
            self.current_tab()
                .and_then(|slot| slot.device.as_ref())
                .map(|device| &device.volume)
        }?;
        self.devices
            .service
            .read(cx)
            .snapshot
            .volume(id)
            .filter(|volume| volume.visible)
            .map(|volume| volume.id.clone())
    }

    pub(super) fn volume_command_enabled(&self, id: &VolumeId, command: Command, cx: &App) -> bool {
        let Some(volume) = self.devices.service.read(cx).snapshot.volume(id) else {
            return false;
        };
        if !volume.visible || self.device_is_busy(id, cx) {
            return false;
        }
        match command {
            Command::MountVolume => {
                volume.mount_points.is_empty() && volume.mountable && !volume.locked
            }
            Command::UnmountVolume => volume.can_unmount(),
            _ => false,
        }
    }

    pub(super) fn execute_volume_command(
        &mut self,
        id: VolumeId,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Menus retain the identity they were opened for. Revalidate its current
        // state before acting, including a disconnect while the popup was open.
        if !self.volume_command_enabled(&id, command, cx) {
            return;
        }
        match command {
            Command::MountVolume => self.start_device_operation(DeviceAction::Mount(id), cx),
            Command::UnmountVolume => {
                self.request_device_removal(DeviceAction::Unmount(id), window, cx)
            }
            _ => {}
        }
    }

    pub(in super::super) fn run_volume_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.list_keys_available(window, cx)
            && let Some(id) = self.volume_action_target(cx)
        {
            self.execute_volume_command(id, command, window, cx);
        }
    }

    pub(in super::super) fn on_mount_volume(
        &mut self,
        _: &MountVolume,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::MountVolume, window, cx);
    }

    pub(in super::super) fn on_unmount_volume(
        &mut self,
        _: &UnmountVolume,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::UnmountVolume, window, cx);
    }

    pub(in super::super) fn move_volume_cursor(
        &mut self,
        motion: Motion,
        cx: &mut Context<Self>,
    ) -> bool {
        let volumes: Vec<_> = self
            .devices
            .service
            .read(cx)
            .snapshot
            .volumes
            .iter()
            .filter(|volume| volume.visible)
            .map(|volume| volume.id.clone())
            .collect();
        let current = self
            .selected_volume()
            .and_then(|id| volumes.iter().position(|volume| volume == id));
        let Some(index) = motion.target(current, volumes.len()) else {
            return false;
        };
        self.set_selected_volume(Some(volumes[index].clone()));
        self.devices
            .table
            .read(cx)
            .vertical_scroll_handle
            .scroll_to_item(index, ScrollStrategy::Nearest);
        cx.notify();
        true
    }

    pub(in super::super) fn on_volume_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.collection() != Some(Collection::Volumes) || !self.list_keys_available(window, cx) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        if let Some(motion) = Motion::for_key(&event.keystroke.key)
            && self.move_volume_cursor(motion, cx)
        {
            self.window.focus.focus(window, cx);
            cx.stop_propagation();
            window.prevent_default();
        }
    }
}
