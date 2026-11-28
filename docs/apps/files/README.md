# GDS Files

GDS Files is a native Wayland file manager for browsing local directories, removable volumes, tags, and SFTP/SMB servers, paired with a native desktop file chooser.

```sh
just dev files
just dev files "/path/to/folder"
just build files
```

For end-user installation and system integration, see the [Files User Guide](../../../bin/files/README.md), [NixOS Installation](../../nixos.md), [NixOS Desktop Integration](nixos.md), [Fedora RPMs](fedora.md), [Debian/Ubuntu DEBs](debian.md), and [Arch/AUR Packages](arch.md).

## Navigation & URI Model

The workspace combines a top breadcrumb/address bar, a collapsible left sidebar (vertical tabs, optional volumes, and fixed locations), a virtualized table/grid content view, an optional single-item details panel (`Ctrl+P`), and a window-owned bottom terminal dock (`Ctrl+\``).

Every browsable target has a canonical URI and participates in per-tab back/forward history (right-click back/forward arrows to jump directly to a history entry; `Ctrl/Cmd+Click` opens a new tab; pinned tabs restore their order on launch):

| URI Form | Target |
| --- | --- |
| `file:///…` (displayed as OS paths for local folders) | Local filesystem directories |
| `sftp://…`, `smb://…` | Native remote SFTP and SMB directories |
| `trash:///`, `recent:///`, `network:///` | Standard virtual locations (also accepts `trash:/`, `recent:/`, `recentlyused:/`, `network:/`, `remote:/`, and legacy `location:/…`) |
| `location:/favorites`, `location:/volumes` | GDS built-in virtual collections (also accepts `starred:///` and `computer:///`; `location://` is invalid) |
| `location:/volume/<stable\|attachment>/<id>?path=<rel>` | Removable/mounted volume paths preserving device identity across remounts |
| `tags:/`, `tags:/<tag>` | Baloo/KIO hierarchical file tag catalogs |

## Views, Inspection & Live Monitoring

- **Per-location view state**: Saved in `$XDG_STATE_HOME/gds/files/state.sqlite` (`view_settings`), recording list vs grid mode (`Ctrl+G`), sort column/direction (`Alt+Shift+S`), column visibility/order/widths (`Alt+Shift+V`), and details panel state independently per location alongside customizable application defaults.
- **Background content inspection (`service::file_inspection`)**: Deduplicates image, video, audio (including Lofty tags and embedded cover art), and text header probes (`<= 256 KiB` metadata prefix; `<= 32 MiB` image decode limit) across views, caching thumbnails in `$XDG_CACHE_HOME/thumbnails/x-large/`.
- **Non-disruptive directory watcher**: Coalesces filesystem notifications, ignores pure access events, re-inspects only affected entries on ordinary edits, guards results by navigation revision, and updates rows in place while preserving selection and scroll position.

## Local Operations, Trash & Desktop Handoff

- **Non-destructive conflict policy**: Copy, cut, paste, drag-and-drop, new folder (`Ctrl+Shift+N`), and `F2` rename never silently overwrite existing targets or merge directories; paste conflicts open a rename/cancel dialog suggesting an available name.
- **FreeDesktop Trash 1.0**: Supports home (`$XDG_DATA_HOME/Trash`) and per-volume (`.Trash-$uid` / `.Trash/$uid`) trash directories, preserving raw Unix path bytes, rejecting `..` traversal, and requiring destructive confirmation for `Shift+Delete` and Empty Trash.
- **Opening files & archives**: Regular files open via the XDG OpenURI / AppChooser portals; executable scripts and `.desktop` files require explicit chooser confirmation. Archive files (`.zip`, `.7z`, `.tar`, `.tar.gz`) open in external archive managers.
- **`org.freedesktop.FileManager1`**: Exports `ShowFolders`, `ShowItems`, and `ShowItemProperties` (`gds-files --dbus-service`), grouping revealed items into tabs and forwarding Wayland/X11 startup activation tokens without stealing an already owned bus name.
- **Wayland & X11 clipboard**: Uses `zgui`'s multi-MIME `Clipboards::{read_mime, write_mime}` (`text/uri-list`, `x-special/gnome-copied-files`, `application/x-kde-cutselection`, `application/x-gds-clipboard-id`, `text/plain;charset=utf-8`) with offer revision CAS (`ClipboardError::Changed`) so cut completion never clears newer clipboard contents, backed by an in-process `Snapshot` fallback when unfocused (`ClipboardError::NotFocused`).

## Subsystem References

- [FreeDesktop / XDG Compliance Review](freedesktop-compliance.md)
- [File Picker CLI & Portal Protocol](picker.md)
- [SFTP & SMB Network Locations](network.md)
- [Volumes & Sidebar](devices-and-sidebar.md)
- [Sidebar expansion frame times on GPUI-fast and zgui](sidebar-frame-times.md)
- [File Tags & URI Schemes](tags.md)
- [Desktop VM E2E & Unit Testing](testing.md)
- [Files Functional Testing on FreeBSD](freebsd-testing.md)
- [Redox OS Compatibility](redox-compatibility.md)
- [Files Roadmap (Built-in Archives, Remote Move & Search)](../../roadmap/files.md)
