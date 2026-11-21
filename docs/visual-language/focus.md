# Focus Presentation

Keyboard focus across GDS is light, complete, and distinct from selection. Applications share the theme and control rules in `src/appearance` and `src/ui` without inventing per-page focus rings or halos.

## State Separation

- **Selection**: Identifies chosen item(s) via paired `selection.bg` / `selection.fg` and persists when focus moves elsewhere.
- **Hover / Press**: Pointer-only surface feedback; never acts as a keyboard cursor.
- **Keyboard `focus-visible`**: Identifies the active target during `Tab` / `Shift+Tab`, arrow navigation, or `Alt` region shortcuts—remaining clearly visible even on an already selected row.
- **`focus-within`**: Used only for command routing and focus restoration, never to draw extra outlines around ancestor containers.
- **Text inputs**: Always display their focused border and insertion caret upon pointer or keyboard entry so active editing is unmistakable.

## Single Owner, 1 px Thin Contour

GDS disables `zgui-ui`'s external focus ring shadows (`FocusTokens::disable_ring_shadows` / `--zui-focus-ring-shadow: 0 0 #0000`) across initial startup, mode/source switches, and hot-reload updates:

- Rather than disabling focus cues, disabling outer ring shadows removes `3 px` box-shadow halos and instead tints existing `1 px` borders (or paints an inset/edge `1 px` `:focus-visible` contour on borderless controls and filled buttons using a contrast-safe foreground line).
- Shared buttons display this contour only in keyboard modality, while inputs, selects, and switches retain focus cues in both pointer and keyboard modalities.
- Semantic focus ink remains opaque in all palettes, including High Contrast.
- Accessible regions (`Alt+Space`, `Alt+L`, `Alt+V`, `Alt+T`, `Alt+N`) outline **only** the region or active notification cursor itself—once a child control or table owns focus, the enclosing region draws no second frame, and pointer clicks on rows or blank space leave no region outline.

## Scroll Viewport Clipping

Scrolling containers in Files intentionally clip overflow. Because an external halo outside a table's bounds gets clipped on its horizontal edges, the table's `1 px` keyboard contour is painted **inside** its own scroll viewport edge without altering row/column dimensions, scroll offsets, or hit targets.

In Files' data table context, native table selection bindings are disabled so `Tab` / `Shift+Tab` perform standard window focus traversal while arrow keys and `Home` / `End` drive Files' path-keyed selection model.

### Offscreen Rendering Comparison

2× WGPU offscreen renders of the Files browser illustrating why the inset `1 px` contour replaced outer `3 px` halos inside clipped scroll viewports:

| Clipped outer 3 px halo (avoided) | Shared 1 px inset keyboard contour |
| --- | --- |
| ![Previous Kit ring clipped on horizontal edges](assets/focus-previous-kit.png) | ![Complete 1 px table contour for keyboard focus](assets/focus-keyboard.png) |

See also [pointer activation](assets/focus-pointer.png) (row selection highlighted without a region focus frame).
