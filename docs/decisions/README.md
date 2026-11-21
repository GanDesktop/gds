# Technical Decisions & Active Compromises

This directory records **active architectural trade-offs, upstream patches, and platform workarounds** in GDS—explaining *why* non-obvious constraints exist today and *under what conditions* a compromise can be retired.

Current system behavior belongs in the main documentation (`docs/development.md`, `docs/visual-language/`, `docs/apps/`), and unimplemented plans belong in [docs/roadmap/](../roadmap/README.md).

## Index

| Document | Scope |
| --- | --- |
| [Upstream Forks & Dependency Pins](upstream-patches.md) | Why `zgui` tracks the direct `GanDesktop/zgui` (`gds-0.1.0`) Git fork without `[patch.crates-io]`, plus `smb = 0.10.2` and `rust-embed` constraints |
| [Hybrid-GPU Direct Vulkan Initialization](hybrid-gpu-vulkan-init.md) | Conservative allowlist `VK_LUNARG_direct_driver_loading`, single-roundtrip `dmabuf` `dev_t` probe, and clean `VULKAN \| GL` fallback avoiding ~2 s dGPU wakes |
| [Runtime, Clipboard & Platform Compromises](runtime-and-platform.md) | Injecting Tokio `zbus` into `ashpd`, multi-MIME file clipboard CAS with unfocused fallback, Niri window controls vs tiling opacity, and `shell.nix` / `NIX_CFLAGS_LINK` build guards |
| [Terminal PTY Loop & Directory Policy](terminal-pty-loop.md) | Why `service::terminal` implements its own Tokio `AsyncFd` + `SIGCHLD` session loop instead of Alacritty's upstream event loop, and why directory switching never injects `cd` into live shells |
| [External Tool Integrations](external-integrations.md) | CLI-based Baloo/KIO tag queries vs linking Qt/C++, external archive delegation (`OpenURI` vs CLI `7zz`), and non-NixOS `--nix` OpenGL limitations |
