# Development & Architecture Guide

GDS currently builds native `gds-files` on [`zgui`](https://github.com/GanDesktop/zgui/tree/gds-0.1.0) (`view!`, Stylo `--gds-*`, Wayland + X11) and the UI-independent portal daemon (`xdg-desktop-portal-gds`) from a single Rust crate. Unmigrated companion apps (`bin/agents/`, `bin/nixes/`) retain their source on disk while excluded from active `[[bin]]` targets.

## Repository Layout

```text
Cargo.toml                 # Shared library features, dependencies, and active [[bin]] targets
LICENSE                    # Apache-2.0 license shared by all first-party code
src/                       # Shared library (gds): app, appearance, service, ui, persistence, i18n
macros/                    # Procedural macros (gds-settings-derive)
assets/                    # Per-app icons (icon-solid.svg, icon-desktop.svg) and desktop.toml
flake.nix / shell.nix      # Nix flake and local dev-shell entry points
packaging/
├── nix/                   # Nix derivations, NixOS & Home Manager modules, NUR entry
├── fedora/                # Fedora RPM spec and build container
├── debian/                # Debian/Ubuntu debhelper rules and build container
├── arch/                  # Arch PKGBUILD and build container
├── linux/                 # Shared native Linux desktop asset staging
└── cloudflare/            # Installer worker (gds.gandesktop.dev) and R2 bucket routing
bin/
├── files/                 # GDS Files (FSD application on zgui)
├── portal/                # GDS Portal (headless FileChooser D-Bus service)
├── agents/                # Retained unmigrated source (excluded from active targets)
└── nixes/                 # Retained unmigrated source (excluded from active targets)
```

Generated binaries, distro packages, E2E artifacts, and build caches stay under `target/`.

## Quick Start & Daily Commands

Run inside a graphical Wayland or X11 terminal. The [justfile](../justfile) automatically evaluates [shell.nix](../shell.nix) to enter the locked Nix environment:

```sh
just dev files                    # Launch GDS Files in debug mode
just dev files "/path/to/folder"  # Open a specific folder

just build [files|portal|all]     # Incremental debug build
just release [files|portal|all]   # Optimized release build
just check                        # Typecheck Files, Portal, and test targets
just lint                         # Check formatting and workspace Clippy lint gates
just fmt-check                    # Check Rust formatting (just fmt to format)
just test                         # Run workspace unit & integration tests
just e2e-prepare all              # Prepare existing desktop test VMs
just e2e-build                    # Bundle normal Files builds for VMs
just test-files --vm ubuntu       # Run Files VM workflows on one guest
just test-files-native            # Run the private headless Weston E2E runner
just shell                        # Open an interactive Nix development shell
```

Without `just` installed on the host, invoke `nix develop --impure --expr 'import ./shell.nix' --command just dev files` (avoid bare `nix develop`, which snapshots untracked `target/` artifacts; see [Runtime & Platform Compromises](decisions/runtime-and-platform.md)).

### Rapid Iteration (`install-dev`)

When iterating on desktop integration or UI, `just install-dev files` builds `target/debug/gds-files` incrementally and installs a lightweight user-level launcher (`Files (Dev)`) that executes the workspace binary directly with required runtime libraries and helpers (`xdg-terminal-exec`, `ffmpeg`, `terminfo`) while preserving your desktop session's `SHELL`, `WAYLAND_DISPLAY`, `DISPLAY`, and D-Bus environment. Run `just uninstall-dev` to restore any prior release launchers.

## Crate Features & Capability Isolation

`gds` enables **zero features by default**. Modules and their third-party dependencies activate together behind matching capability features:

| Feature | Scope |
| --- | --- |
| `desktop-protocol` | Pure Serde IPC types in `service::desktop::protocol` (shared with `portal`) |
| `persistence` | Atomic config file writes, file locks, and symlink/conflict protection |
| `watch` | Filesystem change monitoring, independent of the UI runtime |
| `desktop` / `file-tags` | OpenURI / AppChooser portals, `FileManager1`, external terminal launch, and optional `user.xdg.tags` xattrs |
| `storage` | Toasty / SQLite initialization, migrations, and connection supervision |
| `logs` | Opt-in in-process `tracing` collector (`--enable-logs`) |
| `task-manager` | `Supervisor`, hierarchical `Scope`s, cancellation, and completion barriers (no UI dependency) |
| `app` | `Gds` service registry, `Tasks::spawn_ui`, and `zgui-tokio` integration (no `ui` dependency) |
| `terminal` | Local PTY session worker and `alacritty_terminal` state |
| `file-inspection` | On-demand image/audio/video/text metadata probing and freedesktop PNG thumbnail cache |
| `remote` | Native Rust SFTP (`russh-sftp`) and SMB (`smb`) services |
| `appearance` / `i18n` / `ui` | Stylo `--gds-*` theme/font/token engine, Fluent translations, and reusable `zgui` components |
| `files` | Full feature bundle for `gds-files` |
| `portal` | Headless `xdg-desktop-portal-gds` bundle (never pulls in `zgui`, SQLite, terminal, or `remote`) |
| `gui-tests` / `files-e2e` | `zgui-platform-headless` test helpers and isolated Weston E2E runner |

Verify standalone feature boundaries directly with Cargo when changing `Cargo.toml`:

```sh
cargo check --lib --no-default-features
cargo check --bin xdg-desktop-portal-gds --no-default-features --features portal
cargo tree --no-default-features --features portal --edges normal,build
```

## Application Architecture (Feature-Sliced Design)

Graphical binaries organize `bin/<app>/src/` strictly along [Feature-Sliced Design](https://feature-sliced.design/docs/reference/layers):

```text
app → pages → widgets → features → entities → shared
```

- **Strict downward imports**: Higher layers may import any lower layer through `<slice>.rs` public entry points; internal segments (`ui`, `model`, `api`) stay private.
- **Peer slice isolation**: Slices on the same layer never import each other. Cross-entity relationships use explicit `@x` adapters (`entities/<provider>/x/<consumer>`, e.g., `entities/file/x/location` and `entities/location/x/tags` in Files). `app` and `shared` are segmented by technical role rather than domain slices.

| Layer | Responsibility | Files (`bin/files/src/`) |
| --- | --- | --- |
| `app` | CLI, bootstrap, lifecycle, window host, desktop D-Bus | `cli`, `config`, `desktop`, `ui`, `windows` |
| `pages` | Top-level window workspaces | `workspace` (browser, picker, settings host) |
| `widgets` | Composable self-contained views | `settings`, `help` |
| `features` | User workflows & mutations | `file_operations`, `pick_files`, `connect_server` |
| `entities` | Domain state, storage & protocol models | `file`, `location`, `preferences`, `tags` |
| `shared` | Domain-agnostic app utilities | `i18n`, `lib` (clipboard/selection), `ui` |

## Shared Library Boundaries (`src/`)

`src/lib.rs` exposes six top-level modules:

1. **`app`**: Provides `Gds::current()` / `Gds::try_current()` with inherent accessors `tasks()`, `appearance()`, `i18n()`, and `ui()`. App-wide watchers register via `Gds::with`; window work uses its reactive `Owner`, `Tasks::owner_scope`, and `Tasks::spawn_ui`.
2. **`appearance`**: Owns `theme` (providers, semantic `Palette`, `Observer` hot-reload), `typography` (Fontconfig aliases and scaling), layout `tokens`, and per-window Stylo custom properties (`--gds-*` / `--zui-*`).
3. **`service`**: Pure Rust / Tokio runtime services (`task_manager`, `desktop`, `remote`, `storage`, `terminal`, `file_inspection`, `logs`, `watch`). **Services never reference `ui` or `zgui` types.**
4. **`ui`**: Reusable `zgui` components (`frame`, `sidebar`, `page`, `forms`, `feedback`, `interaction`, `terminal`, `logs`, `time`, `about`, `picker`, `drag`, `window`, `styles`) and UI-side caches (`ui::file_inspection::Cache` deduplicating metadata and thumbnail requests).
5. **`persistence`**: Atomic file replacement, lock files, and protection against clobbering read-only or declaratively managed symlinks.
6. **`i18n`**: Fluent bundle loading and reactive text helpers (`t!`, `i18n::text`).

### Execution & Shutdown Model

`zgui` drives rendering, Stylo style resolution, signals, and UI-thread tasks (`zgui::reactive::spawn_local`). `service::task_manager::Supervisor` owns a dedicated multi-threaded Tokio runtime for all I/O, bridged via `zgui-tokio` and `Tasks::spawn_ui`. Services accept `Scope` handles to register cancellable or must-drain tasks; explicit quit closes windows, seals task admission, drains tracked cleanup in `Supervisor::shutdown`, and exits the event loop. See [Task Ownership & Shutdown](development/task-manager.md).

## `view!`, Stylo Tokens, Translations & Shared Feedback

- **Declarative UI (`zgui::view!`)**: Construct trees with `view!`, `#[component]`, and `css!`, styled with Stylo classes and `--gds-*` custom properties installed by `Appearance::install_window_sheet()` and `ui::styles::install_ui_stylesheets()`.
- **Fluent i18n (`gds::i18n::t!`)**: Store `.ftl` files under `i18n/assets/{en-US,zh-CN}/`. Use `t!("key", name = value)` or `gds::i18n::text("key")` inside views and callbacks. Desktop `.desktop` files use POSIX locales (`zh_CN`) generated from `assets/<app>/desktop.toml`.
- **Notifications (`gds::ui::notifications`)**: Mount `NotificationHost` inside windows, then emit `error` / `warning` / `info` / `success`, `Report` (with copyable details), or `Operation` (progress-to-result slot reuse).
- **Dialogs (`ModalDialog` / `ConfirmDialog` / `DialogActions`)**: Modal dialogs mount over `ModalDialog` or `ConfirmDialog` with `DialogFooter` + `DialogActions` (`Alt+A` / `Enter` confirm, `Alt+C` / `Esc` cancel, `DialogScope` background keytip suppression, and destructive variants for irreversible actions). Opening a modal dismisses any active transient menu or popover first.
- **Diagnostics (`--enable-logs`)**: Disabled by default with zero allocation overhead. Passing `--enable-logs` activates `service::logs` (up to 2,000 entries / ~2 MiB in-memory) and `ui::logs::Viewer` + the Tasks inspector via `Ctrl+Shift+L`. `RUST_LOG` filters stderr/collector verbosity without implicitly enabling the GUI viewer.
- **External Terminal**: Launching an external terminal uses `GDS_TERMINAL` (single executable name, not a shell string) or falls back to `xdg-terminal-exec --dir=…`.

## Release & Distribution Pipeline

- **Cargo Release Profile**: `opt-level = 3`, `lto = false`, `codegen-units = 16`, `debug = 0`, `strip = "symbols"`, `incremental = false`, keeping standard `unwind` panic semantics.
- **Local Container Packaging**: Build native distro packages from the working tree with `just rpm [43|44] [dev|release]`, `just deb [debian|ubuntu] [trixie|noble|resolute] [dev|release]`, and `just arch [dev|release]`.
- **Automated Tag Releases ([release.yml](../.github/workflows/release.yml))**: Pushing a `v*` tag matching `Cargo.toml` runs a **Draft-First** pipeline:
  1. `prepare` validates version parity, runs installer tests (`python3 -m unittest discover -s scripts/tests -v`), and creates a GitHub Draft Release.
  2. `build` compiles `gds-files` and `xdg-desktop-portal-gds` across Arch, Debian 13 (`trixie`), Ubuntu 24.04 (`noble`), Ubuntu 26.04 (`resolute`), and Fedora 43/44 containers alongside `nix` NUR evaluation.
  3. `smoke-native` verifies `install.sh --yes --with-portal`, `--help`, `desktop-file-validate`, and `--uninstall` inside clean minimal runtime containers.
  4. `publish` generates `MANIFEST` and `SHA256SUMS`, uploads assets to GitHub Releases and Cloudflare R2 (`https://pkgs.gandesktop.dev`), updates `https://gds.gandesktop.dev/install.sh` (also tested and deployed on `main` via [installer.yml](../.github/workflows/installer.yml)), undrafts the release, and advances `nur` / `stable` for non-prerelease tags.

## Architecture Deep Dives

- [Task Ownership & Application Shutdown](development/task-manager.md)
- [Storage & Configuration Persistence](development/storage.md)
- [Agents & BAS Protocol Architecture](development/agents-protocol.md)
- [Desktop Test Virtual Machines](development/virtual-machines.md)
- [Technical Decisions & Active Compromises](decisions/README.md)
