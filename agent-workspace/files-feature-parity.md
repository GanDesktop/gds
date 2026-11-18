# Files feature and interface comparison

Compare the working Files migration with the original `5fc77a8` implementation and the gpui-kit revision in its lockfile. Use the native application entry points and the existing Wayland fixture. Inspect ordinary, selected, empty, busy, error, compact and expanded states where the original distinguishes them. Compare interaction and motion as well as the settled frames. Record which behavior was observed and which still needs a real service or a compositor session.

| Work area | Interfaces and interactions |
| --- | --- |
| Browsing | Window chrome, address, sidebar, tabs, sidebar sizing, list, grid, columns, search, selection, marquee, drag, keyboard and motion |
| File workflows | Context menus, prompts, operations and history, clipboard, status popovers, places, trash, tags, volumes, remote connections, chooser and inspection |
| Common UI and secondary pages | Frame, navigation, focus, shared controls and motion, forms, notifications, Settings, Help, About, logs/tasks and the terminal |

Three coding lanes compare the original source with the current implementation. Reuse native primitives and put missing reusable components in `src/ui`. Coordinate public interfaces before changing consumers. The lead reviews changes and evidence, including original sources, native captures and the existing end-to-end scenario. Work on the current experimental branch. Keep runtime dependencies on Wayland and preserve the existing working tree.

## Reference observations

The GPUI reference is a detached build of `5fc77a8`, with gpui-kit pinned to `f61ebe1` by its Cargo.lock. The existing workflow passed on Weston. Its ten screenshots are under `target/files-e2e/gpui-5fc77a8-run-0zxky0nw/`. Additional captures of the same program are under `target/files-e2e/gpui-5fc77a8-pages-np6915yl/`. The GPUI client reserves 20 logical pixels around the window for its shadow. Compare equal content widths when checking the compact layout.

| Reference state | Observable behavior to preserve |
| --- | --- |
| Rename with invalid input | The editor shows the original item in the title. Typing a slash displays an inline error. Pressing Enter keeps the editor open for correction. The pre-repair AntiWebUI capture at `target/files-e2e/pre-parity-invalid-name-a8ml7edf/` reproduces the dismissal. |
| Network with no active connections | The title and selected place identify Network. The empty state has an icon and guidance, the overflow menu contains connection actions, and the footer terminal action is unavailable here. |
| Guarded performance settings | The warning, unlock control and parameter fields are visible together. Editing becomes available after unlocking. |
| Directory, list and grid | The reference workflow captures file rows, restored files, filesystem watcher updates, grid artwork, tab reordering, search, IME entry and mouse history. Exercise the actual action that changes each view. |
| Animation | The ordinary fixture runs with reduced motion. Use a separate run with motion enabled to observe intermediate frames and confirm the settled position. |

## Native comparisons

The [compact side-by-side captures](../target/files-e2e/parity-final-9q29qzuh/comparisons) pair the original Weston screenshot with the AntiWebUI GPU frame. Both clients have a 920 × 600 content area. The native frame uses the application’s transparent window background. Its premultiplied colors and native popups are composited over an empty Weston desktop of the same size. The original ran after more file operations and with additional tabs, which accounts for its extra tab buttons and transient notifications. Its file-browsing settings fixture had enabled auto-focus search. Both applications default that setting to off.

- Keyboard navigation through empty Favorites, Trash, Volumes, Network and Tags, then into Settings and Help, passed in `parity-scenes-y_s_7mzz`. An earlier capture showed Ctrl+L losing focus on empty Trash. The repeat used the keyboard for every navigation.
- The same run checked that an invalid rename keeps both the displayed explanation and the editable dialog after Enter. Compare `compare-invalid-submit` against the original at `gpui-5fc77a8-menus-etxzfj_o`.
- The classic workflow passed on the pinned Wayland fork in `target/files-e2e/run-ckqks3hw`. It covers file operations, watching, grid layout, tab dragging, IME search and mouse history. Its compositor pass started the normal application, captured a presented frame and checked clean shutdown.
- A second run with motion enabled sampled the sidebar tab region at 201, 193, 172, 152, 134, 117, 101, 85, 69, 55, 45 and 37 logical pixels. These numbers are the inner tab region, inset from the outer sidebar’s 222 and 58 pixel widths.

## Acceptance evidence

| Action exercised | Observed result |
| --- | --- |
| Switch from the populated file table to the grid at the same content width | The three file tile centers follow the original column pitch. Six simultaneous notifications occupy the original vertical extent. [Matched grid](../target/files-e2e/parity-grid-8_n6p6r4/comparisons/compare-grid.png). |
| Repeat address entry while collection data changes asynchronously | Thirty successive Ctrl+L navigations across five collections reached their intended destinations. [Native input log](../target/files-e2e/parity-nav-9hzd2hij/driven.log). |
| Hold Alt in the workspace and in the rename dialog | The workspace showed region shortcuts. The dialog showed its own confirm and cancel shortcuts. Invalid input survived Enter. [Workspace](../target/files-e2e/parity-final-9q29qzuh/driven/compare-alt-hints/frame.png) · [Dialog](../target/files-e2e/parity-final-9q29qzuh/driven/compare-modal-hints/frame.png). |
| Filter and select a palette, unlock performance settings and return to Files | Choosing Retro and Bamboo changed the live preference, locked sliders became editable, and Escape and F1 continued to work after the unlock button disappeared. [Performance](../target/files-e2e/parity-final-9q29qzuh/comparisons/compare-settings-performance.png) · [Help](../target/files-e2e/parity-final-9q29qzuh/comparisons/compare-help.png). |
| Open Set permissions from the file menu, dismiss it, resize and continue using Files | The dialog closed and the next action ran. The native menu hover timer previously accessed disposed state in this sequence. The fork now cancels it with the popup row. [Menu](../target/files-e2e/parity-final-9q29qzuh/comparisons/compare-file-menu.png) · [Permissions](../target/files-e2e/parity-final-9q29qzuh/driven/compare-permissions/frame.png). |
| Open the terminal in the selected directory, type a command and press Enter | The actual PTY wrote its current directory to a fixture file. Files observed and displayed the new file. About opened afterwards and navigated through License and Credits. [Terminal](../target/files-e2e/parity-final-9q29qzuh/driven/compare-terminal-dock/frame.png) · [About](../target/files-e2e/parity-final-9q29qzuh/driven/compare-about/frame.png). |
| Open and save at a constrained window size | Open returned the original non-UTF-8 name bytes, cancel emitted no output, overwrite confirmation returned the chosen path without modifying the file, and the upload picker accepted a file and a folder together. [Chooser results](../target/files-e2e/parity-chooser-ll2mWNGf/passed.txt). |
| Open and reuse the separate diagnostics window | Level and target filters, search, row details, task-tree navigation, close and reopen were exercised. Weston presented the independent diagnostic toplevel. [Diagnostic capture](../target/files-e2e/diagnostics-probe.ass78D/wayland/wayland-screenshot-2026-11-18_03-04-29.png). |
| Return from the notification stack to an editor | Escape and the close control returned keyboard focus to the same Files filter. When the source address editor was hidden, Files restored focus to the workspace. Six stacked notices measured 62 logical pixels from one row to the next. [Interaction results](../target/files-e2e/notice-focus.GrmUmf/passed.txt). |

Both repository Clippy passes completed with warnings denied, rustfmt passed, and the workspace suite passed 289 tests. One opt-in test requires a prepared Baloo/KIO index. The normal Files dependency tree uses the AntiWebUI Wayland backend.

The integration branch pins AntiWebUI at `9cde499047c552cf0f5fc8fa029f16427bd1e90e`. The fork adds Wayland activation between application windows and binds menu hover timers to the lifetime of their rows. The protocol test exercises activation across two Wayland connections.

The lane notes cover the other screen states and identify dependencies on external services. See [browsing](files-parity-browsing.md), [file workflows](files-parity-workflows.md) and [shared components](files-parity-components.md).
