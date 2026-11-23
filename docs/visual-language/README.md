# GDS Visual Language

GDS applications share a single visual and interaction grammar implemented in `src/appearance/` and `src/ui/`. Each application persists its own user preferences without a suite-wide global config file.

> **Core rule**: Same meaning, same appearance, same interaction. Desktop themes change colors—never the layout, icon vocabulary, or interaction grammar users have already learned.

## 1. Design Principles

- **Native, calm, and content-first**: Favor crisp hierarchy, single-pass translucent surfaces, and `1 px` hairlines over marketing whitespace, nested card stacks, control drop-shadows, or decorative gradients.
- **No Shock (fixed spatial anchors)**: Recurring structural elements keep invariant coordinates across selection, content, and state changes (`SettingReset` reserves a fixed `32 × 32 px` slot when empty; the Files details pane maintains a constant `280 px` width, `168 px` preview stage, and `44 px` 2-line title slot—scrolling the whole column vertically on short windows rather than mutating preview geometry).
- **Icons speak first, backed by clear tooltips**: Common toolbar and row actions use semantic icons with concise `400 ms` hover/focus tooltips (`ShortcutHint`) and accessible names. Keep visible text for user-identifiable data (filenames, paths, errors, selected values).
- **Suite-first components**: Implement shared patterns in `src/ui/` and style them exclusively through semantic tokens rather than app-private color keys or ad-hoc alpha tweaks.

## 2. Three-Layer Contract

| Layer | Suite-Wide Invariant | Per-Application Scope |
| --- | --- | --- |
| **Semantic** | Action names, icon meanings, destructive intent, focus, selection, and disabled states | Domain actions and object capabilities |
| **Component** | `Frame`, `SideBar`, `NavItem`, `ContentPage`, `ToggleSwitch`, `SearchChoiceField`, `ModalDialog`, `ConfirmDialog`, `AboutDialog`, menus, notifications | View composition and domain content |
| **Theme** | Semantic palette roles, 5-step typography scale, surface opacity, and motion policy | Saved theme source, mode, font, and opacity preferences |

## 3. Typography Scale, Rhythm & Responsive Windows

### Unified 5-Step Typography Scale

Base font size defaults to `16 px` (user-adjustable `12–24 px` in [`src/appearance/typography.rs`](../../src/appearance/typography.rs), driving `--gds-font-scale = size / 16`). Every UI text element—and data-list row/header heights—scales proportionally through **five canonical semantic steps** (ad-hoc pixel sizes such as `9 / 11 / 13 / 18 / 22 / 24 px` are prohibited outside explicit font-preview samples and terminal PTY preferences):

| Token | Base (`1.0×`) | Line Height | Usage Contract |
| --- | --- | --- | --- |
| `--gds-text-2xs` | `10 px` | `12 px` (`1.2`) | Micro badges only: `KeytipBadge`, footer status count pills, compact capacity `%` |
| `--gds-text-xs` | `12 px` | `16 px` | Table column headers, secondary list columns, `SideBarSectionTitle`, nav subtitles, footer status, `Keycap`, `Timestamp` |
| `--gds-text-sm` | `14 px` | `20 px` | **Primary UI body**: buttons, inputs, menus, sidebar labels, file list & grid names, setting titles/descriptions, tooltips |
| `--gds-text-md` | `16 px` | `22 px` | Section & modal headings: `DialogTitle`, details pane 2-line filename title, empty-state titles |
| `--gds-text-lg` | `20 px` | `28 px` | **Top-level page headings**: `ContentPage` (`PageTitle`), `Settings` category headings, `Help` section headings, `About` app name |

`font-variant-numeric: tabular-nums` is enforced on `Timestamp`, `Clock`, slider numeric readouts, and compact capacity percentages.

### Spatial, Control & Material Tokens

Shared layout constants live in [`src/appearance/tokens.rs`](../../src/appearance/tokens.rs):

| Token / Element | Default Value | Notes |
| --- | --- | --- |
| Spacing scale | `4, 6, 8, 10, 12, 16, 18, 24 px` | `4 px` base grid (`18 px` table/footer horizontal inset; `16 × 24 px` page header padding) |
| Brand & operational icons | `24 / 18 / 16 / 12 px` | `24 px` app mark in `ApplicationMenuButton`; `18 px` sidebar & list file icons; `16 px` action icons; `12 px` badges & window controls (`11 px` `CornerButton`) |
| Hit targets & controls | `32 × 32 px` icon buttons | `36 px` buttons/inputs/selects/single-line `NavItem`s (`32 px` compact, `24 px` subordinate footer controls); `44 × 24 px` `ToggleSwitch` (`36 × 20 px` `Sm`); `44 px` two-line `NavItem` rows |
| Top toolbar / footer / dock header | `48 px` / `32 px` / `40 px` | `8 px`-grid `3 : 2` toolbar-to-footer ratio (`32 px` flush toolbar controls + `8 px` vertical insets; `24 px` subordinate footer controls + `4 px` vertical insets) |
| Sidebar width | `224 px` expanded / `56 px` collapsed | `1 : 4` ratio across sidebar, toolbar left slot, terminal tab rail, and footer spacer (`x = 28 px` rail axis; `12 px` stationary menu-button inset) |
| Radii & dividers | `4 / 6 / 8 / 10 / 9999 px` | `4 px` keycaps/crumbs, `6 px` controls & row pills, `8 px` panels/dialogs/menus/tiles, `10 px` optional Linux window corners, `9999 px` switches/chips; `1 px` dividers, zero control shadows |
| Surface opacity defaults | Floating: `100%` content / `85%` sidebar<br>Tiling: `85%` / `50%` | Tiling auto-detected across `niri`, `sway`, `Hyprland`, `river`, `dwl`, `qtile`, `i3`, `bspwm`, `awesome` (or live `NIRI_SOCKET` / `SWAYSOCK` / `HYPRLAND_INSTANCE_SIGNATURE`); text, icons, menus, and dialogs stay `100%` opaque |

### Window Sizing & Orthogonal `3 × 2` Responsive Breakpoints

- **Initial, minimum & reading-width bounds** ([`src/ui/window.rs`](../../src/ui/window.rs)): Workspaces open at **`1240 × 760`** logical pixels (clamped to `85%` of the primary display work area) with a **`460 × 360`** minimum size. Form and reading views (`Settings`, `Help`, `Network` card groups) cap inner content columns at **`max-width: 880 px`** on wide/2K/4K displays, while data tables and grids remain fluid.
- **Orthogonal viewport clamping (`WidthTier × HeightTier`)**: Width (`600 / 1200 px`) and height (`520 px`) evaluate independently to handle both narrow-tall tiling columns and wide-short horizontal splits without overwriting persisted user preferences:

| Axis & Tier | Viewport Range | Layout & Interaction Contract |
| --- | --- | --- |
| **Width: `Wide`** | `W >= 1200 px` | Full three-column workspace: toggleable sidebar (`224 ↔ 56 px`), toolbar `[Sidebar, Back, Forward, Parent, Address]`, and full `280 px` details pane (if details content exceeds window height, the entire details column scrolls vertically without altering preview/title geometry). |
| **Width: `Compact`** | `600 <= W < 1200 px` | Disables and collapses the right details pane; keeps the sidebar user-toggleable and retains toolbar `[Sidebar, Back, Forward, Parent]`; compresses `ContentPage` actions into `...` whenever central content width drops below `620 px`. |
| **Width: `Micro`** | `460 <= W < 600 px` | Disables details, locks the sidebar to the `56 px` icon rail (hiding the sidebar toggle), hides toolbar `[Back, Forward, Parent]`, compresses page actions into `...`, and wraps `SettingRow` controls onto a second line below their labels. |
| **Height: `Normal`** | `H >= 520 px` | Renders `ContentPage` header (`PageTitle` + inline filter + actions/`...`), bounds sidebar `Locations` height so `Tabs` remain visible, and supports resizable split + maximized terminal dock modes. |
| **Height: `Micro`** | `360 <= H < 520 px` | ① Folds away `ContentPage` header, moving page actions into a `...` button at the trailing end of the toolbar address bar (before `WindowControls`); ② Moves directory filter (`Ctrl+F`) into the `32 px` footer's left status slot (idle item count + `24 px` `🔍` trigger expands a `24 px` input in-place over `160 ms` on `Ctrl+F` or click, stays visible while non-empty, and restores item count on `Esc` / clear; falls back to a bottom-left floating bar only when Portal Save mode occupies the footer); ③ Auto-collapses sidebar `Locations` to preserve `Tabs`; ④ Restricts the terminal dock to **Hidden** and **Maximized** states only (no split intermediate state). |

- **Application Identity & Menu (`ApplicationMenuButton`)**: Each app pairs a warm, tactile 3D vector illustration (`assets/<app>/icon-desktop.svg`, `256 × 256`, rasterized for launchers and `About`) with a matching-perspective zero-stroke `currentColor` silhouette (`assets/<app>/icon-solid.svg`, `24 × 24`). `ApplicationMenuButton` shows the `24 px` solid brand mark at rest (including `:focus-visible`) and morphs into a `16 px` `Menu` icon strictly on pointer `:hover` (`100 ms`). Secondary pages (`Settings`, `Help`) replace this top-left slot with an `ArrowLeft` back button.
- **Window Controls (`WindowControls`)**: Placed in the trailing toolbar slot (`ui::window::workspace_options`). Hidden automatically in Niri sessions, on macOS, and when server-side decorations are negotiated; secondary-clicking empty toolbar drag regions opens the compositor's native window menu.

## 4. State Expression

| State | Visual Expression | Anti-Pattern to Avoid |
| --- | --- | --- |
| **Hover / Press** | `8%` / `12%` foreground tint overlay (`--gds-control-hover/pressed` opaque composite on `Outline` buttons) | Matching selection intensity or shifting text color |
| **Active tab / location** | Paired `selection` / `selection_foreground` (`primary_container` / `on_primary_container`) | Text-color-only indication or hardcoded white text |
| **Item selection** | Paired selection fill + foreground (independent of focus) | Merging active tab, keyboard cursor, and item selection |
| **Keyboard focus** | Complete `1 px` contour ([Focus Presentation](focus.md)) | Clipped outer 3 px halos or outlining ancestor containers |
| **Disabled** | Reduced emphasis (`50%` opacity on switches/nav items), non-interactive, optional reason tooltip | Illegible contrast or active hover feedback |
| **Destructive action** | Explicit verb in menus; danger fill only on confirmation action | Permanently bright-red everyday trash icons |

Target contrast is at least `4.5:1` for body text and `3:1` for essential UI controls across light and dark wallpapers.

## 5. Semantic Icon Vocabulary

Operational icons come from GDS's embedded 155-icon `24 × 24` `currentColor` SVG set in `src/ui/icon/assets/` (151 Lucide stroke icons plus 4 geometric window-control glyphs), mapped strictly by semantic role (`gds::ui::IconName`, never raw SVG paths in app views):

| Semantic Role | Icon | Usage Contract |
| --- | --- | --- |
| `app.menu` | `icon-solid.svg` → `Menu` on `:hover` | Top-left main menu button |
| `navigate.back` / `forward` / `parent` | Left / Right / Up arrow | History traversal (right-click opens history jump menu) and parent container |
| `panel.sidebar` / `panel.details` | `PanelLeft` / `Info` | Toggle sidebar (`Ctrl+B`) or single-toggle details panel (`Ctrl+P`) |
| `tab.new` / `tab.close` / `tab.pin` | `Plus` / `X` / `Pin` | Context creation, close without deleting data, and pin/unpin in trailing/corner slot |
| `dialog.confirm` / `dialog.cancel` | `Check` (or verb icon) / `X` | `DialogActions` primary (`Alt+A`) and cancel (`Alt+C`) buttons |
| `file.copy` / `cut` / `paste` / `rename` | `Copy` / `Scissors` / `ClipboardPaste` / `Pencil` | Standard clipboard operations and `F2` modal rename |
| `file.trash` / `file.restore` / Empty trash | `Trash` / `Undo2` / `Broom` | Move to trash vs restore vs bulk empty/clear (`Broom` distinguishes bulk clear from single-item `Trash`) |
| `terminal.open` / `goto_current_folder` | `Terminal` / `FolderInput` | Single window-level dock toggle in bottom-right footer (`Ctrl+\``); spawn fresh tab in active folder |
| `location.favorite` / `recent` / `network` | `Star` / `Clock` / `Network` | Bookmarks, visit history, and network navigation (`Server` on connected server cards) |
| `table.columns` | `Columns3Cog` | Column visibility and ordering dialog (`Alt+Shift+V`) |

In GDS Agents, ACP input controls map protocol semantics—not translated labels—to `16 px` icons: `Shield` (approval/permissions), `Workflow` (collaboration/ACP mode), `Sparkles` (AI model), `Lightbulb` (reasoning effort), and `Zap` (fast mode).

## 6. Shared Grid, Surfaces & Notifications

- **Single column grid**: Each window renders its left column (top-left toolbar, sidebar, terminal tabs, bottom-left footer spacer) once with `--gds-sidebar-bg`, and its right column once with `--gds-content-bg`, separated by a single unbroken `1 px` `Frame` vertical hairline without double-stacked translucent layers.
- **Data lists (`DataListStyle` & `ListSurface`)**: Proportional `40 px * scale` rows (`2 px` vertical inset producing `36 px * scale` `6 px`-radius selection pills), `30 px * scale` headers, `--gds-text-sm` (`14 px * scale`) primary names, and `--gds-text-xs` (`12 px * scale`) headers/metadata. Clicking blank space inside `ListSurface` clears selection and range anchors. Hidden files render at `65%` content opacity; cut or dragged source items render at `35%` opacity with a `12 px` `Scissors` badge on cut items (taking the lowest single opacity rather than multiplying). Dragging one or multiple items renders the compact `DragCard` preview.
- **Timestamps (`gds::ui::time::Timestamp`)**: Driven by one minute-aligned reactive clock per app; supports relative (`Just now`, `< 30 min`, `Today HH:mm`) and absolute (`YYYY-MM-DD HH:mm`) `tabular-nums` formats with full ISO+offset hover tooltips.
- **Notifications (`gds::ui::notifications`)**: Pinned `8 px` below the `48 px` top toolbar (`top: 56 px`, `calc(var(--gds-toolbar-height) + var(--gds-notification-top-gap))`) and `12 px` from the right edge (`360 px` width) using opaque `--gds-panel` material. Messages stack vertically without overlapping, bound details to 4 lines with a full-copy action, keep errors until dismissed, and support `Alt+N` / `Alt+X` / `Alt+Shift+X` keyboard access.

## Topic Guides

- [Themes & Materials](theme.md) — palette sources, Material 3 role mapping, custom TOML themes, and `theme::HotReload`
- [Shortcuts & Focus Scopes](shortcuts.md) — keybindings, `ShortcutBadge`, modifier keytips, and scope routing
- [Component Composition](composition.md) — `zgui::view!` component anatomy (`ContentPage`, `SettingRow`, `ToggleSwitch`, `SearchChoiceField`, `ModalDialog`, `DirectoryPicker`, `About`)
- [Sidebar Anatomy](sidebar.md) — `SideBar` structure, continuous width transitions, and spatial stability
- [Focus Presentation](focus.md) — `1 px` keyboard contour and scroll clipping
- [Motion](motion.md) — reduced-motion policy and `100 ms` / `140 ms` / `160 ms` transitions
