# Sidebar Anatomy

`gds::ui::sidebar` provides a composable vertical navigation skeleton modeled on [shadcn Sidebar](https://ui.shadcn.com/docs/components/sidebar#composition). The host `Frame` paints the single sidebar surface material; sidebar containers manage layout, scrolling, and width interpolation.

```text
SideBar (default: 222 px expanded / 58 px collapsed)
├─ SideBarHeader          (fixed height, outside scroll viewport)
├─ SideBarContent         (fills remaining height; vertical scroll or split sections)
│  ├─ SideBarSection
│  │  ├─ SideBarSectionHeader → SideBarSectionTitle
│  │  └─ SideBarSectionContent
│  └─ SideBarSection …
└─ SideBarFooter          (fixed height, outside scroll viewport)
```

Compose containers inside `zgui::view!`. For independently scrolling groups (such as Files' draggable Tabs / Volumes split), pass `scrollable = false` on `SideBarContent` and `flex: 1 1 0%; min-height: 0; overflow-y: auto` on each `SideBarSectionContent`.

## Continuous Width Presentation

Workspace hosts derive one animated `Signal<SidebarPresentation, LocalStorage>` and share it across column children:

```rust,ignore
let presentation = use_sidebar_presentation(sidebar_state);
let sidebar_width = Signal::derive_local(move || presentation.get().width());
view! {
    Frame(sidebar_width = sidebar_width) {
        SideBar(presentation = presentation) { /* ... */ }
    }
}
```

Pass the same `SidebarPresentation` signal to headers, `NavItem(presentation = ...)`, footers, dividers, and the Files terminal tab column:

- **Timing**: Uses `gds::appearance::motion::use_transition` at **`160 ms`, `ease-out cubic`** (no spring overshoot). Retargeting mid-flight reverses smoothly from the current width; reduced motion snaps immediately; settled states request zero frames. Static sidebars (e.g., Settings) may pass `state = ...` directly without sampling motion.
- **Single tree**: Keep one element tree, stable IDs, and one set of scroll handles across widths—never swap between separate "expanded" and "rail" trees or animate surface alpha.
- **Hit-target gating**: Trailing row actions are interactive only when fully expanded (`presentation.actions_visible()`). Opacity alone must never hide an active click target.

## Spatial Stability Between Expanded & Collapsed States

Collapsing narrows horizontal width while keeping every item's **vertical center and row height identical** so the pointer always hits the same logical item before and after toggling:

- **Identical vertical rhythm**: Preserve row order, row height (`38 px` standard `NavItem`; `44 px` two-line rows), gaps, section header slots (using `invisible` when hiding text), and split positions. Hiding volume capacity text keeps its vertical slot so subsequent items never jump upward.
- **Shared horizontal axis**: Expanded leading icons align to a uniform leading edge; collapsed icons center along the symmetric `10 px`-inset rail axis shared by the top-left menu button, new-tab button, tabs, locations, and terminal tabs.
- **Overlay badges**: Collapsed close buttons, status dots, and modifier keytips render on an overlay layer so they never push the primary `18 px` icon off-center.
