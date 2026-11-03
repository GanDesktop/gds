# GDS Files

GDS Files is a native file browser for Linux with Wayland. It browses local directories, mounted volumes and SFTP/SMB locations, and provides a native file chooser for desktop applications.

```sh
just dev files
just dev files "/path/to/folder"
just build files
```

See [Fedora RPM installation](fedora.md), [NixOS installation](../../nixos.md)
and [desktop integration](nixos.md) for packaged use.

## Navigation and views

The window combines a top path bar, vertical file tabs, a content area, an optional details panel and a bottom terminal dock. The sidebar contains tabs, an optional volume group and fixed location entries. Its expanded and collapsed widths also apply to the terminal's left column.

Each tab has its own location history, selection, filter and scroll position. Home, recent items, favorites, network locations, trash and volumes all participate in that history. Sidebar entries open in the current tab; Ctrl/Cmd+click creates a tab. Closing the last tab creates a Home tab. Pinned locations reopen on the next launch; this does not restore shell processes.

The address bar supports editable local paths, supported remote URIs and the virtual addresses `location://recent`, `location://favorites`, `location://volumes`, `location://network` and `location://trash`. Click the address bar or press Ctrl+L to edit it, then press Enter to navigate. Home keeps its filesystem path. Navigation commits after loading succeeds; failure or cancellation preserves the previous location. Back/forward restores the visited view. Right-clicking a navigation arrow lists that direction's history.

Every browsable location has a URI: local paths use `file://`, remote directories use `sftp://` or `smb://`, and built-in pages use `location://`. Volume locations retain their device identity through `location://volume/<stable|attachment>/<id>?path=<relative-path>`. Home and available local folders still display filesystem paths in the address bar, which also accepts file URIs.

Recent and Favorites use one list for local, remote and built-in locations. Successful visits share one most-recent-first history; bookmarks are added explicitly. Rows share selection, keyboard navigation, opening in a tab, copying their URI and bookmark removal. Removing a favorite does not delete its target. Existing local and remote records migrate automatically; their old separate histories cannot reconstruct a shared visit chronology, so unified ordering starts with subsequent visits.

Local directories offer a virtualized table and grid. Name filtering, hidden-file visibility, sorting, column order/visibility/width and the details panel are available. View settings are saved per location; application defaults are separate. The details panel presents single-item metadata and supported image/video previews. Recursive search is not implemented.

## File operations

Single-click selects; double-click or Enter opens. Ctrl toggles selection, Shift extends it, and blank-area selection supports dragging. Menus distinguish a single item, multiple items and the current directory.

Local operations include copying, cutting, pasting, new folders, renaming, trash, permanent deletion and file drag-and-drop. A paste-name conflict opens a rename/cancel dialog with an available-name suggestion. Commits do not overwrite existing targets; directory merging is unavailable. Permanent deletion and emptying trash require confirmation. Restoring a trash item fails visibly if its original destination is occupied.

Regular files open through the desktop's default application; “Open with…” requests an application chooser. Executable content and desktop launchers require an explicit application choice, while recognized media with an executable permission bit can still open normally. Cancelling an application choice does not add a recent item.

File clipboard exchange on Wayland uses URI lists plus GNOME/KDE copy/cut representations. X11 currently supports file clipboard metadata only within the same GDS process. Copying a path as text requires lossless UTF-8; browsing and opening local files retain raw OS paths. Unix owner/group/permission metadata does not constitute a full access-control check.

Archive files use external applications through the usual open/open-with flow. Built-in archive browsing, compression, batch renaming and keyboard remapping are not implemented.

## Network, volumes and terminals

- [Network locations](network.md): native SFTP/SMB connections, saved locations and recursive transfers. Remote locations retain URI identity; remote deletion is permanent and moving remote files is unavailable.
- [Volumes](devices-and-sidebar.md): UDisks2 discovery, capacity, labels, mount points and explicit mount/unmount operations.
- [Embedded terminals](../terminal.md#files): window-owned local shells. Hiding the dock, switching tabs or opening settings preserves them; file navigation never changes an existing shell's directory.
- [File picker](picker.md): file/directory selection, multiple selection and the native Portal request protocol. Results are local paths.

## Settings and diagnostics

Open settings from the application menu or Ctrl+,. Changes apply immediately and save automatically; each field has its own default. Preferences apply only to Files. Settings are stored in `$XDG_CONFIG_HOME/gds/files/settings.toml`; location and view state are in `$XDG_STATE_HOME/gds/files/state.sqlite`. Managed symlinks and read-only settings are protected from replacement; save failures are reported separately from changes already applied in memory.

Pass `--enable-logs` to enable the in-memory log viewer, available from the menu or Ctrl+Shift+L. Logs stay within the process and are not written to disk. Ordinary error notifications remain available without this flag.

Developer architecture and commands are in [development](../../development.md). Visual and focus rules are defined by the shared [visual language](../../vistual-language/README.md) and [shortcuts](../../vistual-language/shortcuts.md).
