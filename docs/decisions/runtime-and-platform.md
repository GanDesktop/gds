# Runtime, Clipboard & Platform Compromises

## 1. Injecting Tokio `zbus` Connections into `ashpd`

- **Context**: GDS standardizes all application async I/O (UDisks2, Settings Portal, FileManager1, OpenURI) on a single Tokio runtime owned by `service::task_manager::Supervisor`. Enabling `ashpd`'s `tokio` feature directly keeps the dependency graph free of `async-io`, and GDS constructs its shared `zbus::Connection` inside Tokio and passes it explicitly into `ashpd` requests via `OpenFileRequest::connection(...)` rather than creating a separate portal connection per call.
- **References**: [`zbus` Tokio runtime support](https://docs.rs/zbus/5.19.0/zbus/#special-tokio-support).

## 2. Multi-MIME File Clipboard & Unfocused Fallback

- **Context**: GDS Files exchanges copied/cut file URI lists (`text/uri-list`, `x-special/gnome-copied-files`, `application/x-kde-cutselection`, `application/x-gds-clipboard-id`, `text/plain;charset=utf-8`) and rich remote URI offers without privileged compositor clipboard protocols (`wlr-data-control` or `ext-data-control`).
- **Compromise**:
  - On **Linux Wayland and X11** (plus `zgui-platform-headless`), GDS uses `zgui`'s `Clipboards::read_mime` and `Clipboards::write_mime` with offer revision compare-and-swap (`ClipboardError::Changed`) so cut-completion and explicit removal never overwrite a newer clipboard selection.
  - On **Wayland**, claiming the selection over core [`wl_data_device`](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_data_device) requires active focus and a seat input serial (`ClipboardError::NotFocused` when unfocused). Because headless Weston E2E sessions and synthetic UI drivers without physical seat serials cannot claim compositor clipboard ownership, GDS retains an in-process `Snapshot` cache fallback (keyed against the observed platform offer revision) whenever `write_mime` reports `NotFocused`, `UnsupportedFormat`, or `UnsupportedKind`.
  - Non-Linux desktop targets (`arboard` fallback) lack multi-MIME file offers and use the same in-process `Snapshot` cache fallback for file copy/cut metadata.

## 3. Niri-Only `WindowControls` Hiding vs Tiling Surface Opacity

- **Context**: GDS detects the desktop environment in two separate places with intentionally asymmetric scope:
  - **Default surface opacity** (`85%` content / `50%` sidebar vs `100%` / `85%`) recognizes tiling compositors broadly (`niri`, `sway`, `hyprland`).
  - **Hiding client-side `WindowControls`** (minimize/maximize/close) triggers **only on Niri** (`XDG_CURRENT_DESKTOP` containing `niri`, or non-empty `NIRI_SOCKET` when `XDG_CURRENT_DESKTOP` is unset) or when the compositor negotiates server-side decorations.
- **Why**: Sway and Hyprland setups vary widely in floating rules, titlebars, and keybind expectations, whereas Niri consistently manages window lifecycle without CSD buttons. Keep `WindowControls` capability-driven on non-Niri compositors rather than hiding close buttons across all tiling WMs.

## 4. Direct `shell.nix` Evaluation & `NIX_CFLAGS_LINK` Reset

- **Why `shell.nix` over bare `nix develop`**: In an uncommitted working directory or worktree with a large `target/` build directory, running bare `nix develop` against `flake.nix` can copy untracked files into `/nix/store`. `justfile` instead evaluates `import ./shell.nix` directly—reading the exact pinned revisions and NAR hashes from `flake.lock` while sharing `packaging/nix/dev-shell.nix`—zeroing store snapshot overhead.
- **Why `dev-shell.nix` resets `NIX_CFLAGS_LINK`**: Launching a shell from inside FHS wrappers (such as `steam-run`) can inject `NIX_CFLAGS_LINK="-L/usr/lib -L/usr/lib32"`, causing Nix's linker and ALSA/system libraries to mix incompatible host `glibc` symbols (e.g., `undefined reference to sqrtf@GLIBC_2.43`). Resetting `NIX_CFLAGS_LINK` inside the dev shell keeps builds hermetically bound to the locked Nix toolchain.
