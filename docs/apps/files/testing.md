# Files testing

The primary E2E entry point is `just test-files --vm all`, using the shared
[desktop VM framework](../../development/vm-e2e.md). Run `just e2e-prepare all`
and `just e2e-build` first. The VM tests exercise normal `gds-files` binaries
through QEMU input and AccessKit/AT-SPI and preserve per-case UI and filesystem
evidence.

The [FreeBSD test report](freebsd-testing.md) summarizes the 13 passing KDE
Plasma Wayland workflows, the FreeBSD file-operation and `kqueue` backends they
exercise, and the remaining Linux-only integration boundaries.

For the shared library's Redox guest tests and the dependencies that block
Files on Orbital, see the [Redox compatibility report](redox-compatibility.md).

## Supplementary Weston runner

Run the isolated native workflow with `just test-files-native`. It builds the
opt-in `files-e2e` target, obtains Weston from the repository's locked Nixpkgs,
and runs `gds-files` inside a private headless Wayland session. A working Vulkan
or OpenGL driver is required; this is a real native window rather than
`zgui-platform-headless` alone.

The runner creates a new `HOME`, all XDG directories, a D-Bus session bus, and a
compositor for every run without connecting to the host system bus. Startup,
configuration, SQLite storage, filesystem operations, and `zgui` rendering use
their normal implementations, synchronized by application readiness signals and
rendered frames.

Artifacts remain in the printed `target/files-e2e/run-*` directory on both success
and failure:

- `session.log`, `exit-code.txt`, and `passed.txt` on scenario completion;
- compositor screenshots and accessibility dumps at each checkpoint;
- the isolated `HOME` containing the resulting files, configuration, and database;
- `manifest.json` containing the command, executable hash, revision, and replay
  command, plus `source.diff` and `source.tar.gz` capturing the working sources.

Run the manifest's replay command inside `just shell` to repeat the scenario with
fresh state. Screenshots are inspection artifacts, not golden images: tests do
not compare colors, layout coordinates, or fixed interface text.

Unit tests cover boundaries that the native workflow cannot observe or schedule
reliably:

| Area | Reason for a unit test |
| --- | --- |
| Cross-device source cleanup | Add, edit, or replace an inode exactly between the completed copy and cleanup using real temporary files. |
| Places persistence | Hold a real SQLite writer transaction while changing and dropping window-owned state; migrate directory pins and restore ordered abstract locations. |
| Clipboard intent & CAS | Validate multi-MIME encoding/decoding, stale refresh generation ordering, and unfocused fallback revision tracking. |
| Sorting | Preserve raw-name identity and stable tie order when visible labels and metadata are identical. |
| Tags | Verify command discovery and KIO output handling, URI/history/persistence, xattr changes, and tag preservation during transfer. |

Run those tests with `cargo test --locked --bin gds-files --features files` inside
the development shell. Each test owns its mutable state and temporary resources;
the Baloo command boundary uses executable fixtures for missing workers, command
failures, and argument handling. These do not replace the separate test against
installed Baloo/KIO binaries.

For tag storage alone, run `cargo test --lib --no-default-features --features file-tags service::file_tags`.
The opt-in `real_baloo_catalogue_and_local_targets` test requires an isolated Baloo database with a file tagged `工作`, `Project/design`, and `space tag`. Set `BALOO_DB_PATH`, `GDS_TEST_BALOOCTL`, `GDS_TEST_KIOCLIENT`, and `GDS_TEST_TAGGED_FILE`; on installations with nonstandard plugin paths also set `QT_PLUGIN_PATH`. Run it with:

```sh
cargo test --bin gds-files --features files real_baloo_catalogue_and_local_targets -- --ignored
```

Use an isolated index for the fixture. Dependencies and backend limitations are described in [Tags](tags.md).

Tag GUI behavior requires a real desktop with Baloo, KIO, and Dolphin:

- Check backend detection with indexing enabled and disabled, then browse Unicode and hierarchical tags in table and grid views.
- Confirm pure tag catalogues have no file metadata columns or details panel; real file results retain their metadata and details.
- Exercise Enter, Backspace, undo/redo, and cancel in the editor; verify the resulting xattrs and Dolphin's view agree.
- Add an external tag while the editor is open and confirm saving preserves it; change only xattrs and check that an open folder's details refresh.
- Copy tagged files, compare source and destination xattrs, and restart Files to check pinned abstract locations and their order.
- On disposable indexed files, delete a parent tag through the context menu or Delete key. Verify that its subtags are removed, similar names and unrelated tags remain, and file contents are unchanged. Cancelling the confirmation must leave all associations intact.

Physical-device operations, authenticated remote servers, external application
launch, cross-application clipboard interaction, and desktop Portal integration
require their real environments. The isolated workflow does not cover them. In
particular, Weston's headless seat supplies no physical input serial for
Wayland clipboard ownership, so headless runs exercise the in-process fallback
cache while cross-client clipboard offers are verified in real desktop/VM
sessions.
