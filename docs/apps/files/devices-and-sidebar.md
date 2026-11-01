# Volumes and sidebar

GDS Files uses UDisks2 on Linux to discover volumes and perform device operations. The volume view provides capacity, labels, default mount-point selection, browsing and explicit mount/unmount actions.

## Sidebar layout

The sidebar contains tabs, an optional volume group and location shortcuts. Locations stay at the bottom; tabs and volumes share the remaining height and scroll independently. The volume group is hidden by default and can be enabled in Files settings. Alt+V temporarily reveals and focuses it for the current session.

A draggable horizontal divider adjusts the tab/volume height allocation and supports keyboard adjustment. The saved height is retained when a small window temporarily constrains the actual size. Collapsing the sidebar preserves row heights, group state and scroll positions; only horizontal presentation changes. See [sidebar anatomy](../../vistual-language/sidebar.md).

Locations and volumes navigate within the current tab. Ctrl/Cmd+click opens another tab; browsing a volume does not create a separate kind of hidden tab. Back and forward use the same location history as ordinary folders and collections.

## Volume view

The management page uses the common data table, with one row per volume. It shows the name, filesystem, available/total capacity and default mount point. System volumes have a small marker beside the name. Labels take precedence over fallback names.

Single-click selects; arrow keys, Home and End move selection. Double-click or Enter opens the selected volume. Right-click and row menus act on their selected volume. Clicking table background clears selection and keyboard range state.

| Action | Availability |
| --- | --- |
| Open | Browses the selected default mount point when available |
| Mount / Ctrl+M | Explicit operation for an eligible unmounted volume |
| Unmount / Ctrl+Shift+M | Eligible mounted volume; running system volumes are protected |
| Rename / F2 | Sets the filesystem label through UDisks2; unavailable for locked/read-only volumes |
| Choose default mount point | Selects among the volume's existing mount points |

Clicking an unmounted volume displays its unmounted state; it does not mount automatically. Mount/unmount shortcuts apply only in the volume view or a volume directory. Duplicate operations are disabled while a request is running. An empty label clears the filesystem label; Files does not unmount a filesystem automatically to rename it.

Default mount-point choices use stable volume identity when available. Without a stable identity, a choice lasts for the attachment. The initial choice favors the shallowest path, with stable path ordering. A disappeared selected mount point stays unavailable until explicitly changed.

Capacity is sampled in the background from filesystem statistics, not a directory scan. Available space means space available to the current user. Periodic refresh, manual refresh and completed file operations update it; unloading a volume clears its capacity. Unknown, loading and failed capacity states are distinct from zero free space.

## Disconnects and authorization

An unplugged or unmounted volume leaves its tabs open with an unavailable state and hides the old file listing. Reconnection checks volume identity and the relative directory; reusing the same mount path does not identify the same device. A late operation result cannot navigate a tab that has since moved elsewhere.

UDisks2 and polkit own authorization. A graphical session needs a working polkit agent for interactive requests. Missing services or denied actions produce an error while ordinary local browsing remains available. Insertion updates the volume list without stealing focus or opening a window. Encrypted volumes must be unlocked with a system tool; Files has no password-unlock interface.

The backend also supports whole-device USB power-off and optical eject, but the current volume UI exposes single-volume mount/unmount and label operations. Whole-device removal and eject are not presented as completed UI features. The backend retains partial unmount results and reports busy or failed steps rather than claiming success.

## Implementation

Device discovery and operations belong to [`entities/location/api/volumes`](../../../bin/files/src/entities/location/api/volumes.rs), including the Linux backend, observation service, identity model and capacity sampling. The service subscribes to UDisks2 object/property changes and periodically reconciles state. Live device state is separate from saved locations and preferences.

The workspace composes sidebar groups, volume tables, navigation and task coordination. Shared UI provides navigation rows, capacity bars and split behavior. Device-specific policy stays in Files. Partitioning, formatting, filesystem repair, automatic mounting and other platform backends are not implemented.
