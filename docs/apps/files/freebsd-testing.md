# Files functional testing on FreeBSD

The native `x86_64-unknown-freebsd` build of `gds-files` passed **13 of 13 Files
workflows** on FreeBSD 15.1-RELEASE-p7 under KDE Plasma 6 Wayland (`KWin` with
Mesa software Vulkan and AT-SPI 2.60.4).

## Verified workflows

| Workflow | Verified FreeBSD behavior |
| --- | --- |
| F01 · Navigation and history | Directory entry, parent navigation (`Alt+Up`), address jumps, and per-tab back/forward history (`Alt+Left` / `Alt+Right`). |
| F02 · Filter and hidden files | Non-recursive live name filtering (`Ctrl+F`) and hidden-entry toggling (`Ctrl+H`). |
| F03/F05 · Grid view across restart | List/grid view switching (`Ctrl+G`) persisted in `$XDG_STATE_HOME/gds/files/state.sqlite` across process restart. |
| F04 · Independent tabs | Multi-tab creation (`Ctrl+N`), independent locations, tab switching (`Ctrl+Tab` / `Ctrl+Shift+Tab`), and tab close (`Ctrl+W`). |
| F06/F10 · Rename, undo, redo and create | Atomic `F2` rename, `Ctrl+Z` / `Ctrl+Shift+Z` history replay, `Ctrl+Shift+N` folder creation with `O_DIRECTORY \| O_NOFOLLOW` verification, and Escape cancellation. |
| F07 · Copy and move through the clipboard | Multi-MIME Wayland clipboard (`Ctrl+C` / `Ctrl+X` / `Ctrl+V`) staged recursive copy and move for files and nested directories. |
| F08 · Conflict cancellation | Paste conflict detection and Escape cancellation preserving both source and existing destination payloads. |
| F09 · Trash, restore and permanent deletion | Ordinary `Delete` to Freedesktop Trash, `Ctrl+Z` non-overwriting restore, `Shift+Delete` confirmation cancel, and confirmed permanent deletion. |
| F12 · Refresh after an external change | Live `kqueue` directory refresh when an external process creates `external download.txt` and unlinks `original.txt`. |
| F13 · Rename a file with special characters | Selection and atomic rename of `中文 %?#.txt` without affecting sibling entries. |
| F17 · CLI file picker | `gds-files --pick-file` selection, JSON stdout payload (`path`, `read_only`, `filter_index`), and zero exit code. |
| F19 · Write from the embedded terminal | `F4` PTY dock startup in the current directory (`fdescfs` mounted on `/dev/fd` with `rdlnk`), `pwd > terminal-directory.txt`, and live list refresh. |
| F24 · Copy into a locked directory, then retry | Real `Permission denied (os error 13)` staging failure on a mode-`0555` directory with source preservation, followed by a successful paste into a writable directory. |

## FreeBSD backend mechanisms

**Atomic no-replace rename.** `Request::rename_no_replace` and staged transfer
commits share `rustix::fs::renameat_with(..., RenameFlags::NOREPLACE)`. On
FreeBSD, [`GanDesktop/rustix` @ `gds-1.1.5`](https://github.com/GanDesktop/rustix/tree/gds-1.1.5)
([bytecodealliance/rustix#1751](https://github.com/bytecodealliance/rustix/pull/1751),
wired via `[patch.crates-io]`) binds `renameat2(2)` with `AT_RENAME_NOREPLACE`
through `weakcall!`, returning `ENOSYS` on pre-14.3/15.1 systems without link
failures. [Rename implementation](../../../bin/files/src/features/file_operations/api/rename.rs#L155-L169) ·
[Context menu gate](../../../bin/files/src/pages/workspace/menus.rs#L1581-L1588)

**Local transfers.** `Batch::execute` runs the staged copy/move executor on
Linux and FreeBSD: files and directories are copied via `openat` + `O_NOFOLLOW`
into a `.gds-transfer-*` sibling directory, checked for `st_dev`/`st_ino` and
timestamp stability, stamped with `futimens` / `utimensat` and widened `u16`
`mode_t` permissions, and committed atomically without replacing existing
destinations. [Transfer implementation](../../../bin/files/src/features/file_operations/api/transfer.rs#L229-L261) ·
[Undo directory check (`EMLINK` / `ELOOP`)](../../../bin/files/src/features/file_operations/history.rs#L150-L207)

**Trash.** `recycle::supported()` is enabled on Linux and FreeBSD, using
`trash` 5.3.0's `getmntinfo`-backed Freedesktop Trash 1.0 implementation for
deletion and `.trashinfo` enumeration while retaining GDS's frozen-identity
restore and purge checks. [Trash backend](../../../bin/files/src/features/file_operations/recycle.rs#L28-L31) ·
[Delete action](../../../bin/files/src/pages/workspace/operations.rs#L795-L796)

**Directory refresh.** Files subscribes to the open directory with
`RecursiveMode::NonRecursive`. [`GanDesktop/notify` @ `gds-8.2.0`](https://github.com/GanDesktop/notify/tree/gds-8.2.0)
backports the upstream [`notify-rs/notify#644`](https://github.com/notify-rs/notify/issues/644)
`kqueue` fix so `Vnode::Write` and `Vnode::Link` on a non-recursively watched
directory report `Modify` on the directory path itself instead of guessing an
untracked pre-existing child as `Create` or escalating into a recursive walk.
`Changes::apply_shared` rescans the directory on that path while reusing
unchanged content probes and holding only one `kqueue` descriptor for the
directory. [Watch registration](../../../bin/files/src/entities/file/api/directory/watching.rs#L48-L64) ·
[Snapshot reconciliation](../../../bin/files/src/entities/file/api/directory/watching/changes.rs#L90-L92) ·
[Upstream fork details](../../decisions/upstream-patches.md)

**Fontconfig & terminal monospace metrics.** `appearance::typography` enables
Fontconfig discovery on Linux and FreeBSD (watching `/usr/local/etc/fonts` and
`/usr/local/share/fonts` alongside `/etc/fonts`), resolving concrete default
sans-serif and `FC_SPACING`-verified monospace families (such as
`DejaVu Sans Mono`) so the embedded terminal cell grid never falls back to a
proportional face. [Typography resolution](../../../src/appearance/typography.rs#L188-L225) ·
[Fontconfig cache](../../../src/appearance/typography/fontconfig.rs#L278-L296)

**Volumes, capacity & network mount discovery.** `volumes` shares the
`org.freedesktop.UDisks2` D-Bus client (`udisks.rs`) across Linux (`udisksd`)
and FreeBSD (`bsdisks`), reading `bsdisks_ConnectionBus` /
`bsdisks_IsHotpluggable` and `devfs` device inodes on FreeBSD, sampling
mounted-filesystem capacity with `fstatvfs`, and enumerating mounted network
shares (`nfs`, `smbfs`, `fusefs.sshfs`, `fusefs.rclone`, and GVfs mounts) via
thread-safe `getfsstat(..., MNT_NOWAIT)`. [UDisks2 / bsdisks client](../../../bin/files/src/entities/location/api/volumes/udisks.rs#L89-L128) ·
[Capacity sampling](../../../bin/files/src/entities/location/api/volumes/usage.rs#L76-L87) ·
[Network mount discovery](../../../bin/files/src/entities/location/api/volumes/network.rs#L61-L134)

**Wayland desktop & XDG portals.** GDS aligns FreeBSD with the same Wayland /
Freedesktop D-Bus contracts as Linux: single-instance negotiation,
`org.freedesktop.FileManager1`, `org.freedesktop.portal.Settings` appearance and
reduced-motion subscriptions, `org.freedesktop.portal.OpenURI.OpenFile` (`ashpd`)
for default document launch and the "Open With" application chooser, and the
standalone `xdg-desktop-portal-gds` (`org.freedesktop.impl.portal.FileChooser`)
daemon. GDS does not maintain legacy `xdg-open` or bespoke `.desktop` chooser
workarounds when a FreeBSD desktop session lacks a running `xdg-desktop-portal`
backend (or upstream FreeBSD `F_KINFO` support for `OpenURI.OpenFile`).
[Document open & chooser](../../../src/service/desktop/client.rs#L125-L143) ·
[Settings portal](../../../src/appearance/portal.rs#L70-L150) ·
[Portal backend](../../../bin/portal/src/main.rs#L23-L49)

## Remaining Linux-only boundaries

Capabilities outside the 13 baseline workflows that remain gated to Linux:

- **Extended-attribute file tags**: `user.xdg.tags` xattr inspection/editing and
  Baloo/KIO catalogs (`file_tags::copy_from` is a no-op during FreeBSD local
  copies; see [file_tags.rs](../../../src/service/file_tags.rs#L238-L243)).

## Evidence and replay

The 13-of-13 verification run (`target/vm-e2e/run-20261122T043144-359317Z/` on
the test host) completed in 116.25 s against the `freebsd-dev` guest (`atlas`,
uid 1001) with `gds-files` SHA-256
`86b6edab3eb9b94bd982fccd65c6493b99774884ee6019c8256cb67f5ace081b`. Each case
directory records `steps.jsonl`, `final.ui.json`, and `final.files.json`.
[Workflow definitions](../../../tests/e2e/test_files.py)

With the native FreeBSD `gds-files` binary uploaded to the `atlas` user's
`~/Downloads` directory, run the full suite from the repository root:

```sh
just vm freebsd copy /path/to/freebsd/gds-files
just test-files --vm freebsd --guest-bin-dir /home/atlas/Downloads
```

Pass `-k <pattern>` (for example `-k 'F06_F10 or F12'`) to replay selected
workflows. [FreeBSD guest setup and test invocation](../../development/vm-e2e.md#freebsd-files)
