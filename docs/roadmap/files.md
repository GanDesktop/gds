# GDS Files Roadmap

See [File Type Detection and Display](file-types.md) for localized MIME descriptions, category icons, and ambiguous filename matches.

## 1. Built-In Archive Browsing & Extraction

GDS Files currently delegates `.zip`, `.7z`, `.tar`, and `.tar.gz` files to external desktop archive managers. Planned built-in support splits into two phases:

- **Phase 1 (Read-only & extraction)**: Browse `.zip`, `.7z`, `.tar`, and `.tar.gz` archives inline (breadcrumbs, tabs, history, sorting, table/grid views), prompt for passwords on encrypted archives, extract all or selected entries without overwriting existing targets, and extract temporary copies on demand for external "Open" or clipboard/drag handoff.
- **Phase 2 (In-place mutation)**: Create archives from selections, drag files into archives to repack, and optionally write back edited temporary entries.

### Architectural Requirements

1. **Distinct location & entry identity** (`entities/location/model/address.rs`): Represent `(archive_file_path, internal_entry_path, entry_id)` explicitly so archive entries never masquerade as raw local `PathBuf`s and duplicate internal names remain unambiguous.
2. **Background streaming & bounded indexing**: Because `tar` streams sequentially and solid `7z` blocks require decoding preceding block entries, index and extract asynchronously under Task Manager scopes with cancellation and navigation-revision guards.
3. **Traversal-safe extraction**: Enforce GDS's non-overwriting target policy, preserve raw non-UTF-8 byte names where applicable, and reject symlink/path-traversal escapes outside the destination folder.

### Candidate Crate Evaluation

| Format | Rust Crate Candidate | Key Integration Considerations |
| --- | --- | --- |
| `.zip` | [`zip`](https://docs.rs/zip/latest/zip/read/struct.ZipArchive.html) | Central directory indexing, legacy filename encodings, password decryption, and non-overwriting extraction adapter |
| `.7z` | [`sevenz-rust2`](https://docs.rs/sevenz-rust2/latest/sevenz_rust2/) | Solid-block caching, encrypted payload & header decryption *(note: `libarchive` 3.8.11 still rejects encrypted 7z payloads/headers)* |
| `.tar` / `.tar.gz` | [`tar`](https://docs.rs/tar/latest/tar/struct.Archive.html) + [`flate2`](https://docs.rs/flate2/latest/flate2/) | Sequential scan indexing, hardlink/symlink safety, and bounded decompression cache |

## 2. SFTP & SMB Network Extensions

Building on the shipped `russh-sftp` and `smb` browse/copy foundation ([Network Locations](../apps/files/network.md)):

- Remote cut/move, drag-and-drop transfers, and paste conflict-rename dialogs.
- SMB server-root share enumeration (`smb://server/`) and broader Samba / Windows / NAS domain signing & encryption interoperability.
- Optional secret-service credential persistence, `~/.ssh/config` (`ProxyJump`) support, network service discovery, and remote media previews.

## 3. Local Workflow & Interoperability Milestones

- **Recursive directory search** and **multi-file batch renaming** (`F2` currently enforces single-item rename).
- [Cross-window file drag and drop on Wayland](wayland-file-drag-and-drop.md) covers native drag initiation, copy and move handling, and acceptance tests with Nautilus and Dolphin.
- **Cross-desktop interoperability verification**: Ongoing test matrix with Nautilus and Dolphin across Wayland, XWayland, and X11 for bidirectional copy/cut/paste, URI drag-and-drop with special/non-UTF-8 filenames, cross-manager Trash restoration, and `FileManager1` / `GDS Portal` modal parenting.
