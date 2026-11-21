//! Declarative and model-driven popup / context menus wrapping `zgui-ui` (`shortcuts.md` §3).

use std::cell::Cell;

use zgui::geom::{Css, CssPx, Point};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, StoredValue, UnsyncCallback};
use zgui::view::current_host;
use zgui::vocab::{Key, NamedKey};
use zgui::{component, css, view};
use zgui_ui::menu::MenuContext;
use zgui_ui::overlay::OverlayState;
use zgui_ui::{
    ContextMenuContentProps, ContextMenuPlace, ContextMenuProps, ContextMenuTriggerProps,
    DropdownMenuContentProps, DropdownMenuProps, DropdownMenuTriggerProps, MenuCheckboxItemProps,
    MenuItemProps, MenuLabelProps, MenuSeparatorProps, MenuSubContentProps, MenuSubProps,
    MenuSubTriggerProps,
};
use zgui_ui_primitives::{Binding, Placement, RovingContext};

use crate::ui::icon::{IconName, IconProps};
use crate::ui::{Shortcut, ShortcutBadgeProps};

/// Preferred default width (in CSS px) of a popup or context menu surface.
pub const WIDTH: f32 = 320.0;
/// Minimum width (in CSS px) of a popup or context menu surface (`main:menu.rs`).
pub const MIN_WIDTH: f32 = 220.0;
/// Maximum width (in CSS px) of a popup or context menu surface (`main:menu.rs`).
pub const MAX_WIDTH: f32 = 500.0;
/// Maximum height (in CSS px) of a compact dropdown popup menu surface (`main:menu.rs`).
pub const MAX_HEIGHT: f32 = 360.0;
/// Window edge margin (in CSS px) reserved around context menus (`main:menu.rs`).
pub const EDGE_MARGIN: f32 = 8.0;
/// Surface border thickness (in CSS px) around popup and context menus (`main:menu.rs`).
pub const SURFACE_BORDER: f32 = 1.0;
/// Horizontal gap (in CSS px) between an item label and its trailing [`ShortcutBadge`].
pub const SHORTCUT_GAP: f32 = 24.0;

const SHEET: &str = css!(
    r#"
    .gds-menu-content,
    .gds-menu-content.gds-menu-content {
        position: relative;
        --zui-surface-fill: var(--gds-panel);
        --zui-surface-ink: var(--gds-panel-foreground);
        --zui-surface-border: 1px solid var(--gds-border);
        --zui-surface-radius: 8px;
        background-color: var(--gds-panel);
        color: var(--gds-panel-foreground);
        border: 1px solid var(--gds-border);
        border-radius: 8px;
        min-width: 220px;
        max-width: 500px;
        max-height: min(360px, var(--zui-popper-available-height, 360px));
        overflow-x: hidden;
        overflow-y: auto;
        font-family: var(--gds-font-sans);
        font-size: var(--gds-font-size-small, 14px);
    }

    .gds-menu-focus-park {
        position: absolute;
        top: 0;
        left: 0;
        width: 0;
        height: 0;
        padding: 0;
        margin: 0;
        border: none;
        outline: none;
        opacity: 0;
        pointer-events: none;
    }

    .gds-menu-content.gds-menu-content.gds-menu-content--context {
        max-height: min(calc(100vh - 18px), var(--zui-popper-available-height, calc(100vh - 18px)));
    }

    .gds-menu-content .zui-menu__item,
    .gds-menu-content .zui-menu__label {
        font-size: var(--gds-font-size-small, 14px);
    }

    .gds-menu-content .zui-menu__item:not(.zui-menu__item--destructive):hover,
    .gds-menu-content .zui-menu__item:not(.zui-menu__item--destructive):focus,
    .gds-menu-content .zui-menu__item:not(.zui-menu__item--destructive)[data-state="open"] {
        background-color: var(--gds-hover);
        color: inherit;
    }

    .gds-menu-host {
        display: contents;
    }

    .gds-menu-item-body {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        flex: 1 1 auto;
        width: 100%;
        min-width: 0;
        gap: 24px;
        font-size: var(--gds-font-size-small, 14px);
    }

    .gds-menu-item-row {
        display: inline-flex;
        align-items: center;
        flex: 1 1 auto;
        gap: 8px;
        min-width: 0;
    }

    .gds-menu-item-label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-font-size-small, 14px);
    }

    .gds-menu-item-shortcut {
        display: inline-flex;
        align-items: center;
        flex-shrink: 0;
        margin-left: auto;
    }
    "#
);

/// One entry inside a programmatic [`MenuModel`].
#[derive(Clone)]
pub enum MenuEntry {
    Item {
        label: String,
        icon: Option<IconName>,
        shortcut: Option<Shortcut>,
        disabled: bool,
        destructive: bool,
        on_select: UnsyncCallback<()>,
    },
    Checkbox {
        label: String,
        checked: bool,
        disabled: bool,
        on_toggle: UnsyncCallback<bool>,
    },
    Submenu {
        label: String,
        icon: Option<IconName>,
        entries: Vec<MenuEntry>,
    },
    Label(String),
    Separator,
}

/// Ordered list of menu entries used by dynamic popups and context menus.
#[derive(Clone, Default)]
pub struct MenuModel {
    pub entries: Vec<MenuEntry>,
}

impl MenuModel {
    /// Creates an empty [`MenuModel`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one entry to the menu.
    pub fn push(&mut self, entry: MenuEntry) {
        self.entries.push(entry);
    }

    /// Appends an action item to the menu.
    pub fn item(
        &mut self,
        label: impl Into<String>,
        icon: Option<IconName>,
        shortcut: Option<Shortcut>,
        disabled: bool,
        destructive: bool,
        on_select: UnsyncCallback<()>,
    ) {
        self.push(MenuEntry::Item {
            label: label.into(),
            icon,
            shortcut,
            disabled,
            destructive,
            on_select,
        });
    }

    /// Appends a checkable toggle item to the menu.
    pub fn checkbox(
        &mut self,
        label: impl Into<String>,
        checked: bool,
        disabled: bool,
        on_toggle: UnsyncCallback<bool>,
    ) {
        self.push(MenuEntry::Checkbox {
            label: label.into(),
            checked,
            disabled,
            on_toggle,
        });
    }

    /// Appends a section heading label to the menu.
    pub fn label(&mut self, title: impl Into<String>) {
        self.push(MenuEntry::Label(title.into()));
    }

    /// Appends a visual separator line to the menu.
    pub fn separator(&mut self) {
        self.push(MenuEntry::Separator);
    }
}

/// Compatibility helper wrapping menu state for `bin/files` context popup anchors.
#[must_use]
pub fn popup_in(model: MenuModel) -> MenuModel {
    model
}

/// Opens the enclosing [`ContextMenuHost`]'s menu at window CSS position `position`.
///
/// Returns `true` when invoked inside a [`ContextMenuHost`] subtree.
pub fn popup_at(position: Point<CssPx, Css>) -> bool {
    let Some(place) = ContextMenuPlace::current() else {
        return false;
    };
    place.open_at(position);
    true
}

fn has_leading_column(entries: &[MenuEntry]) -> bool {
    entries.iter().any(|entry| {
        matches!(
            entry,
            MenuEntry::Checkbox { .. }
                | MenuEntry::Item { icon: Some(_), .. }
                | MenuEntry::Submenu { icon: Some(_), .. }
        )
    })
}

/// Zero-size neutral focus target placed at the top of every menu surface so
/// `FocusTrapOptions::MODAL` enters the menu without highlighting the first item until the user
/// navigates via keyboard or hovers an item with the pointer.
#[component]
pub fn MenuFocusPark() -> impl IntoView {
    install_menu_stylesheet();
    let park_ref = NodeRef::new();
    let group = RovingContext::current();
    let surface_ref = OverlayState::current()
        .or_else(|| MenuContext::current().map(|menu| menu.root()))
        .map(|state| state.content());

    if current_host().is_some() {
        let entered = Cell::new(false);
        let enter_watch = RenderEffect::new(move |_| {
            if park_ref.get().is_some() && !entered.get() {
                entered.set(true);
                park_ref.focus();
            }
        });
        on_cleanup_local(move || drop(enter_watch));
    }

    if let (Some(host), Some(surface)) = (current_host(), surface_ref) {
        let host_focus = host.focused();
        let focus_watch = RenderEffect::new(move |_| {
            if let Some(holder) = host_focus.get()
                && surface.get_untracked() == Some(holder)
            {
                park_ref.focus();
            }
        });
        on_cleanup_local(move || drop(focus_watch));
    }

    view! {
        box(
            node_ref = park_ref,
            class = "gds-menu-focus-park",
            a11y:hidden = true,
            tabindex = move || {
                if group.is_some_and(|g| g.active().is_some()) {
                    Focus::Programmatic
                } else {
                    Focus::Sequential
                }
            },
            on:key_down = move |ev: &mut EventCx<'_, zgui::view::events::KeyDown>| {
                let mods = ev.modifiers.held();
                if mods.control() || mods.alt() || mods.meta() {
                    return;
                }
                let Some(group) = group else {
                    return;
                };
                let moved = match &ev.key {
                    Key::Named(NamedKey::ArrowDown | NamedKey::Home) => group.go_to_end(false),
                    Key::Named(NamedKey::ArrowUp | NamedKey::End) => group.go_to_end(true),
                    _ => false,
                };
                if moved {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            }
        )
    }
}

/// Renders a slice of [`MenuEntry`] values into `zgui-ui` menu items with `22px` trailing
/// [`ShortcutBadge`] keycaps (`24px` gap, `14px` text) and AT-SPI shortcut metadata.
#[must_use]
pub fn render_entries(entries: Vec<MenuEntry>) -> Vec<AnyView> {
    install_menu_stylesheet();
    let align_column = has_leading_column(&entries);
    let mut views = Vec::with_capacity(entries.len() + 1);
    views.push(AnyView::new(view! { MenuFocusPark() }));
    views.extend(entries.into_iter().map(|entry| match entry {
        MenuEntry::Item {
            label,
            icon,
            shortcut,
            disabled,
            destructive,
            on_select,
        } => {
            let inset = align_column && icon.is_none();
            let mut item_attrs = Attrs::new();
            if let Some(ref key) = shortcut {
                let hint = zgui::vocab::SharedString::from(key.label());
                item_attrs =
                    item_attrs.a11y_from(A11yBinding::unspecified().keyboard_shortcut(hint));
            }
            AnyView::new(view! {
                MenuItem(
                    disabled = disabled,
                    destructive = destructive,
                    inset = inset,
                    on_select = on_select,
                    {..item_attrs}
                ) {
                    row(class = "gds-menu-item-body") {
                        row(class = "gds-menu-item-row") {
                            {icon.map(|name| view! { Icon(name = name) }.into_view())}
                            text(class = "gds-menu-item-label") {{label}}
                        }
                        {shortcut.map(|key| {
                            view! {
                                box(class = "gds-menu-item-shortcut", a11y:hidden = true) {
                                    ShortcutBadge(shortcut = key)
                                }
                            }
                            .into_view()
                        })}
                    }
                }
            })
        }
        MenuEntry::Checkbox {
            label,
            checked,
            disabled,
            on_toggle,
        } => AnyView::new(view! {
            MenuCheckboxItem(
                default_checked = checked,
                disabled = disabled,
                on_change = on_toggle
            ) {
                {label}
            }
        }),
        MenuEntry::Submenu {
            label,
            icon,
            entries,
        } => {
            let inset = align_column && icon.is_none();
            let sub_entries = StoredValue::new_local(entries);
            AnyView::new(view! {
                MenuSub {
                    MenuSubTrigger(inset = inset) {
                        row(class = "gds-menu-item-row") {
                            {icon.map(|name| view! { Icon(name = name) }.into_view())}
                            text(class = "gds-menu-item-label") {{label}}
                        }
                    }
                    MenuSubContent(class = "gds-menu-content gds-menu-content--context") {
                        {sub_entries.with_value(|list| render_entries(list.clone()))}
                    }
                }
            })
        }
        MenuEntry::Label(title) => {
            AnyView::new(view! { MenuLabel(inset = align_column) {{title}} })
        }
        MenuEntry::Separator => AnyView::new(view! { MenuSeparator() }),
    }));
    views
}

pub(crate) fn install_menu_stylesheet() {
    if current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-menu", SHEET);
    }
}

/// Anchored dropdown popup driven by a reactive [`MenuModel`].
#[component]
pub fn PopupMenu(
    #[prop(into, optional)] open: Binding<bool>,
    #[prop(into)] model: Signal<MenuModel, LocalStorage>,
    #[prop(default = Placement::BOTTOM)] placement: Placement,
    trigger: Children,
) -> impl IntoView {
    install_menu_stylesheet();
    view! {
        DropdownMenu(open = open) {
            DropdownMenuTrigger {{trigger.into_view_once()}}
            DropdownMenuContent(class = "gds-menu-content", placement = placement) {
                {render_entries(model.get().entries)}
            }
        }
    }
}

fn keyboard_context_anchor(host_ref: &NodeRef) -> Point<CssPx, Css> {
    if let Some(host) = current_host()
        && let Some(focused) = host.focused().get_untracked()
        && let Some(bounds) = host.window_box(focused)
    {
        let scale = host_ref.scale().max(0.01);
        return Point::new(
            CssPx((bounds.origin.x.0 + bounds.size.width.0 * 0.25) / scale),
            CssPx((bounds.origin.y.0 + bounds.size.height.0) / scale),
        );
    }

    if let Some(bounds) = host_ref.window_bounds() {
        let scale = host_ref.scale().max(0.01);
        return Point::new(
            CssPx((bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale),
            CssPx((bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale),
        );
    }

    Point::new(CssPx(0.0), CssPx(0.0))
}

#[component]
fn KeyboardContextOpener(children: Children) -> impl IntoView {
    let host_ref = NodeRef::new();
    let place = ContextMenuPlace::current();

    view! {
        box(
            node_ref = host_ref,
            class = "gds-menu-host",
            on:key_down = move |ev: &mut EventCx<'_, zgui::view::events::KeyDown>| {
                if ev.repeat {
                    return;
                }
                let mods = ev.modifiers.held();
                let is_menu_key = ev.key == Key::Named(NamedKey::ContextMenu) && mods.is_empty();
                let is_shift_f10 =
                    ev.key == Key::Named(NamedKey::F10) && mods == zgui::vocab::Modifiers::SHIFT;
                if (is_menu_key || is_shift_f10)
                    && let Some(place) = place
                {
                    let anchor = keyboard_context_anchor(&host_ref);
                    place.open_at(anchor);
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            }
        ) {
            {children.into_view_once()}
        }
    }
}

/// Region wrapper providing a pointer/keyboard-anchored context menu from a builder closure.
#[component]
pub fn ContextMenuHost(
    #[prop(into, optional)] open: Binding<bool>,
    model: std::rc::Rc<dyn Fn() -> MenuModel>,
    children: Children,
) -> impl IntoView {
    install_menu_stylesheet();
    let build = model;
    view! {
        ContextMenu(open = open) {
            ContextMenuTrigger {
                KeyboardContextOpener {
                    {children.into_view_once()}
                }
            }
            ContextMenuContent(class = "gds-menu-content gds-menu-content--context") {
                {render_entries(build().entries)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;
    use std::sync::Arc;

    use zgui::app::Fonts;
    use zgui::atlas::{MemorySink, TextureSink};
    use zgui::bits::DamageSet;
    use zgui::geom::{CssPx, DevicePx, Point, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{Mounted, RwSignal, UnsyncCallback, flush, install};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::scene::Scene;
    use zgui::view;
    use zgui::view::current_host;
    use zgui::vocab::{
        KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction,
        PointerButton, PointerEvent, Role, Timestamp,
    };
    use zgui_platform_headless::{Harness, OffscreenSurface};

    use super::{
        ContextMenuHostProps, EDGE_MARGIN, MAX_HEIGHT, MAX_WIDTH, MIN_WIDTH, MenuEntry, MenuModel,
        PopupMenuProps, SHEET, SHORTCUT_GAP, SURFACE_BORDER, has_leading_column, popup_in,
        render_entries,
    };
    use crate::app::Gds;
    use crate::service::task_manager::Scope;
    use crate::ui::Shortcut;
    use crate::ui::icon::IconName;

    #[derive(Default)]
    struct NoopRenderer {
        atlas: MemorySink,
        target: Option<RenderTarget>,
    }

    impl Renderer for NoopRenderer {
        fn capabilities(&self) -> RenderCapabilities {
            RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.target
        }

        fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
            TextureHandle(0)
        }

        fn release_external(&mut self, _: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::default()
        }

        fn draw(&mut self, _: &Scene, _: &DamageSet) -> FrameOutcome {
            FrameOutcome::Presented(FrameStats::default())
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            &mut self.atlas
        }
    }

    fn noop_renderer(
        _: &Arc<dyn Surface>,
        target: RenderTarget,
    ) -> Result<Box<dyn Renderer>, AppError> {
        let mut renderer = NoopRenderer::default();
        renderer.configure(target);
        Ok(Box::new(renderer))
    }

    fn live_menu_heights(surface: &OffscreenSurface) -> Vec<f64> {
        let mut nodes = HashMap::new();
        let mut root = None;
        for update in surface.a11y_log() {
            if let Some(tree) = update.tree {
                root = Some(tree.root);
            }
            for (id, node) in update.nodes {
                nodes.insert(id, node);
            }
        }
        let Some(root_id) = root else {
            return Vec::new();
        };
        let mut stack = vec![root_id];
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(node) = nodes.get(&id) else {
                continue;
            };
            if node.role() == Role::Menu
                && let Some(rect) = node.bounds()
            {
                out.push(rect.y1 - rect.y0);
            }
            for &child in node.children().iter().rev() {
                stack.push(child);
            }
        }
        out
    }

    #[test]
    fn menu_dimensions_and_entry_rendering_match_spec() {
        assert_eq!(MIN_WIDTH, 220.0);
        assert_eq!(MAX_WIDTH, 500.0);
        assert_eq!(MAX_HEIGHT, 360.0);
        assert_eq!(2.0 * (EDGE_MARGIN + SURFACE_BORDER), 18.0);
        assert_eq!(SHORTCUT_GAP, 24.0);
        assert!(
            SHEET.contains("max-height: min(360px, var(--zui-popper-available-height, 360px));"),
            "compact dropdown menus must stay capped at 360px"
        );
        assert!(
            SHEET.contains(
                ".gds-menu-content.gds-menu-content.gds-menu-content--context {\n        max-height: min(calc(100vh - 18px), var(--zui-popper-available-height, calc(100vh - 18px)));"
            ),
            "context menus must expand up to available window/popper height (100vh - 18px)"
        );

        install().ok();
        let scope = Mounted::new();
        scope.with(|| {
            let mut plain = MenuModel::new();
            plain.item(
                "Rename",
                None,
                Shortcut::parse("F2"),
                false,
                false,
                UnsyncCallback::new(|()| {}),
            );
            assert!(!has_leading_column(&plain.entries));

            let mut mixed = popup_in(MenuModel::new());
            mixed.item(
                "New tab",
                Some(IconName::Plus),
                Shortcut::parse("Ctrl+N"),
                false,
                false,
                UnsyncCallback::new(|()| {}),
            );
            mixed.push(MenuEntry::Separator);
            mixed.item(
                "Paste",
                None,
                Shortcut::parse("Ctrl+V"),
                false,
                false,
                UnsyncCallback::new(|()| {}),
            );
            assert!(has_leading_column(&mixed.entries));
            let rendered = render_entries(mixed.entries);
            assert_eq!(rendered.len(), 4);
        });
        scope.unmount();
    }

    #[test]
    fn context_menu_expands_beyond_compact_dropdown_cap_at_large_font_sizes() {
        let popup_open_slot: Rc<RefCell<Option<RwSignal<bool, LocalStorage>>>> =
            Rc::new(RefCell::new(None));
        let slot_view = Rc::clone(&popup_open_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            let offscreen = harness
                .platform()
                .offscreens()
                .first()
                .cloned()
                .expect("offscreen surface");
            offscreen.set_a11y_listener(true);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1024.0),
                DevicePx(800.0),
            )));
            harness.settle(12);

            // 1. Right-click at (120, 48) to open a 30-item ContextMenuHost (~920px uncapped at 16px font):
            // must expand well past 360px/440px AND cap cleanly to Popper/window available height (<= 782px).
            let pos = Point::new(CssPx(120.0), CssPx(48.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            let ctx_heights = live_menu_heights(&offscreen);
            assert!(
                ctx_heights
                    .iter()
                    .any(|h| *h > 700.0 && *h <= 800.0 - 18.0 + 0.5),
                "expected 30-item ContextMenuHost bounded by available height (> 700px and <= 782px), got {ctx_heights:?}"
            );

            // Dismiss context menu and open 20-item compact PopupMenu: must stay capped at <= 360px.
            let dismiss_pos = Point::new(CssPx(900.0), CssPx(700.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(dismiss_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(dismiss_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(12);

            let popup_open = popup_open_slot.borrow().expect("popup signal");
            popup_open.set(true);
            flush();
            harness.settle(16);

            let popup_heights = live_menu_heights(&offscreen);
            assert!(
                popup_heights.iter().any(|h| (*h - 360.0).abs() <= 1.5),
                "expected 20-item compact PopupMenu capped at 360px, got {popup_heights:?}"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1024.0, 800.0)
            .run_on(driver, move || {
                let app = Gds::install(Scope::default());
                app.appearance().motion().set_enabled(false);
                let typo = crate::appearance::typography::Preferences {
                    size: 16.0,
                    ..Default::default()
                };
                app.appearance()
                    .typography()
                    .apply(&typo)
                    .expect("16px typography");
                let _ = app.appearance().install_window_sheet();

                let popup_open = RwSignal::new_local(false);
                *slot_view.borrow_mut() = Some(popup_open);

                let build_ctx = Rc::new(|| {
                    let mut m = MenuModel::new();
                    for idx in 1..=30 {
                        m.item(
                            format!("Context action {idx}"),
                            Some(IconName::Folder),
                            Shortcut::parse("Ctrl+N"),
                            false,
                            false,
                            UnsyncCallback::new(|()| {}),
                        );
                        if idx == 5 || idx == 11 {
                            m.separator();
                        }
                    }
                    m
                });

                let mut popup_model = MenuModel::new();
                for idx in 1..=20 {
                    popup_model.item(
                        format!("Dropdown option {idx}"),
                        Some(IconName::Folder),
                        Shortcut::parse("Ctrl+N"),
                        false,
                        false,
                        UnsyncCallback::new(|()| {}),
                    );
                }
                let popup_sig = Signal::stored_local(popup_model);

                view! {
                    column(style:width = "100%", style:height = "100%") {
                        row(style:height = "40px") {
                            PopupMenu(
                                open = popup_open,
                                model = popup_sig,
                                trigger = || AnyView::new(view! { text { "Open dropdown" } })
                            )
                        }
                        ContextMenuHost(model = build_ctx) {
                            box(style:width = "600px", style:height = "500px") {
                                text { "Target area" }
                            }
                        }
                    }
                }
            })
            .expect("menu height harness");
    }

    fn live_menu_item_rects(surface: &OffscreenSurface) -> Vec<(f64, f64, f64, f64)> {
        let mut nodes = HashMap::new();
        let mut root = None;
        for update in surface.a11y_log() {
            if let Some(tree) = update.tree {
                root = Some(tree.root);
            }
            for (id, node) in update.nodes {
                nodes.insert(id, node);
            }
        }
        let Some(root_id) = root else {
            return Vec::new();
        };
        let mut stack = vec![root_id];
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(node) = nodes.get(&id) else {
                continue;
            };
            if node.role() == Role::MenuItem
                && let Some(rect) = node.bounds()
            {
                out.push((rect.x0, rect.y0, rect.x1, rect.y1));
            }
            for &child in node.children().iter().rev() {
                stack.push(child);
            }
        }
        out
    }

    fn send_named_key(harness: &mut Harness<Box<dyn AppHandler>>, key: NamedKey, code: KeyCode) {
        let event = KeyEvent::named(key, PhysicalKey::Code(code));
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: event.clone(),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event,
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(8);
    }

    #[test]
    fn menu_opens_without_highlighting_first_item_until_keyboard_or_hover() {
        let selected: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
        let selected_view = Rc::clone(&selected);
        let focus_slot: Rc<RefCell<Option<Signal<Option<NodeId>, LocalStorage>>>> =
            Rc::new(RefCell::new(None));
        let focus_slot_view = Rc::clone(&focus_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            let offscreen = harness
                .platform()
                .offscreens()
                .first()
                .cloned()
                .expect("offscreen surface");
            offscreen.set_a11y_listener(true);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(800.0),
                DevicePx(600.0),
            )));
            harness.settle(12);

            let host_focus = focus_slot.borrow().expect("host focus signal");

            // 1. Right-click near bottom of window (menu flips upward above pointer, just like
            // right-clicking a bottom row in Files). Focus must enter `MenuFocusPark` — NOT item 0!
            let open_pos = Point::new(CssPx(160.0), CssPx(540.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(open_pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(open_pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            let items = live_menu_item_rects(&offscreen);
            assert_eq!(items.len(), 3, "expected 3 menu items");
            let initial_focus = host_focus.get_untracked().expect("menu park focused");

            // Pressing Enter while no item is highlighted must NOT activate item 0 ("Open").
            send_named_key(&mut harness, NamedKey::Enter, KeyCode::Enter);
            assert_eq!(
                selected.get(),
                None,
                "opening context menu must not pre-highlight/focus item 0"
            );

            // 2. First ArrowDown highlights item 0 ("Open"); second ArrowDown moves to item 1.
            send_named_key(&mut harness, NamedKey::ArrowDown, KeyCode::ArrowDown);
            let item0_focus = host_focus.get_untracked().expect("item 0 focused");
            assert_ne!(
                item0_focus, initial_focus,
                "ArrowDown must move focus from neutral park onto item 0"
            );
            send_named_key(&mut harness, NamedKey::ArrowDown, KeyCode::ArrowDown);
            let item1_focus = host_focus.get_untracked().expect("item 1 focused");
            assert_ne!(
                item1_focus, item0_focus,
                "second ArrowDown must advance from item 0 to item 1"
            );

            // 3. Close with Escape, reopen near top (160, 80) so menu opens downward from (160, 80),
            // hover item 2 ("Copy"), then slide pointer onto separator between item 1 and item 2:
            // highlight clears back to `MenuFocusPark`, AND ArrowDown from there still works!
            send_named_key(&mut harness, NamedKey::Escape, KeyCode::Escape);
            let top_pos = Point::new(CssPx(160.0), CssPx(80.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(top_pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(top_pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            let items = live_menu_item_rects(&offscreen);
            assert_eq!(items.len(), 3);
            let park_top = host_focus.get_untracked().expect("menu park focused");
            let h0 = (items[0].3 - items[0].1) as f32;
            let h1 = (items[1].3 - items[1].1) as f32;
            let h2 = (items[2].3 - items[2].1) as f32;
            let item2_center = Point::new(
                CssPx(160.0 + 60.0),
                CssPx(80.0 + 5.0 + h0 + h1 + 9.0 + h2 * 0.5),
            );
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(item2_center),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(8);
            let item2_focus = host_focus.get_untracked().expect("item 2 focused");
            assert_ne!(item2_focus, park_top, "hovering item 2 must focus item 2");

            let separator_pos = Point::new(CssPx(160.0 + 60.0), CssPx(80.0 + 5.0 + h0 + h1 + 4.5));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(separator_pos),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(8);
            assert_eq!(
                host_focus.get_untracked(),
                Some(park_top),
                "leaving item onto separator must return focus to MenuFocusPark"
            );

            // ArrowDown after defocusing onto separator highlights item 0, and Enter selects it!
            send_named_key(&mut harness, NamedKey::ArrowDown, KeyCode::ArrowDown);
            let reopened_item0 = host_focus.get_untracked().expect("item 0 focused");
            assert_ne!(reopened_item0, park_top);
            assert_ne!(reopened_item0, item2_focus);
            send_named_key(&mut harness, NamedKey::Enter, KeyCode::Enter);
            assert_eq!(
                selected.get(),
                Some(0),
                "Enter after ArrowDown must select item 0 ('Open')"
            );

            // 4. Reopen menu and press ArrowUp: must highlight last item (item 2).
            selected.set(None);
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(open_pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(open_pos).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);
            send_named_key(&mut harness, NamedKey::ArrowUp, KeyCode::ArrowUp);
            send_named_key(&mut harness, NamedKey::Enter, KeyCode::Enter);
            assert_eq!(
                selected.get(),
                Some(2),
                "ArrowUp from neutral open state must highlight the last item"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(800.0, 600.0)
            .run_on(driver, move || {
                let app = Gds::install(Scope::default());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                *focus_slot_view.borrow_mut() = current_host().map(|h| h.focused());

                let selected_ctx = Rc::clone(&selected_view);
                let build_ctx = Rc::new(move || {
                    let mut m = MenuModel::new();
                    let s0 = Rc::clone(&selected_ctx);
                    m.item(
                        "Open",
                        Some(IconName::Folder),
                        Shortcut::parse("Enter"),
                        false,
                        false,
                        UnsyncCallback::new(move |()| s0.set(Some(0))),
                    );
                    let s1 = Rc::clone(&selected_ctx);
                    m.item(
                        "Open with...",
                        Some(IconName::Folder),
                        None,
                        false,
                        false,
                        UnsyncCallback::new(move |()| s1.set(Some(1))),
                    );
                    m.separator();
                    let s2 = Rc::clone(&selected_ctx);
                    m.item(
                        "Copy",
                        Some(IconName::Copy),
                        Shortcut::parse("Ctrl+C"),
                        false,
                        false,
                        UnsyncCallback::new(move |()| s2.set(Some(2))),
                    );
                    m
                });

                view! {
                    column(style:width = "100%", style:height = "100%") {
                        ContextMenuHost(model = build_ctx) {
                            box(style:width = "800px", style:height = "600px") {
                                text { "Target area" }
                            }
                        }
                    }
                }
            })
            .expect("menu initial focus harness");
    }
}
