# Cross-window file drag and drop on Wayland

Use Wayland’s core `wl_data_device` protocol for file drags between GDS Files windows and between GDS Files and other desktop applications. The first milestone is copying local files in both directions with Nautilus and Dolphin. zgui owns the native drag session and its input events. GDS Files decides what each drop target does and runs the file operation.

## What works today

| Gesture | Behavior in the version pinned by GDS |
| --- | --- |
| Between targets in one Files window | A file row captures the pointer. The window’s drag coordinator identifies the folder or tab under it, opens it after a hover delay, and submits a move on release. Holding Ctrl selects copy. [Workspace drag handling](../../bin/files/src/pages/workspace/drag.rs) |
| From another application into Files | The native Wayland backend accepts `text/uri-list` when the source offers copy. Directory backgrounds, folder rows, and sidebar directories pass local paths to `PasteJob`. The terminal inserts paths into its input. [Wayland receiver](https://github.com/GanDesktop/zgui/blob/e90275a74764f643eb396ffd60eba0aafcebc461/crates/zgui-platform-wayland/src/driver/mod.rs#L249) · [Terminal drop target](../../bin/files/src/pages/workspace/terminals.rs) |
| From Files into another window | The pinned Wayland backend declares `drag_source = false`. The in-window drag preview therefore produces no native offer for a second GDS window or another application. [zgui capabilities at `e90275a7`](https://github.com/GanDesktop/zgui/blob/e90275a74764f643eb396ffd60eba0aafcebc461/crates/zgui-platform-wayland/src/capabilities.rs#L17) |

## What crosses the window boundary

1. **Start the drag.** After the pointer passes the drag threshold, the source creates a fresh `wl_data_source` and advertises its MIME types and supported actions. It calls `wl_data_device.start_drag` with the origin surface and the serial of the press that still holds the implicit grab. The compositor routes the drag to the window under the pointer. [Wayland data device](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_data_device)
2. **Choose a format and action.** The destination receives a `wl_data_offer` and surface-local coordinates. As the pointer enters or moves between targets, the destination can accept a MIME type and, with protocol version 3, advertise its supported and preferred actions. The compositor selects an action from the overlap. Modifier keys can change that choice, so the target follows the negotiated action. [Wayland data offer](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_data_offer)
3. **Read the file references.** The destination requests `text/uri-list` through a file descriptor. A two-file drag might carry `file:///home/atlas/My%20Photo.png` and `file:///home/atlas/notes.txt`, one URI per line. The destination file manager uses those references to copy or move the files. Releasing the pointer and completing the operation are separate protocol events. [Data transfer model](https://wayland.freedesktop.org/docs/book/Protocol.html) · [URI list format](https://www.iana.org/assignments/media-types/text/uri-list)

## Implementation sequence

### 1. Add a native drag source and prove local copy

Expose drag initiation through zgui’s platform API. The Wayland backend can use Smithay Client Toolkit’s `create_drag_and_drop_source` and `DragSource::start_drag` on the existing seat and connection. Keep the initiating press tied to its surface and seat until the threshold is crossed. Retain the source and its payload until completion or cancellation. Give each gesture its own data source so dragging leaves the clipboard selection intact. [Smithay drag source](https://docs.rs/smithay-client-toolkit/0.20.0/smithay_client_toolkit/data_device_manager/data_source/struct.DragSource.html)

At the existing file-row threshold, freeze the selected paths and reuse the URI encoder from the Files clipboard module. It already handles percent encoding, CRLF separators, and Unix filenames containing non-UTF-8 bytes. Advertise `text/uri-list` with `copy` for the first interoperability test. [File selection and drag threshold](../../bin/files/src/pages/workspace/drag.rs) · [URI encoder](../../bin/files/src/shared/lib/clipboard/protocol.rs)

### 2. Route native drops through existing targets

Let the hovered control decide whether to accept the offer. Directory targets submit one `PasteJob` through `inspect_paste`, preserving conflict prompts, undo history, and view refresh. The terminal accepts a copy action and inserts paths. Once a native session takes over a gesture, only the destination submits the operation. The source row’s pointer-release handler must not submit a second job. Before making native file drags the default, verify same-window move, Ctrl-copy, and hover-to-open for folders and tabs. Tab reordering can continue to use the window-local gesture. [Drop target helpers](../../src/ui/drag.rs) · [Files operation handling](../../bin/files/src/pages/workspace/operations.rs)

### 3. Tie move completion to the file operation

The current receiver calls `wl_data_offer.finish()` **before it delivers the drop to Files**. At that point a destination conflict dialog may still lie ahead. With a negotiated `move`, the protocol permits the source to delete transferred data after it receives `dnd_finished`. Allowing move with the current ordering could report success even when the user cancels the operation. [Pinned receiver implementation](https://github.com/GanDesktop/zgui/blob/e90275a74764f643eb396ffd60eba0aafcebc461/crates/zgui-platform-wayland/src/input/dnd.rs#L134) · [Wayland source completion](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_data_source)

Give the target a way to complete or abandon a dropped offer after its file job returns. Send `finish` only on full success. On cancellation or partial failure, release the offer without sending `finish` and keep the per-file outcomes for reporting and undo. For outgoing file URIs, let the source release its drag state on `dnd_finished`. Unconditionally unlinking the original pathname there risks a second deletion because a destination such as KDE’s KIO can already perform the move itself. Verify who performs the move in each direction before advertising it. [Files transfer implementation](../../bin/files/src/features/file_operations/api/transfer.rs) · [KDE drop job](https://github.com/KDE/kio/blob/master/src/widgets/dropjob.cpp)

### Sandbox interoperability

For a Flatpak peer that needs document access, add the `application/vnd.portal.filetransfer` MIME type. The source calls `StartTransfer` and `AddFiles`, then sends the transfer key in the Wayland offer. The recipient calls `RetrieveFiles` to obtain paths accessible in its sandbox. This can follow host-to-host local file support. [XDG FileTransfer portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileTransfer.html)

## Desktop acceptance

The following cases remain to be verified on native Wayland. Exercise two separate GDS windows, then test both directions with Nautilus and with Dolphin. Record the guest desktop and application versions alongside the results. [VM E2E coverage](../development/vm-e2e.md#coverage-and-migration)

| Case | Evidence of success |
| --- | --- |
| Copy a multi-selection, including spaces, a literal `%`, and a non-UTF-8 filename | The destination has the exact names and contents. The source and the pre-existing clipboard selection are intact. |
| Press Esc during a drag, or cancel a name conflict | The source remains intact. The destination contains no extra copy or overwritten file. |
| Move a local file, then exercise a multi-file move with a failure | A successful move creates one destination and removes the source once. On partial failure, untransferred files remain at their source and the offer receives no whole-batch success acknowledgement. |
| Drag inside one Files window | Folder and tab hover still work. Default move and Ctrl-copy each submit one operation. Dropping on the terminal inserts paths. |

Use QEMU for the press, cross-window motion, and release so the compositor issues a genuine grab serial. Use AT-SPI to identify file rows, targets, and conflict dialogs. The guest currently reports window-relative bounds, while QEMU consumes screen coordinates. Map each visible window to the guest screen before injecting the gesture. Check file names and contents independently over SSH. For the protocol acknowledgement, extend zgui’s existing two-client test to observe the source-side outcome. [Guest accessibility snapshot](../../tests/e2e/guest.py) · [VM input driver](../../tests/e2e/driver.py) · [zgui protocol test](https://github.com/GanDesktop/zgui/blob/e90275a74764f643eb396ffd60eba0aafcebc461/crates/zgui-platform-wayland/tests/file_drop.rs)
