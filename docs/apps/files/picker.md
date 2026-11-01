# File picker

GDS Files provides file and directory selection in a separate process. Confirming emits one JSON line on stdout and exits; cancelling emits no selection. Diagnostics go to stderr.

```sh
gds-files --pick-file "$HOME" --filter 'Text:*.txt,*.md'
gds-files --pick-file --multiple "$HOME"
gds-files --pick-directory "$HOME"
gds-files --pick-directory --multiple "$HOME"
```

## CLI options and interaction

| Option | Meaning |
| --- | --- |
| `--pick-file` | Select files; directories remain navigable |
| `--pick-directory` | Select directories; with no selected directory, confirm the current directory |
| `--multiple` | Allow multiple selections in a picker mode |
| `DIRECTORY` | Initial local directory; defaults to Home |
| `--filter '[LABEL:]GLOB[,GLOB...]'` | Repeatable filename-filter groups; file mode only |
| `--read-only` | Initially check the read-only result flag; file mode only |
| `--` | Interpret subsequent input as the directory, including names beginning with `-` |

The picker replaces the terminal controls with filter, read-only, cancel and confirm controls. Name filtering and file-type filtering combine; directories remain browsable. Read-only is a result flag for the caller and does not change filesystem permissions.

| Shortcut | Action |
| --- | --- |
| Alt+A / Ctrl+Enter | Confirm an eligible selection |
| Alt+C / Esc | Cancel; Esc first dismisses local editing/menu state |
| Alt+R | Toggle read-only in file mode |
| Alt+T | Cycle file filters when more than one exists |
| Arrow keys / Shift+arrow keys | Move/extend selection |
| Ctrl+A | Select eligible items in multiple-selection mode |
| Enter / double-click | Enter a directory or confirm a file |
| Ctrl+L / Ctrl+F | Edit location / filter names |
| Alt+Up / Alt+Left / Alt+Right | Parent / back / forward |

Editing, menus, settings and modal dialogs retain their own keyboard scopes. Holding Ctrl or Alt reveals available hints for that modifier without moving the layout. The picker has no embedded terminal.

The window uses `io.github.GanDesktop.GdsFiles.Picker`, dialog window semantics and an initial size of 1100 × 740. See [desktop integration](nixos.md#picker-window-rules) for window rules.

## CLI result

A single selection returns an absolute path, read-only flag and zero-based filter index:

```json
{"path":"/absolute/path/report.txt","read_only":true,"filter_index":0}
```

One selected directory has the same shape. When more than one item is selected, the result is wrapped in `selections`:

```json
{"selections":[{"path":"/data/a.txt","read_only":false,"filter_index":0},{"path":"/data/b.txt","read_only":false,"filter_index":0}]}
```

Paths are not canonicalized to replace a selected symlink. A non-UTF-8 path has `path: null` and a recoverable native `OsString` representation. On Unix:

```json
{"path":null,"path_os":{"Unix":[47,116,109,112,47,255]},"read_only":false,"filter_index":0}
```

Exit status is `0` for confirmation, `1` for cancellation/window close/startup failure, and `2` for result-output or CLI argument failure. Use stdout only after success. `--help` and `--version` are ordinary CLI queries and do not emit selection results.

## Native Portal protocol

GDS Portal invokes the internal `--portal-request` mode with a stdin JSON `Request` from [`gds::service::desktop::protocol::chooser`](../../../src/service/desktop/protocol/chooser.rs). It supports Open, Save and SaveFiles, raw Unix paths, filename/MIME filters, choices and parent-window/modality information. This mode emits the protocol's structured `Result`, separately from the public CLI format above.

Save puts a filename input beside the confirm controls and asks before selecting an occupied target. SaveFiles selects a destination directory and generates unique names for conflicts. The Portal converts results to local file URIs and reaps cancelled picker processes.

Portal implementation: [`bin/portal/src`](../../../bin/portal/src). Its protocol tests can run independently:

```sh
cargo test --locked --bin xdg-desktop-portal-gds --no-default-features --features portal
```
