# Shared Themes & Materials

`src/appearance/theme` resolves external, built-in, and user-authored color palettes into one source-agnostic semantic snapshot (`Palette` / `Colors`) consumed by `zgui-ui-tokens` (`--zui-*`), Stylo `--gds-*` custom properties, and GDS components. External themes supply colors only—never layout metrics, corner radii, or widget chrome.

```text
System (Noctalia / DMS / KDE / GNOME)  |  Built-in TOML  |  Custom User TOML
                                      ↓
               provider: palette roles + effective light/dark mode + watched paths
                                      ↓
                        Palette / Colors immutable snapshot
                                      ↓
                   --zui-* + --gds-* Stylo sheet & GDS components
```

## 1. Theme Sources & Light/Dark Mode

Appearance settings expose three user-selectable theme sources—**System**, **Built-in**, and **Custom**—alongside an independent **Mode** selector (**System**, **Light**, **Dark**). Switching theme sources preserves the user's saved mode, font, built-in palette choice, and surface opacities.

### System (`Source::System`)

Selected by default (`dms` in legacy configs deserializes as `system`). Resolves from the active desktop session (`XDG_CURRENT_DESKTOP`) or, in standalone Wayland compositor sessions, from live IPC sockets in `$XDG_RUNTIME_DIR` (never reading stale cached exports when no supported shell is running):

- **Noctalia v5** (live `$XDG_RUNTIME_DIR/noctalia-${WAYLAND_DISPLAY:-wayland-0}.sock`, or `noctalia-*.sock` excluding `dmenu` helpers when `WAYLAND_DISPLAY` is unset): Automatically stages the embedded GDS template ([gds.json](../../src/appearance/theme/provider/system/noctalia/assets/gds.json)) into `$XDG_CACHE_HOME/gds/theme/noctalia-template.json`, registers `${NOCTALIA_CONFIG_HOME:-$XDG_CONFIG_HOME}/noctalia/gds.toml` when absent (never overwriting symlinks or user-edited configs), triggers an immediate render over Noctalia's IPC socket, and reads `$XDG_CACHE_HOME/gds/theme/noctalia.json` (`schema_version = 1`, active `mode`, all 50 `colors.{dark,light}` Material 3 roles, and optional 22-color `terminal.{dark,light}` palette in `provider/system/noctalia.rs`). Only Noctalia setups with `include.autoload = false` need to include `gds.toml` explicitly.

- **DankMaterialShell (DMS)** (live `DMS_SOCKET` or `$XDG_RUNTIME_DIR/{dms,danklinux}*.sock` when Noctalia is not active): Reads `$XDG_CACHE_HOME/DankMaterialShell/dms-colors.json` (all 50 `colors.light` / `colors.dark` Material 3 roles, active `mode`, plus optional `dank16.color0..color15` terminal ANSI colors). Missing required roles, unpaired contrast roles, or malformed colors return an explicit error rather than silently pretending a fallback succeeded.
- **KDE Plasma** (`XDG_CURRENT_DESKTOP` contains `KDE`): Reads `$XDG_CONFIG_HOME/kdeglobals` (`Colors:Window`, `Colors:View`, `Colors:Selection`, `Colors:Button`, `Colors:Tooltip`) via `provider/system/kde.rs`, preserving Plasma accent tinting. System mode derives light/dark from the active window background; explicit Light/Dark overrides GDS mode defaults without synthesizing a second KDE scheme.
- **GNOME** (`XDG_CURRENT_DESKTOP` contains `GNOME`): Pure in-process Rust reader (`provider/system/gnome.rs`) combining the XDG Settings Portal (`org.freedesktop.appearance` `color-scheme` and `accent-color`), standard Adwaita light/dark baselines, and `$XDG_CONFIG_HOME/gtk-{4.0,3.0}/{settings.ini,gtk.css}` `@define-color` overrides (`gtk-4.0` takes precedence). Requires no Python helper or GTK runtime library.

### Built-in (`Source::Builtin`)

Embedded via `include_str!` from [`src/appearance/theme/provider/builtin/assets/`](../../src/appearance/theme/provider/builtin/assets/), each defining complete opaque `#RRGGBB` `[light]` and `[dark]` tables:

| ID | Direction | Source File |
| --- | --- | --- |
| `bamboo` *(default)* | Fresh natural green | [bamboo.toml](../../src/appearance/theme/provider/builtin/assets/bamboo.toml) |
| `retro` | Classic Win98 gray ramp, navy selection, and teal | [retro.toml](../../src/appearance/theme/provider/builtin/assets/retro.toml) |
| `neon` | Synthwave violet, magenta, and cyan | [neon.toml](../../src/appearance/theme/provider/builtin/assets/neon.toml) |
| `macaron` | Cream, rose, mint, and lavender pastels | [macaron.toml](../../src/appearance/theme/provider/builtin/assets/macaron.toml) |
| `high_contrast` | Pure black/white surfaces, crisp borders, and high-visibility blue/yellow accents | [high_contrast.toml](../../src/appearance/theme/provider/builtin/assets/high_contrast.toml) |
| `aero` | Ice-blue surfaces and sky-blue accents | [aero.toml](../../src/appearance/theme/provider/builtin/assets/aero.toml) |

### Custom User Themes (`Source::Custom`)

Users can place TOML theme files in `$XDG_CONFIG_HOME/gds/themes/` (subdirectories supported), shared across Files and Agents. Copy `bamboo.toml` as a starter template and include `[metadata]` plus complete `[light]` and `[dark]` sections:

```toml
[metadata]
name = "Forest"
schema_version = 1
author = "Theme Author"

[metadata.i18n]
zh-CN = "森林"
en-US = "Forest"

# Complete [light] and [dark] #RRGGBB palettes follow...
```

- `name` is required; `i18n` maps case-insensitive BCP-47 tags (`en-US`, `zh-CN`) to localized menu labels, falling back directly to `name`.
- Files stores `source = { custom = "personal/my-theme.toml" }` in `settings.toml`; Agents stores `{"theme_source": {"custom": "personal/my-theme.toml"}}` in `settings.json`. Reopen Appearance settings to discover newly added files; edits to the active file hot-reload automatically.

## 2. Semantic Palette, Material 3 Alignment & Terminal Mapping

Components read semantic and Material 3 aligned roles from `--gds-*` / `--zui-*` Stylo custom properties or `Gds::current().appearance().theme().colors()`:

| GDS Role | Noctalia / DMS (Material 3) Input | KDE Plasma Input | Contract |
| --- | --- | --- | --- |
| `content` (`--gds-content`) | `surface` | `Colors:View` | Primary workspace canvas |
| `sidebar` (`--gds-sidebar`) | `surface_container_low` | `Colors:Window` | Left column surface |
| `surface` (`--gds-surface`) | `surface_container` | `Colors:Button` / `Window` | Card and chip container surface |
| `panel` / `tooltip` (`--gds-panel`) | `surface_container_high` | `Colors:Button` / `Tooltip` | Opaque dialogs, menus, drag cards, and popovers |
| `control` (`--gds-control`) | `surface_container_lowest` (Light) / `surface_container_highest` (Dark) | `Colors:Button` | Raised neutral buttons (`ButtonVariant::Outline`, Cancel actions) with `--gds-control-border`, `--gds-control-hover`, and `--gds-control-pressed` |
| `foreground` / `muted_foreground` | `on_surface` / `on_surface_variant` | `ForegroundNormal` / `ForegroundInactive` | Paired opaque foregrounds (never dim whole controls via alpha) |
| `primary` / `primary_foreground` | `primary` / `on_primary` | `Colors:Selection` | Primary emphasis, focus ring, and progress |
| `selection` / `selection_foreground` | `primary_container` / `on_primary_container` | `Colors:Selection` | Contrast-safe selection pair (`--zui-color-accent`) |
| `secondary_container` / `tertiary_container` | `secondary_container` / `tertiary_container` + `on_*` | Derived from palette | Tonal secondary controls (`--zui-color-secondary`), chips, and complementary accents |
| `border` / `outline` | `outline_variant` / `outline` | `Colors:Window.ForegroundInactive` / derived | Hairline dividers vs outlined input borders |
| `error` / `error_container` / `warning` / `success` | `error`, `error_container` + GDS status defaults | GDS status defaults | Solid and container semantic feedback pairs |

- **Full Material 3 token alignment**: All 50 Material 3 keys exported by DMS and Noctalia (`background`, `on_background`, `on_surface`, `surface_dim`, `surface_bright`, `surface_container_lowest`..`highest`, `surface_variant`, `on_surface_variant`, `surface_tint`, `inverse_surface`, `inverse_on_surface`, `inverse_primary`, `outline`, `outline_variant`, `shadow`, `scrim`, `source_color`, `primary*`/`on_primary*`, `secondary*`/`on_secondary*`, `tertiary*`/`on_tertiary*`, `error*`/`on_error*`) are exposed 1:1 on `Palette` (snake_case) and `:root` (`--gds-<kebab-case>`, with M3 `surface` mapped to `--gds-content`). Non-Material providers and `schema_version = 1` custom TOML themes automatically derive consistent defaults from their own resolved colors while allowing optional explicit TOML overrides for any Material 3 or `control` role.
- **Neutral interaction & toggle states**: Ghost chrome buttons (`ButtonVariant::Ghost`) use translucent `8%` / `12%` foreground state layers (`--gds-hover` / `--gds-pressed`), whereas raised neutral buttons (`ButtonVariant::Outline`) composite those `8%` / `12%` layers opaquely onto `--gds-control` (`--gds-control-hover` / `--gds-control-pressed`) with `--gds-control-border` (`border` mixed with `35%` `outline`). Unchecked `ToggleSwitch` tracks and thumbs derive mode-aware high-contrast tokens (`--gds-switch-off` and `--gds-switch-thumb-off`). Marquee and drop targets use `12%` `primary` fill with a `1 px` `primary` border.
- **Terminal palette**: `appearance/theme/terminal.rs` supplies terminal foreground/background, inverse cursor pair, selection pair, and 16 ANSI colors (`terminal.{light,dark}` when exported by Noctalia v5, `dank16` when exported by DMS, or GDS's curated light/dark ANSI table otherwise, keeping indices `16–255` on the standard xterm cube/ramp). Live theme changes update running and starting PTY sessions immediately for rendering and OSC color queries without restarting shells.

## 3. Surface Opacity, Corners & Hot-Reload

- **Surface opacity**: Configured independently for **Main background** and **Sidebar background** (`100%` / `85%` default on floating desktops; `85%` / `50%` on recognized tiling compositors). Applied via `:root` `--gds-content-bg` / `--gds-sidebar-bg` custom properties (`Appearance::install_window_sheet`, reference-counted per window host) so native dialogs, menus, tooltips, and text/icons remain `100%` opaque. Opacity changes update surface alpha without re-parsing theme files.
- **Rounded window corners**: Opt-in per app (`appearance.rounded_corners`, default `false`). Applies a `10 px` radius to Linux client-side frames, content clip, and input hit regions while keeping maximized, fullscreen, server-decorated, and tiled edges square (`0 px` radius and `0 px` border).
- **Live watcher & hot-reload (`theme::HotReload`)**: Watches parent directories and symlink targets for Noctalia and DMS exports, Noctalia IPC socket start/stop, `kdeglobals`, GNOME dconf/Portal signals, GTK 3/4 CSS, and active custom TOML files. Parsing runs off the UI thread; invalid edits report an error and retain the last valid theme snapshot, and stale async loads never overwrite a newer source selection.
