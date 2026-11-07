# Files testing

Run the native workflow from the repository root with `just test-files`. It builds
the opt-in `files-e2e` target, obtains Weston from the repository's locked Nixpkgs,
and runs Files inside a private headless Wayland session. A working Vulkan or
OpenGL driver is required; this is a real native window, not GPUI's test platform.

The runner creates a new HOME, all XDG directories, a D-Bus session bus, and a
compositor for every run. It does not connect to the host system bus. It drives
the normal application through pointer and keyboard events using GPUI Kit's
window interaction helpers. Startup, configuration, SQLite storage, filesystem
operations and rendering use their normal implementations.
Application and window-root notifications provide readiness signals; frame
callbacks synchronize input. The process deadline terminates stalled runs and
does not pace the scenario.

The workflow covers renaming with undo/redo, creating a folder, moving a file to
Trash and restoring it, observing an external file creation, switching to the
grid, and dragging a tab to reorder it.

Artifacts remain in the printed `target/files-e2e/run-*` directory on both success
and failure:

- `session.log`, `exit-code.txt` and, only on scenario completion, `passed.txt`;
- compositor screenshots and semantic element dumps at each checkpoint;
- the isolated HOME containing the resulting files, configuration and database;
- `manifest.json` containing the command, executable hash, revision and replay
  command, plus `source.diff` and `source.tar.gz` capturing the working sources.

Run the manifest's replay command inside `just shell` to repeat the scenario with
fresh state. Screenshots are inspection artifacts, not golden images: tests do
not compare colors, layout coordinates or fixed interface text. The archived
source files can be overlaid on the recorded Git revision to rebuild with
`cargo build --locked --bin gds-files --features files-e2e`.

The seven remaining unit tests cover boundaries that the native workflow cannot
observe or schedule reliably:

| Area | Reason for a unit test |
| --- | --- |
| Cross-device source cleanup | Add, edit or replace an inode exactly between the completed copy and cleanup; use real temporary files. |
| Places persistence | Hold a real SQLite writer transaction while changing and dropping window-owned state. |
| Clipboard intent | Complete an older asynchronous validation after a newer intent has started. |
| Sorting | Preserve raw-name identity and stable tie order when visible labels and metadata are identical. |

Run those tests with `cargo test --locked --bin gds-files --features files` inside
the development shell. Each test owns its mutable state and temporary resources;
none uses a mock, fixed sleep, conditional scenario or loop.

Removed tests asserted defaults, rendering details, simple accessors, duplicate
state transitions, third-party behavior, or drove incomplete application fixtures.
The fake local UDisks service and the preference-saving bypass were removed too.
Physical-device operations, authenticated remote servers, external application
launch, cross-application clipboard interaction and desktop Portal integration
still require their real environments. The isolated workflow does not claim to
cover them. In particular, Weston's headless seat supplies no physical key-press
serial for clipboard ownership; the runner does not replace the clipboard backend.
