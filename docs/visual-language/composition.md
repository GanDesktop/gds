# Component Composition

Reusable GDS structural components expose anatomical `#[component]` sub-elements composed inside `zgui::view!`, styled through Stylo classes and `--gds-*` custom properties while passing content as ordinary children.

## `ContentPage` (`gds::ui::page::*`)

Standard workspace page skeleton (`24 px` horizontal / `16 px` vertical header padding, `--gds-text-lg` `20 px * scale` semibold title, `18 px` table inset inside `ListSurface` so side margins clear selection on click):

```rust,ignore
view! {
    ContentPage {
        PageHeader {
            PageTitle { {title} }
            PageActions { {actions} }
        }
        {notice.map(|notice| view! { PageNotice { {notice} } })}
        PageContent { {body} }
    }
}
```

- **Blank-area deselection & loading**: Wrap data tables in `PageTableArea` to preserve blank-area deselection and context menus. While loading a directory or remote target, show `zgui_ui::Spinner` with the target address and a cancel action without presenting stale entries under the new heading.
- **Responsive header folding**: When central content width drops below `620 px` (or window width `< 600 px`), inline `PageActions` compress into a single `Ellipsis` (`...`) menu. When window height drops below `520 px` (`HeightTier::Micro`), `PageHeader` folds away completely—moving page actions into a `...` button at the trailing end of the toolbar address bar and relocating `Ctrl+F` directory filtering into the `32 px` footer's left slot (`24 px` compact height).

## `SettingRow` & Form Controls (`gds::ui::forms::*`)

```rust,ignore
view! {
    SettingRow {
        SettingLabel {
            {title}
            SettingDescription { {description} }
        }
        SettingActions {
            SettingReset { {reset} }
            SettingControl { {control} }
        }
    }
}
```

- **Spatial & responsive invariants**: `SettingReset` reserves a fixed `32 × 32 px` slot even when empty so the `260 px` `SettingControl` column never shifts when a field diverges from `Preferences::default()`. On wide displays, settings and reading containers cap inner content width at **`max-width: 880 px`**; below **`600 px`** window width (`WidthTier::Micro`), `SettingRow` wraps `SettingActions` cleanly onto a second right-aligned line beneath `SettingLabel`.
- **First-party form primitives**:
  - **`ToggleSwitch` (`gds::ui::ToggleSwitch`)**: Proportional desktop pill switch (`Md`: `44 × 24 px` track / `18 px` thumb / `20 px` travel; `Sm`: `36 × 20 px` / `14 px` / `16 px`), animating thumb `translateX` and `color-mix` track/border/thumb tokens over `140 ms`.
  - **`InputField` & `TextareaField` (`gds::ui::field::*`)**: Unified `36 px` (`32 px` compact) `--gds-content` + `1 px` `--gds-outline` chrome with optional leading icon and `24 × 24 px` clear button. `Select` controls inside `SettingControl` share this exact `36 px` height and border contract.
  - **`SearchChoiceField` (`gds::ui::field::SearchChoiceField`)**: Accessible `Role::ComboBox` with tokenized substring filtering across labels/values/keywords and a virtualized `--gds-panel` popup (`220–420 px` wide, up to `384 px` tall).

## `DragCard` (`gds::ui::drag::*`)

Compact pointer-anchored drag preview rendered on `OverlayLayer::Popover` (`140–248 px` wide, opaque `--gds-panel` surface):

```rust,ignore
view! {
    DragCard {
        DragCardHeader { {icon} {title} }
        DragCardContent { {items} }
        {(remaining > 0).then(|| view! { DragCardFooter { {more_label} } })}
    }
}
```

Single-item drags render `DragCardContent` (`16 px` category icon + filename, plus an optional `"Copy"` modifier hint in `DragCardFooter`); multi-item drags add total count in `DragCardHeader`, up to three item previews, and `"+N more"` in `DragCardFooter`.

## Navigation, Frames, Splits, Menus & Dialogs

- **`NavItem` (`gds::ui::navigation::*`)**: Composes `NavItemIcon` (`18 px`), `NavItemContent`, `NavItemHeader`, `NavItemLabel` (`--gds-text-sm`), `NavItemDetails` (`--gds-text-xs`), `NavItemActions` / `HoverAction` (`32 × 32 px`, interactive only when fully expanded), `PinnedIcon`, and collapsed-rail `CornerAction` (`16 × 16 px` `CornerButton` with `11 px` icon) inside a `36 px` (`44 px` two-line) target.
- **`Frame` (`gds::ui::Frame`)**: `Frame(sidebar_width = width) { ... }` aligns `Toolbar`, `WorkspaceFrame`, and `Footer` column boundaries with one continuous `1 px` vertical divider; sidebar-free views pass **`without_sidebar = true`**.
- **`VerticalSplit` (`gds::ui::split::VerticalSplit`)**: Wraps a **`9 px` hit-target** (`top: -4 px; height: 9 px`) / `1 px` hairline resize handle with height clamping, `12 px` keyboard stepping, and preference persistence on release. Horizontal splits spanning both columns (such as the terminal dock) paint `--gds-sidebar-bg` on the left segment and `--gds-content-bg` on the right segment to preserve single-pass translucency.
- **`PopupMenu` & `ContextMenuHost` (`gds::ui::menu::*`)**: Button dropdowns cap height at `360 px`, while pointer/keyboard context menus expand up to `calc(100vh - 18px)` to avoid scrolling. Both include `MenuFocusPark` so row `0` is never pre-highlighted upon opening until pointer hover or arrow-key navigation.
- **`ModalDialog`, `ConfirmDialog`, `AboutDialog` & `DialogSize` (`gds::ui::feedback::dialog::*`)**: Mount **every** modal surface through `ModalDialog`, `ConfirmDialog`, or `AboutDialog` rather than raw `zgui_ui::Dialog` + `DialogContent` so `DialogScope` keytip isolation, `Alt+A` / `Enter` and `Alt+C` / `Esc` / scrim dismissal, idempotent settlement, and `DialogAnchor` focus restoration stay uniform across the suite:
  - **Three canonical `DialogSize` presets** (all clamped to `max-width: calc(100vw - 32px)` on `Micro` windows): **`Sm` (`420 px`)** for confirmations, single-input rename/new-folder prompts, and `AboutDialog`; **`Md` (`500 px`, default)** for standard forms (`Columns`, `TagEditor`, `PermissionEditor`, `ConnectServer`, conflict resolution); and **`Lg` (`640 px`)** for composite browsers (`DirectoryPickerDialog`, `HelpDialog`).
  - **`ConfirmDialog` & `DialogActions`**: Supports optional header warning icons, bounded affected-item preview cards (`+N more`), and `DialogFooter { DialogActions(primary_label = ..., primary_icon = IconName::Check, cancel_label = ..., destructive = ..., on_decision = ...) }`.
- **`CapacityBar` (`gds::ui::capacity::CapacityBar`)**: `4 px` storage usage track with `160 ms` width interpolation, `1200 ms` indeterminate loading sweep, and automatic compact remaining-free `%` readout below `120 px`.
- **Direct `zgui_ui` primitives**: Reuse `zgui_ui`'s `Empty`, `Spinner`, `Select`, `NativeSelect`, `Slider`, `ScrollArea`, and `Collapsible` alongside GDS's `ToggleSwitch`, `InputField`, `SearchChoiceField`, `ShortcutBadge`, and clipboard helpers.

## Status Popovers & Remote `DirectoryPicker`

- **Files footer status popovers**: Background tasks, selection, and clipboard summary controls sit in the `32 px` footer (`24 × 24 px` subordinate icon button + `--gds-text-2xs` count pill in compact/multi-pill mode; `24 px`-tall icon + `--gds-text-xs` summary text when single and wide, transitioning over `160 ms`). Clicking opens an upward `320 × 320 px` `VirtualList` popover where row-tail actions cancel a task, deselect an item, or remove a clipboard entry.
- **`DirectoryPickerDialog` (`gds::ui::picker::directory::*`)**: Composes `Picker`, `PickerToolbar`, `PickerLocation`, `PickerContent`, and `PickerStatus` inside a `DialogSize::Lg` (`640 px`) `ModalDialog` for paginated remote directory browsing (`Loader`), supporting `Ctrl+L`, `Ctrl+F`, `Ctrl+H`, `Ctrl+R`, and `Alt+Left/Right/Up` without BAS or local-FS coupling.

## About (`gds::ui::about::{About, AboutDialog}`)

Shared application identity view mounted via `AboutDialog` (`420 × 408 px` `DialogSize::Sm` modal) or as a standalone surface:

```rust,ignore
use gds::ui::about::{AboutDialog, Application};

view! {
    AboutDialog(open = open, application = Application::cargo(env!("CARGO_BIN_NAME")))
}
```

- **Metadata registration**: Reads `package.{version,homepage,authors,license}` and `[package.metadata.gds.about]` (feedback URL, root `LICENSE`, shared credits, and per-binary `applications.<bin-name>` `name` + `icon`). `build.rs` embeds the 9-character Git commit hash as `GDS_BUILD_TAG` (falling back to flake revision or explicit `GDS_BUILD_TAG` env var in `.git`-less sandboxes), appending `-<profile>` (e.g., `-debug`) for non-release builds.
- **Behavior**: Rasterizes `assets/<app>/icon-desktop.svg` at `256 × 256` for crisp `76 × 76 px` HiDPI display (falling back to `icon-solid.svg`), renders the `--gds-text-lg` product name, copyable monospace version and build-tag rows (`--gds-control` group), external Website/Feedback cards, Credits, and embedded Apache-2.0 license subpages (`Esc` / `Alt+Left` pops subpages before dismissing the dialog).
