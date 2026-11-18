# Development & Architecture Guide

This experimental branch builds GDS Files with AntiWebUI and the UI-independent `xdg-desktop-portal-gds` from one Rust crate. Files uses the Wayland backend and UIM v2. Agents, Nixes and Images source remains available but their binary targets are excluded until their migration.

## Repository Layout

```text
Cargo.toml                 # Shared library features, dependencies, and all [[bin]] targets
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
├── files/                 # GDS Files (FSD application)
├── agents/                # GDS Agents (FSD application)
├── nixes/                 # GDS Nixes (FSD application)
└── portal/                # GDS Portal (headless FileChooser D-Bus service)
```

Generated binaries, distro packages, E2E artifacts, and build caches stay under `target/`.

## Quick Start & Daily Commands

Run inside a graphical Wayland terminal. The [justfile](../justfile) automatically evaluates [shell.nix](../shell.nix) to enter the locked Nix environment:

```sh
just dev files                    # Launch GDS Files in debug mode
just dev files "/path/to/folder"  # Open a specific folder

just build [files|portal]        # Incremental debug build
just release [files|portal]      # Optimized release build
just check                        # Typecheck workspace and test targets
just lint                         # Check formatting and workspace Clippy lint gates
just fmt-check                    # Check Rust formatting (just fmt to format)
just test                         # Run workspace unit & integration tests
just e2e-prepare all              # Prepare existing desktop test VMs
just e2e-build                    # Bundle normal Files builds for VMs
just test-e2e                     # Run the retained desktop workflows
just test-files --vm ubuntu       # Select Files workflows and one guest
just shell                        # Open an interactive Nix development shell
```

Without `just` installed on the host, invoke `nix develop --impure --expr 'import ./shell.nix' --command just dev files` (avoid bare `nix develop`, which snapshots untracked `target/` artifacts; see [Runtime & Platform Compromises](decisions/runtime-and-platform.md)).

### Rapid Iteration (`install-dev`)

When iterating on desktop integration or UI, `just install-dev files` builds `target/debug` incrementally and installs lightweight user-level launchers (`Files (Dev)`) that execute the workspace binary directly with required runtime libraries and helpers (`xdg-terminal-exec`, `ffmpeg`, `terminfo`) while preserving your desktop session's `SHELL`, `WAYLAND_DISPLAY`, and D-Bus environment. Run `just uninstall-dev` to restore any prior release launchers.

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
| `app` | Typed `Gds` services and native owner-bound task completion |
| `terminal` | Local PTY session worker and `alacritty_terminal` state |
| `file-inspection` | On-demand image/audio/video/text metadata probing and freedesktop PNG thumbnail cache |
| `remote` | Native Rust SFTP (`russh-sftp`) and SMB (`smb`) services |
| `appearance` / `i18n` / `ui` | Theme/font/token engine, Fluent translations, and reusable native components |
| `files` | Native AntiWebUI Files application bundle; Agents, Nixes and Images targets are excluded on this branch |
| `portal` | Headless `xdg-desktop-portal-gds` bundle (never pulls in AntiWebUI, SQLite, terminal, or `remote`) |
| `gui-tests` / `files-e2e` | Native test feature gates and the isolated Files driven-renderer/Wayland workflow |

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

| Layer | Responsibility | Files (`bin/files/src/`) | Agents (`bin/agents/src/`) |
| --- | --- | --- | --- |
| `app` | CLI, bootstrap, lifecycle, window host, desktop D-Bus | `cli`, `config`, `desktop`, `ui`, `windows` | `cli`, `startup` |
| `pages` | Top-level window workspaces | `workspace` (browser, picker, settings host) | `workspace` (session workspace, compact popup) |
| `widgets` | Composable self-contained views | `settings`, `help` | `settings`, `history`, `help` |
| `features` | User workflows & mutations | `file_operations`, `pick_files`, `connect_server` | `navigation`, `agent_auth`, `delete_session`, `workspace_launch` |
| `entities` | Domain state, storage & protocol models | `file`, `location`, `preferences` | `agent`, `backend`, `session` |
| `shared` | Domain-agnostic app utilities | `i18n`, `lib` (clipboard/selection), `ui` | `i18n`, `config`, `api`, `lib`, `ui`, `routes` |

## Shared Library Boundaries (`src/`)

`src/lib.rs` exposes six top-level modules:

1. **`app`**: Provides `Gds::current()` and typed `tasks()`, `appearance()`, `i18n()`, and `ui()` accessors. `Gds::with` registers process-owned reactive work under the shell root. Window services and completion callbacks use their native window owner; no compatibility `Context` or `Entity` layer is retained.
2. **`appearance`**: Owns `theme` (providers, semantic `Palette`, owner-bound hot reload), `typography` (Fontconfig aliases and scaling), and layout `tokens`.
3. **`service`**: Pure Rust / Tokio runtime services (`task_manager`, `desktop`, `remote`, `storage`, `terminal`, `file_inspection`, `logs`, `watch`). **Services never reference `ui` or AntiWebUI types.**
4. **`ui`**: Reusable anatomical AntiWebUI components (`frame`, `sidebar`, `page`, `forms`, `feedback`, `interaction`, `terminal`, `logs`, `time`, `about`) and UI-side caches (`ui::file_inspection::Cache` deduplicating metadata and thumbnail requests). Files owns its chooser workflow and uses native drag primitives.
5. **`persistence`**: Atomic file replacement, lock files, and protection against clobbering read-only or declaratively managed symlinks.
6. **`i18n`**: Fluent bundle loading and context-free reactive text helpers.

### Execution & Shutdown Model

AntiWebUI owns rendering, signals and native reactive lifetimes. `service::task_manager::Supervisor` owns the multi-threaded Tokio runtime for application I/O. Services accept `Scope`s; `Task::on_complete` forwards results to native owner-bound channels without creating another executor. Closing a window cancels its scope and disposes its UI callbacks. After the native event loop exits, `Supervisor::shutdown` seals admission and drains tracked cleanup before dropping the runtime. Keep app-wide watchers under `Gds::with`, and never publish reactive changes while holding a workspace `RefCell` borrow.

## UIM, Translations & Shared Feedback

- **Declarative UI (`gds-uim`)**: Construct trees with `el!` and `m!` using the `.Component::default` / `# id_expr` syntax (see [gds-uim grammar](https://github.com/GanDesktop/gds-uim/blob/90f5424edf6ba9c5ffaacc2b11cb5ffe24960519/docs/grammer.md) and [Component Composition](visual-language/composition.md)). Escape into `$(...)` only for field access, method chains, or arithmetic.
- **Fluent i18n (`gds::i18n::t`)**: Store `.ftl` files under `i18n/assets/{en-US,zh-CN}/`. Native UI uses `t!("key", name = value)` or context-free `i18n::text` / `i18n::count`; reactive reads follow the installed catalogue. Desktop `.desktop` files use POSIX locales (`zh_CN`) generated from `assets/<app>/desktop.toml`.
- **Notifications (`gds::ui::notifications`)**: Bind windows with `notifications::bind_window(window, cx)`, then emit `error` / `warning` / `info` / `success`, `Report` (with copyable details), or `Operation` (progress-to-result slot reuse).
- **Dialogs (`DialogActions`)**: Modal confirmations use `DialogActions::default().primary_label(...).primary_icon(...)` (`Alt+A` / `Enter` confirm, `Alt+C` / `Esc` cancel; `.danger()` for destructive actions).
- **Diagnostics (`--enable-logs`)**: Disabled by default with zero allocation overhead. Passing `--enable-logs` activates `service::logs` (up to 2,000 entries / ~2 MiB in-memory) and `ui::logs::Viewer` + the Tasks inspector via `Ctrl+Shift+L`. `RUST_LOG` filters stderr/collector verbosity without implicitly enabling the GUI viewer.
- **External Terminal**: Launching an external terminal uses `GDS_TERMINAL` (single executable name, not a shell string) or falls back to `xdg-terminal-exec --dir=…`.

## Release & Distribution Pipeline

- **Cargo Release Profile**: `opt-level = 3`, `lto = false`, `codegen-units = 16`, `debug = 0`, `strip = "symbols"`, `incremental = false`, keeping normal `unwind` panic semantics.
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
