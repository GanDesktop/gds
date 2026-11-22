# GDS Files on Redox

The GDS shared library can run configuration, localization, and database code on Redox. A full GDS Files port is premature. The desktop binary fails to compile in two dependencies, and its renderer still needs a way to present frames in an Orbital window.

This assessment covers native Orbital on `x86_64-unknown-redox` as of 2026-11-22. The checks used Rust 1.100.0, the Redox C toolchain, and the GDS working tree based on `e1b970f9f046`, including the local changes present during testing.

## Dependencies that affect Files

| Dependency in the tested build | Evidence | Effect on Files |
| --- | --- | --- |
| `portable-pty 0.9.0` → `serial2 0.2.39` | Cross-compilation produced 15 errors in `serial2`. Its Unix implementation references ioctl and flow-control constants absent from Redox's Rust libc bindings, including `TIOCM_RTS` and `CRTSCTS`. | `portable-pty` requires `serial2` even when Files only opens a pseudoterminal. This blocks the embedded terminal. `alacritty_terminal` itself passed the target check. [Dependency declaration](https://github.com/wezterm/wezterm/blob/f8921727a11b9f8b073e8c24821d72fd41283500/pty/Cargo.toml) · [Serial implementation](https://github.com/de-vri-es/serial2-rs/blob/ae370aada44d925f32050df91b511f12c033ea6b/src/sys/unix/mod.rs#L345) |
| GanDesktop's `zgui-platform-winit` at `e90275a7` | The clipboard write method fails with `E0317`. After platform selection, one path returns `()` where Rust requires `Result<(), ClipboardError>`. | The error comes from the fork's multi-MIME clipboard change. Original zgui returns the result of `set_text` directly. [Fork change](https://github.com/GanDesktop/zgui/commit/7df7d11b216d7d0d2ba78ac2545b6e06856f6e8f) · [Original implementation](https://github.com/zortax/zgui/blob/a3721586bf3d0422796efb7946cd01b1a1dd0e76/crates/zgui-platform-winit/src/clipboard/mod.rs#L214) |
| `wgpu-hal 30.0.2` | `winit 0.30.13` supplies an Orbital window handle. The Vulkan and EGL surface implementations lack a case for that handle, including on the upstream main branch examined. | The current renderer cannot present to a native Orbital window. [winit implementation](https://github.com/rust-windowing/winit/blob/e9809ef54b18499bb4f2cac945719ecc2a61061b/src/platform_impl/orbital/window.rs#L92) · [Vulkan](https://github.com/gfx-rs/wgpu/blob/38bb981557886b40fdd338b7f9f052032e417cb6/wgpu-hal/src/vulkan/instance.rs#L1108) · [EGL](https://github.com/gfx-rs/wgpu/blob/38bb981557886b40fdd338b7f9f052032e417cb6/wgpu-hal/src/gles/egl.rs#L1022) |
| `arboard 3.6.1` | Redox selects the Unix backend. The features enabled by GDS select X11. | Native Orbital copy and paste still need a backend after the zgui compile error is fixed. [Platform selection](https://github.com/1Password/arboard/blob/a3750c79a5d63f3987317e03c412b7d9dffdc2af/src/platform/mod.rs) |
| `fontique 0.11.1` | Redox selects the empty system-font backend. | Automatic system font discovery and fallback are unavailable. Applications can still register fonts explicitly. [Font backend selection](https://github.com/linebender/parley/blob/eea3503dd6cf17130cbb07348e0ff2c918300e94/fontique/src/backend/mod.rs) |
| `accesskit_winit 0.33.2` | Redox selects the null adapter, which ignores window events and accessibility tree updates. | The operating system's accessibility tools cannot receive the Files control tree through this adapter. [Adapter selection](https://github.com/AccessKit/accesskit/blob/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/winit/src/platform_impl/mod.rs) |

## Shared library results

Thirteen of the 17 individually checked library configurations passed `cargo check`. The full `gds-files` binary was checked separately with `--features files --keep-going`, which exposed both compilation failures above.

| Verification | Result |
| --- | --- |
| Ran in the Redox guest | `i18n` loaded a Fluent message. `desktop-protocol` round-tripped a chooser filter through JSON. `persistence` created a configuration file on RedoxFS and read its contents back. `task-manager` started the Tokio runtime. `storage` created and migrated a SQLite database, then reopened it and confirmed the migration was already applied. |
| Compiled for Redox | The default library configuration and the `watch`, `logs`, `desktop`, `remote`, `file-inspection`, and `image-engine` features passed. Directory change delivery, desktop file opening, and SFTP/SMB sessions still need end-to-end verification. |
| Compiled with a runtime limitation | `file-tags` passed the target check. Reading tags in the guest returned `Unsupported`. |
| Failed to compile | `app`, `appearance`, `ui`, and `terminal`. |

GDS also has its own platform gates. On Redox, the tested Files code rejects local copy and move operations and renames that must preserve an existing destination. It disables the trash backend. These behaviors were identified in the source. [Transfers](../../../bin/files/src/features/file_operations/api/transfer.rs) · [Rename](../../../bin/files/src/features/file_operations/api/rename.rs) · [Trash](../../../bin/files/src/features/file_operations/recycle.rs) · [File tags](../../../src/service/file_tags.rs)

## Guest and local evidence

The `redox-dev` QEMU/KVM guest uses Redox's x86_64 Desktop daily image from 2026-11-19, build 589, verified against the official SHA-256 manifest. On the test workstation, open **GDS Test — Redox Desktop** and run `~/Downloads/gdsprobe` and `~/Downloads/gdsdbprobe` in the guest terminal to repeat the runtime checks. [Official VM guide](https://github.com/redox-os/book/blob/master/src/running-vm.md)

The workstation retains the feature matrix in `target/redox-assessment/results.tsv`, the binary build diagnostics in `target/redox-assessment/files-sdk.log`, and the guest output screenshot in `~/.local/share/gds-vms/redox/guest-probe.png`.
