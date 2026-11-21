# Network Locations

GDS Files connects directly to SFTP and SMB servers using pure Rust protocol clients (`russh-sftp` and `smb`) running on the application's Tokio runtime—requiring no GVfs, KIO, SSHFS, rclone, or `libsmbclient` installation. Existing kernel/FUSE mounts remain browsable as ordinary local paths.

## Connecting (`location:/network`)

Add a server from the Network page or enter a URI directly via `Ctrl+L`:

```text
sftp://user@host:2222/srv/data
smb://user@host/share/folder
```

- **Authentication**: SFTP supports SSH agent, password, and private-key auth, storing confirmed host keys in `$XDG_STATE_HOME/gds/files/known_hosts`. SMB supports registered-user and explicit guest access. Passwords stay in process memory only and are never written into saved URIs, favorites, or recent history.
- **Navigation**: Adding a server validates and saves the entry on `location:/network` (grouped alongside active sessions and system network mounts). Opening navigates in the current tab (`Ctrl/Cmd+Click` opens a new tab) with immediate target display and cancellation.

## Browsing & Safe Transfers

Remote directories support back/forward/parent navigation, refresh, name filtering, hidden-item toggle, bookmarking, new folders, renaming, permanent deletion (no remote trash), and recursive copy/paste across local-to-remote, remote-to-local, and connected remote-to-remote endpoints.

Transfers stream into exclusive temporary entries, close them, and publish without overwriting existing targets—cleaning up temporary entries automatically upon cancellation or conflict. Opening a remote file downloads a copy to a user-chosen local folder before launching the desktop handler (local edits are not re-uploaded automatically).

## Current Boundaries

- **Specify an explicit SMB share** (`smb://server/share`); server-root share enumeration, remote cut/move, remote drag-and-drop, and conflict-rename dialogs are not yet implemented (see [Files Roadmap](../../roadmap/files.md)).
- **Path encodings & symlinks**: SFTP requires valid UTF-8 names (rejecting `U+FFFD`); SMB uses strict UTF-16 conversion; recursive copies skip symlinks and special device nodes.
- **Local-only pickers**: CLI, `FileManager1`, and GDS Portal pickers return local filesystem paths only.

## Implementation Map

| Module | Responsibility |
| --- | --- |
| [`src/service/remote.rs`](../../../src/service/remote.rs) | UI-independent SFTP/SMB clients, streams, and SSH `known_hosts` verification |
| [`bin/files/src/entities/location.rs`](../../../bin/files/src/entities/location.rs) | Remote URI identity, history, favorites, and SQLite view settings |
| [`bin/files/src/features/connect_server.rs`](../../../bin/files/src/features/connect_server.rs) | Step-by-step connection/auth dialogs, host-key prompts, and session reuse |
| [`bin/files/src/features/file_operations/remote.rs`](../../../bin/files/src/features/file_operations/remote.rs) | Recursive transfers, deletion, progress, and temporary-file cleanup |
| [`bin/files/src/widgets/remote_browser.rs`](../../../bin/files/src/widgets/remote_browser.rs) | Network page tables and server actions |

Dependency version constraints (`smb = 0.10.2`) are documented in [Upstream Patches & Dependency Pins](../../decisions/upstream-patches.md).
