# Network locations

GDS Files directly supports SFTP and SMB through native Rust clients. Existing network mounts remain accessible as local paths. No GVfs, KIO, rclone, SSHFS or Samba client library is required for direct connections.

## Connecting

Use “Add network location” on the network page, or enter a URI with Ctrl+L:

```text
sftp://user@host:2222/srv/data
smb://user@host/share/folder
```

Choose the protocol before entering its connection and authentication fields. SFTP supports SSH agent, password and private-key authentication. SMB offers registered-user and guest modes. New or changed SSH host keys require confirmation; accepted keys are stored in Files' own `gds/files/known_hosts` state file. Credentials remain in memory; saved URIs, favorites and recent items do not contain passwords.

Adding a location validates and saves it while leaving the network page open. Open/connect navigates in the current tab; Ctrl/Cmd+click opens a new tab. The page groups saved locations, connected servers and existing system mounts. Visited subdirectories belong to recent history rather than becoming server entries. Recent and Favorites show remote locations as ordinary rows alongside local and built-in locations; connection controls remain on the network page.

Loading shows the target address and a cancel action immediately. Failure or cancellation preserves the previous location, and stale results cannot replace later navigation.

## Browsing and transfers

Remote views support parent/back/forward navigation, refresh, name filtering, hidden items, favorites, new directories, renaming and permanent deletion. Copy/paste supports local-to-remote, remote-to-local and connected remote-to-remote transfers, including recursive directories.

Transfers write exclusive temporary entries, close them, then publish without overwriting. Cancellation or a publish conflict cleans up the task's temporary entries. Remote deletion does not use a trash service. Opening a remote file asks for a local download directory before handing the copy to the desktop application; edits are not uploaded automatically.

Remote identity remains a URI throughout navigation and persistence. It is never converted into a fictitious local path or used as a local terminal working directory.

## Limitations

- Use a complete SMB share URI such as `smb://server/share`. Server-root share enumeration can fail with the current client; the UI reports the failure and requests a share name.
- Remote cut/move, drag-and-drop and conflict-renaming dialogs are unavailable. Copy and rename fail on an occupied target; they do not overwrite or merge.
- Recursive copying excludes symlinks and special files. Deleting a symlink does not traverse its target.
- SFTP names must be UTF-8. Names containing U+FFFD are rejected because the client cannot distinguish that literal character from lossy decoding. SMB names use strict UTF-16 conversion. Local paths retain their raw bytes.
- SSH config, ProxyJump, keyboard-interactive authentication, persistent credential storage, automatic discovery, remote thumbnails and edit upload are not implemented.
- SMB domain authentication and server-specific signing/encryption policies require validation against the target server. Unsigned guest responses are accepted only for explicit guest access with an empty password.
- The CLI, FileManager1 and Portal picker use local locations. Download a remote file before selecting it for another application.

## Implementation

| Entry | Responsibility |
| --- | --- |
| [`service/remote`](../../../src/service/remote.rs) | GPUI-independent addresses, sessions, streams, SSH host verification and protocol clients |
| [`entities/location`](../../../bin/files/src/entities/location.rs) | Remote location identity, history, favorites and persisted view state |
| [`features/connect_server`](../../../bin/files/src/features/connect_server.rs) | Connection/authentication forms, host confirmation, session reuse and cancellation |
| [`features/file_operations/remote`](../../../bin/files/src/features/file_operations/remote.rs) | Recursive transfers, deletion, progress, cancellation and temporary-entry cleanup |
| [`widgets/remote_browser`](../../../bin/files/src/widgets/remote_browser.rs) | Remote tables and actions |
| [`pages/workspace/ui/remote`](../../../bin/files/src/pages/workspace/ui/remote.rs) | Tab, navigation and clipboard integration |

The manifest pins `russh 0.65.0`, `russh-sftp 3.0.1` and `smb 0.10.2`; Cargo.lock fixes their dependency graph. The SMB pin avoids incompatible RustCrypto prerelease requirements in newer versions. SMB is built with async I/O, signing and encryption. Protocol work runs on the caller's Tokio runtime.
