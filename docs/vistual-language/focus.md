# Focus presentation

Focus should be light, complete and stable. Files and Agents share the theme and
control policy; pages must not invent their own focus colors or halos.

## Separate the states

- **Selection** identifies the chosen item(s), using the semantic selection
  background and foreground. Selection survives focus moving elsewhere.
- **Hover/press** provide pointer feedback, not a keyboard cursor.
- **Keyboard focus-visible** identifies the current target after Tab, Shift+Tab,
  arrow navigation or an accessible-region shortcut. Keep it distinct from
  selection, including on a selected row.
- **Focus-within** remains useful for routing commands and returning focus. It
  does not justify outlining every ancestor of a focused control.
- **Text editing** keeps its native focused boundary and insertion caret even
  when entered with the pointer. Do not apply keyboard-only rules to editing.

## One owner, one thin contour

The shared theme installs Kit's `Theme.focus_ring = false`. Despite its name,
this does **not** disable focus indication: Kit tints existing borders or paints
an absolute 1 px line for borderless controls. It removes only the external 3 px
halo. Borders retain their geometry and corner radii; filled buttons use Kit's
contrasting inset foreground line rather than an accent that may match the fill.
This applies to ordinary, primary, destructive and icon buttons, outlined input
and select fields, switches and the controls used in navigation.

Shared buttons show this decoration only in keyboard modality, including when
returning to an already focused button with the pointer. Their actual focus and
activation remain Kit-owned. Inputs, selects and switches retain their native
focused control cues; editing must remain recognizable after a pointer click.

Use the current semantic theme ink. Never make `ring` transparent or reduce all
focus opacity to hide a problem; high-contrast themes must retain visible focus.
The installation policy is reasserted on initial appearance application, mode or
source changes and prepared palette installation used by hot reload.

A keyboard-accessible region only marks **its own** keyboard focus. A focused
child control or table owns its own indication; the enclosing region does not
add a second frame. Shared region/cursor helpers are responsible for this rule.
Pointer activation of a file or blank content must not leave a large region
outline. Region access shortcuts retain the same routing and focus targets:
`Alt+Space`, `Alt+L`, `Alt+V`, `Alt+T` and `Alt+N`. Notification navigation marks
only the current keyboard cursor, not an entire notification column; focusing a
child button uses the child's own indicator. See [shortcut contracts](<shortcuts.md>).

## Clipping and geometry

The Files content chain intentionally clips its scrolling viewport. Kit's old
external table halo extended beyond that viewport and could lose its horizontal
edges; the enclosing focus-within frame compounded the visible vertical edges.
A table's thin contour now belongs on its own edge, inside the scroll viewport.
Do not remove overflow clipping to expose a halo or add padding to make room for
one. Inspect all four painted edges and their clip masks in the actual Files
layout, including after scrolling and region-shortcut focus.

Focus decoration must not alter row/column sizes, control bounds, scroll offsets
or hit targets. Decorative children must remain non-interactive: no mouse-down,
drag, menu or focus handlers. Existing selection, marquee, menu, notification
return-focus and terminal key-routing behavior stays authoritative.

## Verification

- The theme reinstall regression cycles all built-in palettes in light and dark
  through both synchronous and prepared installation, checking semantic focus
  ink and unchanged control geometry.
- Shared-control tests render real buttons, icon buttons, input/select fields and
  a switch. They exercise Tab/Shift+Tab, pointer and keyboard activation, painted
  thin contours and contrast-aware filled-button treatment in ordinary and
  high-contrast themes.
- Files layout and interaction tests must verify the table's painted bounds and
  clipping, not just a standalone component or source-text assertion.

Headless GPUI scene primitives provide rendered geometry/clip evidence, not
Wayland compositor screenshots or human visual acceptance. A headless pass must
not be presented as a completed on-device visual review. Native review should
compare pointer activation and keyboard focus at normal and high DPI, with
light/dark/high-contrast palettes and scrolled content.

### Controlled Files rendering comparison

These are WGPU offscreen renders of the real Files Browser at 2× scale, with
application assets and fixture folders. They are not mockups or Wayland captures.
The comparison restores only Kit's previous outer-ring policy on the repaired
layout; it is **not** a screenshot of the entire previous branch (whose ancestor
focus-within frame also contributed to the original problem).

| Previous Kit outer ring | Shared thin keyboard focus |
| --- | --- |
| ![Previous Kit ring clipped to thick vertical sides](<assets/focus-previous-kit.png>) | ![Complete thin table contour for keyboard focus](<assets/focus-keyboard.png>) |

[Pointer activation preview](<assets/focus-pointer.png>): row selection remains,
without a large content focus outline. The fixture path and timestamps are test
data, not part of the visual contract.

Files owns file selection independently of the table component. Its file-table
context removes the table's native selection bindings: Tab and Shift+Tab use
window focus traversal, while arrows and Home/End reach file navigation.
Popup menus keep their own bindings. Disabling native selection alone must not
leave a focusable table that consumes keys without handling them.
