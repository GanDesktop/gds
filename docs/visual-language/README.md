# GDS Visual Language

GDS applications share a single visual and interaction grammar implemented in `src/appearance/` and `src/ui/`. Each application persists its own user preferences without a suite-wide global config file.

> **Core rule**: Same meaning, same appearance, same interaction. Desktop themes change colors—never the layout, icon vocabulary, or interaction grammar users have already learned.

## 1. Design Principles

- **Native, calm, and content-first**: Favor crisp hierarchy and compact density over marketing whitespace, nested card stacks, or decorative gradients.
- **No Shock (fixed spatial anchors)**: Recurring structural elements keep invariant coordinates across selection, content, and state changes (e.g., `SettingReset` reserves fixed horizontal space when empty; the Files details pane pins a constant `168 px` preview stage and fixed 2-line `44 px` title slot above an independently scrollable metadata body regardless of media presence, filename length, or multi-selection).
- **Icons speak first, backed by clear tooltips**: Common toolbar and row actions use semantic icons with concise hover/focus tooltips (`ShortcutHint`) and accessible names. Keep visible text for user-identifiable data (filenames, paths, errors, selected values).
- **Suite-first components**: Implement shared patterns in `src/ui/` and style them exclusively through semantic tokens rather than app-private color keys or ad-hoc alpha tweaks.

## 2. Three-Layer Contract

| Layer | Suite-Wide Invariant | Per-Application Scope |
| --- | --- | --- |
| **Semantic** | Action names, icon meanings, destructive intent, focus, selection, and disabled states | Domain actions and object capabilities |
| **Component** | `Frame`, `SideBar`, `NavItem`, `ContentPage`, breadcrumbs, menus, `DialogActions`, notifications | View composition and domain content |
| **Theme** | Semantic palette roles, typography scale, surface opacity, and motion policy | Saved theme source, mode, font, and opacity preferences |

## 3. Rhythm, Tokens & Responsive Windows

Shared layout and typography constants live in [`src/appearance/tokens.rs`](../../src/appearance/tokens.rs):

| Token / Element | Default Value | Notes |
| --- | --- | --- |
| Spacing scale | `4, 8, 12, 16, 24 px` | 4 px base grid |
| Base font size | `16 px` (`1 rem`) | User-adjustable `12–24 px`; explicit pixel text scales via `typography().scaled(...)` |
| Primary UI text | `14 px` (`0.875 rem`) | `ui::SMALL` for controls, navigation, and tables |
| Secondary / section text | `12–13 px` | Metadata, paths, and `SideBarSectionTitle` (`12 px`) |
| Action & status icons | `16 px` / `12 px` | `24 px` app identity icon inside `32 × 32 px` main menu button; `18 px` sidebar nav icons |
| Standard hit targets | `32 × 32 px` icon buttons | `36 px` control height (`38 px` `NavItem` rows) |
| Top toolbar / footer | `52 px` / `36 px` | Shared column split across toolbar, workspace, terminal dock, and footer |
| Sidebar width | `222 px` expanded / `58 px` collapsed | Synchronized across sidebar, top toolbar left slot, and terminal tab column |
| Radii & dividers | `6 px` controls, `8 px` panels, `10 px` optional Linux window corners | `1 px` subtle dividers instead of full box borders |
| Surface opacity defaults | Non-tiling: `100%` content / `85%` sidebar; Tiling (`niri`, `sway`, `hyprland`): `85%` / `50%` | Background alpha only; text, icons, popovers, and dialogs stay opaque |

### Window Sizing & Responsive Breakpoints

- **Initial & minimum sizes** ([`src/ui/window.rs`](../../src/ui/window.rs)): Workspaces open at `960 × 640` logical pixels (capped at 85% of the primary display work area) with a `520 × 360` minimum size.
- **Responsive Files layout**: Below ~`1080 px` width, Files switches to a compact layout—collapsing the sidebar, disabling the details panel, hiding back/forward/parent buttons, and folding secondary page actions into a header ellipsis menu while keeping the top-left Application Menu (`Alt+M` / `F10`), page title, and search accessible. Widening restores saved sidebar and details preferences.
- **Application Menu (`ApplicationMenuButton`)**: Shows the `24 px` app icon at rest and swaps to a centered `16 px` `Menu` icon while pointer-hovered (`:hover`). Secondary pages (Settings, Help, Session Management) replace this top-left slot with an `ArrowLeft` back button.
- **Window Controls (`WindowControls`)**: Placed in a fixed right-hand toolbar slot (`ui::window::window_options()`). Hidden automatically in Niri sessions and when server-side decorations are negotiated; minimize/maximize buttons respect compositor capabilities. Between the menu button and sidebar toggle lies a dedicated empty window-drag region (omitted when collapsed below `8 px`).

## 4. State Expression

| State | Visual Expression | Anti-Pattern to Avoid |
| --- | --- | --- |
| **Hover / Press** | `8%` / `12%` foreground tint overlay on current surface | Matching selection intensity or shifting text color |
| **Active tab / location** | Paired `primary_container` / `on_primary_container` | Text-color-only indication or hardcoded white text |
| **Item selection** | Paired selection fill + foreground (independent of focus) | Merging active tab, keyboard cursor, and item selection |
| **Keyboard focus** | Complete `1 px` contour ([Focus Presentation](focus.md)) | Clipped outer 3 px halos or outlining ancestor containers |
| **Disabled** | Reduced emphasis, non-interactive, optional reason tooltip | Illegible contrast or active hover feedback |
| **Destructive action** | Explicit verb in menus; danger fill only on confirmation action | Permanently bright-red everyday trash icons |

Target contrast is at least `4.5:1` for body text and `3:1` for essential UI controls across light and dark wallpapers.

## 5. Semantic Icon Vocabulary

Operational icons come from `zgui-ui-icons`'s Lucide stroke icon set mapped by semantic role (`gds::ui::IconName`, never raw SVG paths in app views):

| Semantic Role | Icon | Usage Contract |
| --- | --- | --- |
| `app.menu` | App icon → `Menu` on hover | Top-left main menu button |
| `navigate.back` / `forward` / `parent` | Left / Right / Up arrow | History traversal and parent container (secondary views also use Left arrow to return) |
| `panel.sidebar` / `panel.details` | Sidebar rect / Info circle | Toggle sidebar (`Ctrl+B`) or single-toggle details panel (`Ctrl+P`) |
| `tab.new` / `tab.close` / `tab.pin` | Plus / `X` / `Pin` | Context creation, close without deleting data, and pin/unpin in trailing slot |
| `dialog.confirm` / `dialog.cancel` | Check (or verb icon) / `X` | `DialogActions` primary (`Alt+A`) and cancel (`Alt+C`) buttons |
| `file.copy` / `cut` / `paste` / `rename` | Copy / Scissors / ClipboardPaste / Pencil | Standard clipboard operations and `F2` rename |
| `file.trash` / `file.restore` / Empty trash | Trash / `Undo2` / `Broom` | Move to trash vs restore vs empty trash (`Broom` distinguishes bulk empty from single-item `Trash`) |
| `terminal.open` / `goto_current_folder` | `>_` prompt / Folder+arrow | Single window-level dock toggle in bottom-right footer (`Ctrl+\``); open current folder in new terminal tab |
| `location.favorite` / `recent` / `network` | Star / Clock / Server | Bookmarks, visit history, and SFTP/SMB servers |
| `table.columns` | Columns + gear | Column visibility and ordering dialog (`Alt+Shift+V`) |

In GDS Agents, ACP input controls map protocol semantics—not translated labels—to `16 px` Lucide icons: `Shield` (approval/permissions), `Workflow` (collaboration/ACP mode), `Sparkles` (AI model), `Lightbulb` (reasoning effort), and `Zap` (fast mode).

## 6. Shared Grid, Surfaces & Notifications

- **Single column grid**: Each window renders its left column (top-left toolbar, sidebar, terminal tabs, bottom-left footer spacer) once with the sidebar material, and its right column once with the content material, separated by a single `Frame` hairline without double-stacked translucent layers.
- **Data lists (`DataListStyle` & `ListSurface`)**: `40 px` rows, `30 px` headers, `6 px` row radius, and single-line middle/end truncation. Clicking blank space inside `ListSurface` clears selection and range anchors. Hidden files render at `65%` opacity; cut or dragged source items render at `35%` opacity (taking the lowest single opacity rather than multiplying). Multi-item drags use the compact `DragCard` preview.
- **Timestamps (`gds::ui::time::Timestamp`)**: Driven by one shared minute-cadence clock per app; supports relative (`Just now`, `Today HH:mm`) and absolute (`YYYY-MM-DD HH:mm`) formats with full ISO/UTC hover tooltips.
- **Notifications (`gds::ui::notifications`)**: Pinned `8 px` below the top toolbar and `12 px` from the right edge using opaque popover material. Messages stack vertically without overlapping, bound long details inside a scrollable region with a copy button, keep errors until dismissed, and support `Alt+N` / `Alt+X` / `Alt+Shift+X` keyboard access.

## Topic Guides

- [Themes & Materials](theme.md) — palette sources, role mapping, custom TOML themes, and hot-reload
- [Shortcuts & Focus Scopes](shortcuts.md) — keybindings, `ShortcutBadge`, modifier keytips, and scope routing
- [Component Composition](composition.md) — `zgui::view!` component anatomy (`ContentPage`, `SettingRow`, `DragCard`, `DialogActions`, `DirectoryPicker`, `About`)
- [Sidebar Anatomy](sidebar.md) — `SideBar` structure, continuous width transitions, and spatial stability
- [Focus Presentation](focus.md) — 1 px keyboard contour and scroll clipping
- [Motion](motion.md) — reduced-motion policy and `140 ms` / `160 ms` transitions
