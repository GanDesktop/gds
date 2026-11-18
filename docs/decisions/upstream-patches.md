# Upstream Forks & Dependency Pins

GDS never vendors third-party crate sources into this repository. Instead of maintaining detached crates.io tarball overrides under `[patch.crates-io]`, GDS directly tracks versioned forks under `GanDesktop/*` in `Cargo.toml`, locks exact commits in `Cargo.lock` and `packaging/nix/cargo-lock.nix`, and retires each fork branch once upstream releases an equivalent fix.

## Files runtime

The Files experiment uses [`GanDesktop/AntiWebUI` at `1e55ade`](https://github.com/GanDesktop/AntiWebUI/commit/1e55adecf93bf14d69c0150712de628dced1147b)
on `gds-desktop-integration`, with Wayland as its backend and SVG import enabled. Its patches
provide multi-MIME clipboard snapshots, conditional clipboard replacement,
foreign parent windows, activation tokens, and restoration of reactive ownership
when a UI channel delivers a callback. Native editable-target delegation preserves
type-to-search and IME composition while list navigation keeps focus. Native reduced
motion settings settle decorative animations and pause clocks without stopping
frame callbacks or timers. Tooltip and submenu delays belong to their target owner,
and timer callbacks restore that owner and skip disposed subtrees. Reactive values are
dropped after releasing the runtime borrow, so their destructors can dispose
nested scopes without reentrant-borrow panics. Select indicators are drawn with native borders so
they do not depend on an installed symbol font. Plain labels support shaped end
ellipsis, line limits and optional breaks within long words while retaining their
complete source for accessibility and copying. Raw bitmaps supply their natural
size to `object_fit`. Imported monochrome SVGs share parsed vector geometry and
inherit the text color. Context menus expose their existing placement options while the
application retains the shared open state. Wayland windows can request a minimum
size and constrain their initial client-chosen size to compositor-provided
`configure_bounds`; explicit compositor sizes take precedence, and compositors
without bounds retain the requested size. A window handling user input can request
an activation token with its seat serial and send it to another application window,
including a window on a separate Wayland connection. Clipboard and owner changes preserve
Files' cut-completion and window-lifetime semantics. Retire each patch when equivalent upstream support is
available.

The fork declares its renderer dependencies directly on
[`bea4dev/wgpu` at `ff143ae`](https://github.com/bea4dev/wgpu/commit/ff143aeeb126dce521b87f47547565c4b6a6da1e).
Cargo does not inherit a dependency's `[patch.crates-io]`; direct Git dependencies
keep the same wgpu/naga family in consuming applications without root overrides.

[`gds-uim` at `b67c2a3`](https://github.com/GanDesktop/gds-uim/commit/b67c2a3bb6071b6a92c8f1d0aa739928397c9cb6)
provides UIM v2 and native AntiWebUI composition. Exact commits are pinned in
`Cargo.lock`; Nix source hashes live in `packaging/nix/cargo-lock.nix`.

## Unmigrated GUI source dependencies

Agents, Nixes and Images are excluded from the experimental branch's binary
targets. Their source is retained; the following optional dependencies remain
in the lockfile and do not enter the Files runtime graph.

The existing sources use GPUI Kit `0.10.0` (`gds-0.10.0`) directly against its `gpui-fast` (`0.1.16` / `gds-0.1.16`) monorepo fork:

| Dependency | Fork Repository / Branch | Why Forked | Retirement Condition |
| --- | --- | --- | --- |
| `gpui-kit` | [`GanDesktop/gpui-kit` @ `gds-0.10.0`](https://github.com/GanDesktop/gpui-kit/tree/gds-0.10.0) (`f61ebe1b`) | Forked from `longbridge/gpui-kit` `v0.10.0` (`f697c313`). Upstream `v0.10.0` adopted configurable Linux client-side corner radii (`Theme::window_radius`), so `gds-0.10.0` only points workspace GPUI dependencies directly to `GanDesktop/gpui-fast`. | Upstream `gpui-kit` depends on an upstream `gpui-fast` release containing all required GDS platform and Vulkan fixes. |
| `gpui-fast` (`gpui`, `gpui_linux`, `gpui_wgpu`, `gpui_platform`, `compat/gpui-pre*`) | [`GanDesktop/gpui-fast` @ `gds-0.1.16`](https://github.com/GanDesktop/gpui-fast/tree/gds-0.1.16) (`8e6e750c`) | Forked from `longbridge/gpui-fast` `v0.1.16` (`454c29e`). Exposes external Wayland (`xdg-foreign-v2`, `xdg-dialog`) and X11 (`WM_TRANSIENT_FOR`) parent window handles across modal teardown, startup activation tokens (`StartupId`), drag-enter mouse modality recovery, core Wayland clipboard MIME transport, single-roundtrip `zwp_linux_dmabuf_v1` `dev_t` probing, and hybrid-GPU `VK_LUNARG_direct_driver_loading` initialization (see [Hybrid-GPU Direct Vulkan Initialization](hybrid-gpu-vulkan-init.md)). | Upstream `longbridge/gpui-fast` and `wgpu` ([gfx-rs/wgpu#9912](https://github.com/gfx-rs/wgpu/issues/9912)) merge matching platform integration and targeted Vulkan initialization semantics. |

> [!NOTE]
> Because `gpui-kit` enables features additively across its compatibility packages (`compat/gpui-pre*`), `gpui-pre` aliases resolve directly to `GanDesktop/gpui-fast` alongside `gpui-fast` in `Cargo.lock` with zero duplicate crates.io GPUI snapshots. Whenever updating `gds-0.1.16` or `gds-0.10.0`, update both `Cargo.lock` and `packaging/nix/cargo-lock.nix`.

## Pinned Crate Versions & Features

- **`smb = "=0.10.2"` (with `russh = 0.65.0`, `russh-sftp = 3.0.1`)**: Newer `smb` releases pull in `RustCrypto` prerelease crates that conflict with `russh`'s stable `RustCrypto` dependency graph. Keep `smb` pinned until `russh` and `smb` align on the same `RustCrypto` major generation.
- **Embedded resources**: Files includes icons and Fluent `.ftl` catalogues at compile time. The retained GUI source bundles still enable `rust-embed`'s `debug-embed`; it is outside the native Files graph.
