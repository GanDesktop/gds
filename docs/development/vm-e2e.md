# Desktop VM workflows

Files and Images use **pytest + libvirt/QEMU + AT-SPI** for desktop E2E testing.
The same workflows run in the existing Fedora, Ubuntu, CachyOS and Noctalia
guests. Each run records the actual OS, desktop and binary hashes; a guest alias
does not prove that a particular desktop was exercised.

## Run

The host needs libvirt/`virsh`, SSH, Nix and [uv](https://docs.astral.sh/uv/).
Use the guests described in [Virtual machines](virtual-machines.md), with their
desktop user, SSH key and passwordless sudo for environment preparation.

```sh
just e2e-prepare all                  # Once per clean guest baseline
just e2e-build                        # Normal Files + Images builds, no test feature
just test-e2e                         # All workflows on all four guests
just test-e2e --vm ubuntu,noctalia     # Selected environments
just test-files --vm ubuntu -k F07    # Replay one independent workflow
just test-images --vm cachyos
just test-e2e --collect-only          # Inspect the matrix without touching a VM
```

`--guest-bin-dir /usr/bin` selects installed binaries instead of the development
bundle. The Linux installed mode expects both applications. The FreeBSD profile
uses the native Files executable. This mode can also verify a package install.
`--bundle PATH` selects another bundle. Do not interact with a test desktop or
run concurrent suites against the same guest during execution.

### FreeBSD Files

Build `gds-files` for `x86_64-unknown-freebsd` using Rust 1.100 and a FreeBSD
toolchain, or run `cargo build --locked --bin gds-files --features files` on
FreeBSD with Rust, Python 3, pkg-config, Fontconfig and libxkbcommon installed.
The default E2E selection uses the four Linux guests and the Linux build bundle.
Select FreeBSD explicitly and point the runner at a directory containing the
native executable.

```sh
just e2e-prepare freebsd
just vm freebsd copy /path/to/gds-files
just test-files --vm freebsd --guest-bin-dir /home/atlas/Downloads
```

The runner reads the Wayland session from `plasmashell` and sends input using
the same libvirt and AT-SPI interfaces as in the Linux guests.
See the [FreeBSD Files test report](../apps/files/freebsd-testing.md) for
verified workflow coverage, FreeBSD backend mechanisms, and remaining
Linux-only desktop boundaries.

### Guest state

Preparation installs Python, AT-SPI introspection, Pillow and `wl-clipboard`.
The runner temporarily enables accessibility and unlocks test desktops. Noctalia
uses `grim` for compositor screenshots because its QEMU display has no screendump
surface; other guests use libvirt. This choice is recorded in the environment.
The runner does not reset disk snapshots. The Nix bundle preserves ELF interpreters and
dependencies under `/nix/store`, using the guest's compositor and Mesa drivers.
Build bundles are cached under `/var/tmp/gds-vm-e2e/bundles` to avoid tmpfs user
quotas; per-case fixtures live under `/tmp/gds-vm-e2e/run-*`. After inspecting a
run's host artifacts, its corresponding guest run directory may be removed.
It includes binary hashes, revision, source diff and a source archive including
uncommitted build inputs. Each invocation separately archives its exact Python
runner and lockfile so tests changed after a binary build remain reproducible.

## Write a workflow

Add straight-line pytest functions in `tests/e2e/test_files.py` or
`test_images.py`. Each `case` owns fresh HOME/XDG directories, Trash, fixtures
and application processes. Restarts within a case share only that case's state.
pytest parametrizes the workflows across the selected VMs.

```python
def test_rename_document(case):
    case.write('original.txt', 'payload')
    case.launch('files', case.work)
    case.select('original.txt')
    case.key('f2')
    case.expect('Cancel', 'button')
    case.key('ctrl+a')
    case.type('renamed.txt')
    case.key('enter')
    case.expect('renamed.txt')
    case.expect_file('renamed.txt', 'payload')
    case.expect_file('original.txt', exists=False)
```

Use `case.step(...)` to name compound assertions and `case.checkpoint(...)` to
capture intermediate screenshots, accessibility trees and file hashes.
Expectations poll observations with deadlines. Inputs and failed workflows are
never retried. Test bodies contain no sleeps, fallback selectors or control flow.

Keyboard events enter through QEMU's virtual input device. Controls are located
by accessible role/name and activated through AT-SPI; file selectors also check
full-path descriptions. No application business methods or test-only state are
exposed. Typed text currently assumes a US keymap. Unicode fixture paths can be
passed through argv; this does not certify real IME composition.

AT-SPI supplies UI evidence. SSH supplies independent filesystem, process and
clipboard evidence. Pillow checks exported dimensions and RGBA hashes independently
of GDS's image rendering. The framework reuses
[pytest fixtures](https://docs.pytest.org/en/stable/how-to/fixtures.html),
[parametrization](https://docs.pytest.org/en/stable/how-to/parametrize.html),
[AT-SPI](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/index.html)
and [libvirt input](https://www.libvirt.org/manpages/virsh.html#send-key).

## Results

Every invocation creates a fresh `target/vm-e2e/run-*` directory:

| Artifact | Purpose |
| --- | --- |
| `results.json`, `results.md`, `junit.xml` | Per-workflow/per-VM status, duration, failing phase and assertion |
| `invocation.json` | Selection and replay command |
| `<vm>/environment.json`, `build.json` | Actual desktop and build provenance |
| `<vm>/<case>/steps.jsonl` | Ordered input/expectation records and first failure |
| `*.png`, `*.ui.json`, `*.files.json` | Screenshot, accessibility tree and independent file hashes |
| `logs/`, `guest-state.tar.gz` | Process output and isolated fixture for inspection |

`PASSED` requires all UI and independent assertions. `FAILED` records the
failing step and phase, distinguishing startup, lookup and product assertions.
JSON and Markdown reports are updated atomically after every phase. A case is
passed only after teardown completes, so interruption preserves completed results
and leaves unfinished cases planned. JUnit is finalized at normal pytest shutdown.
`BLOCKED` means a required environment is unavailable: JUnit represents it as
skipped, but the command still exits nonzero. Selected unexecuted workflows
remain `PLANNED`. Collection-only results never count as passes. A later run
never replaces the artifacts of an earlier failure.

## Coverage and migration

The baseline covers local navigation/history, filtering/hidden files,
create/rename/undo/redo, clipboard copy/move, conflict cancellation,
trash/permanent deletion, external changes, view persistence, independent tabs,
CLI picker, terminal and permission recovery. Images covers gallery navigation,
transform history, save/reopen, exact clipboard pixels, stdin capture, rename,
external-edit protection, settings, real-pointer crop and rectangle annotations.
Pointer workflows locate the complete synthetic image at 1:1 scale in the
desktop screenshot, then send QEMU pointer events; no fixed screen coordinates
or UI golden images are used. Cropping allows one pixel at each pointer-selected
edge and still requires exact decoded pixels throughout the resulting crop.
The tests define the precise contracts; a passing
task subset does not certify every variant in earlier planning documents.

The additional acceptance work remains explicit:

| Area | Required extension |
| --- | --- |
| Cross-application file exchange and drag/drop | Nautilus/Dolphin scenarios and pointer transport |
| Portal open/save | Real calling application and explicit guest portal routing |
| Remote transfers | Resettable SFTP/SMB services and network faults |
| Devices/cross-filesystem/ENOSPC | Disposable disks and controlled filesystems |
| Tags | Isolated Baloo/KIO index and Dolphin |
| IME, scaling, large directories | Input method, display and performance profiles |
| Images metadata/advanced editing | Metadata, remaining annotation tools and export format variants |
| Windows/macOS/X11 | Platform adapters and declared capability profiles |

These are not silently skipped cases or included in executed pass counts. They
must be implemented before claiming full product acceptance.

`just test-files-native` retains the earlier Weston runner during migration; it
cannot establish desktop clipboard compatibility. Remove overlapping micro-tests
after executing their VM replacements. Keep deterministic coverage for exact
inode races, async completion ordering, difficult error boundaries and complex
pure algorithms. The duplicated Files sidebar-animation test, Images
sidebar/inspector-animation test and Images CLI flag-parsing test have been
removed. Their user-facing contracts are exercised by VM navigation, view
persistence, gallery/settings, capture and rename workflows. The remaining
popup-menu regression and the Weston IME/protocol scenarios stay until their
specific contracts are migrated.
