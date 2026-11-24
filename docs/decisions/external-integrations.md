# External Tool Integrations

## 1. CLI-Based Baloo / KIO Tag Queries (`balooctl` & `kioclient`)

- **Context**: GDS Files reads and writes per-file `user.xdg.tags` extended attributes natively in Rust (`gds::service::file_tags`), while global `tags:/` catalog browsing and tag-wide removal integrate with KDE Dolphin's Baloo index.
- **Compromise**: Rather than linking Qt6 / KDE Frameworks C++ libraries into `gds-files` (or maintaining a second private file-indexing daemon), GDS invokes `balooctl6`/`balooctl` and `kioclient6`/`kioclient` via `argv` arrays with a 15-second timeout and task cancellation.
- **Known trade-offs**:
  - **Baloo 6.31 double-quote bug**: Upstream Baloo's `tags:` KIO worker returns an empty set when a tag query contains `"`. GDS Files surfaces an explicit error for double-quoted global queries/deletions while keeping per-file xattr editing intact.
  - **Line-oriented, unpaginated CLI output**: `kioclient ls` cannot losslessly represent tag names containing literal newlines (disallowed by GDS's tag validator), returns unpaginated listings unsuitable for massive tag catalogs, and depends on KDE CLI formatting stability.
- **Retirement condition**: Replace or supplement if KDE exposes a stable pure D-Bus query interface for `tags:/` or if GDS introduces an optional native index backend.

## 2. Delegating Archives to External Desktop Handlers

- **Context**: Users expect `.zip`, `.7z`, `.tar`, and `.tar.gz` files to open predictably today before GDS's built-in archive browser lands.
- **Compromise**: Double-clicking or pressing `Enter` on an archive delegates to the desktop's registered handler via the XDG `OpenURI` / `AppChooser` Portal without adding partial extract/compress items to Files' menus. Note that upstream Linux 7-Zip releases ship only the `7zz` CLI binary rather than a GUI `.desktop` handler, so NixOS/desktop deployments must pair MIME associations with an actual graphical archive manager.
- **Retirement condition**: Superseded once Phase 1 read-only archive browsing and background extraction land ([Files Roadmap](../roadmap/files.md)).

## 3. `--nix` Installer Mode on Non-NixOS Wayland Hosts

- **Context**: `scripts/install.sh --nix` supports installing `github:GanDesktop/gds#gds-files` onto non-NixOS distributions that have Nix installed.
- **Compromise**: Nix-built GUI binaries link against Nixpkgs' Mesa/Vulkan loader rather than the host distribution's `/usr/lib` graphics drivers (`nixGL` / `nix-gl-host` territory). On non-NixOS Wayland desktops (Ubuntu, Fedora, Debian, Arch), native distro packages (`.deb`, `.rpm`, `.pkg.tar.zst`) are therefore always the primary path in `install.sh`, reserving `--nix` for NixOS or users who explicitly manage non-NixOS GPU wrappers.

## 4. FreeDesktop `.thumbnailer` Helpers with Optional Bubblewrap Sandboxing

- **Context**: Common raster images (`image`), SVG (`resvg`), and embedded audio covers (`lofty`) render in-process in pure Rust without external dependencies. Video frames, PDFs, fonts, 3D models, RAW/HEIC/PSD, and other formats rely on specialized native parsers that GDS should not bundle or maintain in-tree.
- **Compromise**: GDS Files discovers standard XDG FreeDesktop `.thumbnailer` descriptors (`$XDG_DATA_HOME/thumbnailers` overriding `$XDG_DATA_DIRS/thumbnailers`), parses `Exec=` directly without a shell, enforces process-group timeouts and bounded PNG decoding, and delegates video thumbnails to external tools such as `ffmpegthumbnailer` rather than invoking `ffmpeg` directly. On Linux, Bubblewrap (`bwrap`) sandboxing defaults to enabled and is recommended in distro packaging; users may explicitly disable sandboxing in **Settings → Extensions** (and non-Linux Unix platforms such as FreeBSD default to unsandboxed execution with an explicit notice).
- **Retirement condition**: Revisit if a cross-platform unprivileged sandbox or standardized portal-based thumbnailing service replaces `.thumbnailer` helpers across Linux and BSD desktops.
