# Volumes & Sidebar

GDS Files uses `org.freedesktop.UDisks2` (`udisksd` on Linux, `bsdisks` on FreeBSD) to discover storage volumes, monitor capacity, and execute explicit mount, unmount, and filesystem-label operations.

## Sidebar Layout

The sidebar stacks **Tabs**, an optional **Volumes** group (hidden by default; enable in **Settings → Files** or reveal for the current session with `Alt+V`), and **Locations** pinned at the bottom.

- A draggable horizontal splitter (with keyboard stepping) divides vertical space between Tabs and Volumes, preserving the user's preferred height when small windows temporarily clamp available space.
- Collapsing the sidebar (`Ctrl+B`) narrows width while preserving row heights, group state, and scroll positions; see [Sidebar Anatomy](../../visual-language/sidebar.md).
- Clicking a volume or location navigates within the active tab (`Ctrl/Cmd+Click` opens a new tab), sharing the tab's unified back/forward history.

## Volume Management View (`location:/volumes`)

Displays one row per discovered volume using the shared `DataTable` and `CapacityBar` (showing user-available vs total space sampled asynchronously from `statvfs`, distinct from loading or unavailable states):

| Action | Shortcut / Trigger | Availability |
| --- | --- | --- |
| Open | `Enter` / Double-click | Navigates to the volume's active default mount point |
| Mount | `Ctrl+M` / Toolbar / Menu | Eligible unmounted volumes (clicking an unmounted volume never auto-mounts) |
| Unmount | `Ctrl+Shift+M` / Toolbar / Menu | Mounted non-system volumes |
| Rename label | `F2` / Menu | Updates filesystem label via UDisks2 (empty clears label; unavailable if locked/read-only) |
| Default mount point | Context menu | Persisted by stable volume identity when multiple mount points exist |

## Disconnects & Authorization

- **Identity-safe reconnects**: Unplugging or unmounting a volume marks open tabs on that device unavailable instead of exposing stale listings; reconnecting verifies volume identity (`location:/volume/...`) rather than trusting a reused mount path.
- **Polkit & UDisks2**: Interactive privilege prompts rely on the desktop session's polkit agent. Missing UDisks2 or denied authorization surfaces a notification without blocking ordinary local browsing. Encrypted volumes must be unlocked externally; whole-disk power-off and optical eject are not exposed in the current UI.

## Implementation

Device discovery, UDisks2 D-Bus subscriptions, identity resolution, and capacity sampling live in [`bin/files/src/entities/location/api/volumes.rs`](../../../bin/files/src/entities/location/api/volumes.rs), completely decoupled from saved preferences and SQLite location storage.
