# GDS Files Roadmap and Interoperability

This document tracks the current implementation scope and remaining roadmap items for GDS Files and GDS Portal.

## Trash Parsing and Restore

- [x] FreeDesktop Trash 1.0 `.trashinfo` parsing and restore semantics.

Current behavior:

- Duplicate `Path` and `DeletionDate` entries use the first occurrence; comments, blank lines, and unknown extension lines are ignored without panicking.
- Home trash resolves relative paths against `$XDG_DATA_HOME`; volume trash handles `.Trash-$uid` and `.Trash/$uid`, preserves raw path bytes, and rejects relative `..` segments prohibited by the specification.
- Restoring never overwrites existing targets, retains unmoved sources on partial failure, and detects record changes before cleanup.

Entry points: [`features/file_operations/recycle.rs`](../../../bin/files/src/features/file_operations/recycle.rs), [`features/file_operations/recycle/tests.rs`](../../../bin/files/src/features/file_operations/recycle/tests.rs). Specification: [Trash 1.0 §6](https://specifications.freedesktop.org/trash/1.0/).

## Native FileChooser and GDS Portal

- [x] Native `FileChooser` parameters, window semantics, and open/save flows via GDS Portal.

Current behavior:

- Requests carry `title`, `parent_window`, `modal`, and `accept_label` across the protocol, process boundary, and window presentation (`src/service/desktop/protocol`), preserving raw Unix path bytes over stdin JSON.
- Default `modal=true`, explicit `false`, empty parent handles, and Wayland (`xdg-foreign-v2`, `xdg-dialog`) / X11 (`WM_TRANSIENT_FOR`) parent window identifiers are supported; when a parent relationship cannot be established across sessions, the native picker remains usable and reports the limitation.
- `OpenFile`, `SaveFile`, and `SaveFiles` run natively with filename/MIME filters, active filter selection, caller `choices`, overwrite confirmation (`SaveFile`), and non-destructive unique name allocation (`SaveFiles`).
- Request `Close`, user cancellation, concurrent request isolation, and child-process reaping are handled by GDS Portal.

Entry points: [`bin/portal/src/chooser.rs`](../../../bin/portal/src/chooser.rs), [`bin/portal/src/protocol.rs`](../../../bin/portal/src/protocol.rs), [`bin/files/src/app/cli.rs`](../../../bin/files/src/app/cli.rs), [`bin/files/src/pages/workspace/ui/picker.rs`](../../../bin/files/src/pages/workspace/ui/picker.rs). Specifications: [FileChooser Portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.FileChooser.html), [Window Identifiers](https://flatpak.github.io/xdg-desktop-portal/docs/window-identifiers.html).

## `org.freedesktop.FileManager1` Service

- [x] Standard `org.freedesktop.FileManager1` interface and opt-in D-Bus activation.

Current behavior:

- Exports `ShowFolders`, `ShowItems`, and `ShowItemProperties` on the standard bus name and object path (`src/service/desktop/file_manager`).
- Accepts local `file://` URI arrays including root directories, percent-encoding, and non-UTF-8 local names; `ShowItems` reveals directories in their parent folder and selects them, while `ShowItemProperties` opens the native properties window.
- Groups selection requests by parent directory, reuses existing windows and tabs where applicable, and forwards caller `StartupId` activation tokens.
- `programs.gds-files.fileManager1.enable` installs D-Bus activation (`gds-files --dbus-service`) separately from the picker and reports existing bus owners without stealing the bus name.

Entry points: [`bin/files/src/app.rs`](../../../bin/files/src/app.rs), [`bin/files/src/app/desktop.rs`](../../../bin/files/src/app/desktop.rs), [`bin/files/src/app/cli.rs`](../../../bin/files/src/app/cli.rs), [`bin/files/src/pages/workspace/ui.rs`](../../../bin/files/src/pages/workspace/ui.rs), [`packaging/nix/apps/files/`](../../../packaging/nix/apps/files/). Specification: [File Manager D-Bus Interface](https://wiki.freedesktop.org/www/Specifications/file-manager-interface/).

## Desktop Interoperability

- [ ] Ongoing verification of GDS Files and GDS Portal interoperability with Nautilus and Dolphin across Wayland, XWayland, and X11 sessions:
  - Bidirectional file copy/cut/paste, partial-move clipboard cleanup, clipboard replacement, and URI drag-and-drop (including spaces, newlines, non-UTF-8 names, and multi-selection).
  - Cross-manager Trash record enumeration and restoration.
  - `FileManager1` cold activation and live window targeting, `xdg-desktop-portal` frontend routing to GDS Portal, and window transient/modal behavior on target compositors.

## Built-In Archive Support

- [ ] Built-in archive browsing, extraction, creation, and modification (unscheduled).

GDS Files currently delegates archive files (such as `.zip`, `.7z`, `.tar`, and `.tar.gz`) to external archive applications (targeting 7-Zip) through the desktop's default handler or “Open with…”. Candidate backends and design scope for future built-in support are documented in [Archive support](archive-support.md).

## SFTP and SMB Network Locations

- [x] Native Rust SFTP/SMB protocol backends, host-key verification, URI navigation, remote listings, recent history, and favorites.
- [x] Recursive upload/download, copy/paste, new folder, rename, permanent deletion, progress reporting, cancellation, and non-overwriting publication.
- [x] Step-by-step protocol connection forms, separated saved locations and visit history, and cancellable folder loading.
- [ ] Remote cut/move, conflict-renaming dialogs, and drag-and-drop.
- [ ] SMB root share enumeration compatibility across Samba, Windows, and NAS domain/signing/encryption policies.
- [ ] Optional remote CLI / `FileManager1` / GDS Portal handoff, SSH config integration, persistent credential storage, discovery, and remote previews.

Implementation details and current protocol limitations are documented in [Network locations](network.md).
