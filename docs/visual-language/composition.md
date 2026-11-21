# Component Composition

Reusable GDS structural components expose anatomical `#[component]` sub-elements composed inside `zgui::view!`, styled through Stylo classes and `--gds-*` custom properties while passing content as ordinary children.

## `ContentPage` (`gds::ui::page::*`)

Standard workspace page skeleton (`24 px` horizontal / `16 px` vertical header padding, scaled `22 px` semibold title, `18 px` table inset inside `ListSurface` so side margins clear selection on click):

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

Wrap data tables in `PageTableArea` to preserve blank-area deselection and context menus. While loading a directory or remote target, show `zgui_ui::Spinner` with the target address and a cancel action without presenting stale entries under the new heading.

## `SettingRow` (`gds::ui::settings::*`)

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

`SettingReset` reserves fixed horizontal space even when empty so controls never shift when a field diverges from `Preferences::default()`.

## `DragCard` (`gds::ui::drag::*`)

Compact multi-item drag preview anchored below-right of the cursor (single-item drags reuse the item's native icon/thumbnail and label directly):

```rust,ignore
view! {
    DragCard {
        DragCardHeader { {icon} {title} }
        DragCardContent { {items} }
        {(remaining > 0).then(|| view! { DragCardFooter { {more_label} } })}
    }
}
```

Displays total item count in the header, up to three category-icon + name previews, and an overflow count in the footer.

## Navigation, Frames, Splits & Dialogs

- **`NavItem` (`gds::ui::navigation::*`)**: Composes `NavItemContent`, `NavItemHeader`, `NavItemLabel`, `NavItemDetails`, `NavItemActions`, and collapsed-rail `CornerAction` inside a `38 px` target (`18 px` leading icon, `4 px` row spacing, optional pin and hover-revealed close slot).
- **`Frame` (`gds::ui::Frame`)**: `Frame(sidebar_width = width) { ... }` aligns toolbar, workspace, and footer column boundaries; sidebar-free views pass `has_sidebar = false`.
- **`VerticalSplit` (`gds::ui::split::VerticalSplit`)**: `VerticalSplit` wraps a `7 px` hit-target / `1 px` hairline resize handle with height clamping, keyboard stepping, and saved-preference persistence on release.
- **`ModalDialog`, `ConfirmDialog` & `DialogActions` (`gds::ui::feedback::dialog::*`)**: Mount modal surfaces through `ModalDialog` or `ConfirmDialog` rather than raw `zgui_ui::Dialog` + `DialogContent` so `DialogScope` keytip isolation, `Alt+A` / `Alt+C` / `Esc` / scrim dismissal, idempotent decision settlement, and `DialogAnchor` focus restoration stay consistent across the suite. Pair custom modal bodies with `DialogFooter { DialogActions(primary_label = ..., primary_icon = IconName::Check, cancel_label = ..., destructive = ..., on_decision = ...) }`.
- **Direct `zgui_ui` primitives**: Reuse `zgui_ui`'s `Empty`, `Spinner`, `Select`, `NativeSelect`, `Switch`, `Slider`, `ScrollArea`, and `Collapsible` alongside GDS's `ShortcutBadge` and clipboard helpers.

## Status Popovers & Remote `DirectoryPicker`

- **Files footer status popovers**: Background tasks, selection, and clipboard summary controls sit after the item count (`32 × 32 px` icon + badge in compact mode; icon + summary text in expanded mode, transitioning over `160 ms`). Clicking opens an upward `ListItem` popover where row-tail actions cancel a task, deselect an item, or remove an entry from the file clipboard.
- **`DirectoryPicker` (`gds::ui::picker::directory::DirectoryPicker`)**: Composes `Picker`, `PickerToolbar`, `PickerLocation`, `PickerContent`, and `PickerStatus` inside a host modal `Dialog` for paginated remote directory browsing (`Loader`), supporting `Ctrl+L`, `Ctrl+F`, `Ctrl+H`, `Ctrl+R`, and `Alt+Left/Right/Up` without any BAS or local-FS coupling.

## About (`gds::ui::about::About`)

Shared application identity view mounted inside a modal `Dialog` in Files (`AboutDialog`) and reusable as a standalone page or window:

```rust,ignore
use gds::ui::about::{About, Application};

view! {
    About(application = Application::cargo(env!("CARGO_BIN_NAME")))
}
```

- **Metadata registration**: Reads `package.{version,homepage,authors,license}` and `[package.metadata.gds.about]` (feedback URL, root `LICENSE`, shared credits, and per-binary `applications.<bin-name>` `name` + `icon`). `build.rs` embeds the 9-character Git commit hash as `GDS_BUILD_TAG` (falling back to flake revision or explicit `GDS_BUILD_TAG` env var in `.git`-less sandboxes), appending `-<profile>` (e.g., `-debug`) for non-release builds.
- **Behavior**: Displays `assets/<app>/icon-desktop.svg` (falling back to `icon-solid.svg`) and the product name, separate copyable version and build-tag rows, external Website/Feedback links, Credits, and the embedded Apache-2.0 license with `Esc` / `Alt+Left` sub-page navigation.
