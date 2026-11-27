//! Sidebar row renderers for tabs, quick-access items, volumes, and standard locations.

use std::path::{Path, PathBuf};

use gds::appearance::tokens::{ICON_BUTTON_SIZE, SIDEBAR_ICON_SIZE, SIDEBAR_ITEM_HEIGHT};
use gds::i18n;
use gds::ui::button::IconButtonProps;
use gds::ui::capacity::CapacityBarProps;
use gds::ui::feedback::tooltip::{HintTooltipProps, wrap_tokens};
use gds::ui::interaction::{
    keytips::{self, HintPlacement},
    region,
};
use gds::ui::navigation::{
    CornerActionProps, CornerButtonProps, NavItemContentProps, NavItemDetailsProps,
    NavItemHeaderProps, NavItemLabelProps, NavItemProps,
};
use gds::ui::{HeldModifier, IconName, IconProps, Shortcut};
use zgui::prelude::*;
use zgui::reactive::RwSignal;
use zgui::view::{Scoped, events, install_stylesheet as install_sheet};
use zgui::vocab::{PointerButton, SharedString};
use zgui::{css, view};
use zgui_ui::SpinnerProps;

use super::super::track_language;
use crate::entities::file::ui::{EntryPresentation, FileIcon};
use crate::entities::location::{Location, LocationTarget, volumes};
use crate::pages::workspace::{
    navigation::{Navigator, QuickAccess},
    operations::DeviceOps,
    panes::{DragHost, MenuHost},
    projection::{
        Rows,
        geometry::{ITEM_GAP, TAB_HEIGHT},
    },
    state::{
        context::{Context, local_memo},
        model::Pane,
        ui_flags::{SidebarRegion, SidebarRow},
    },
};
use crate::shared::ui::actions::{Command, ordinal_shortcut};

const TAB_CLOSE_RESERVE: f32 = ITEM_GAP * 2.0 + ICON_BUTTON_SIZE;

const SIDEBAR_ITEMS_SHEET: &str = "gds-files-chrome-sidebar-items";

const SIDEBAR_ITEMS_CSS: &str = css!(
    r#"
    .gds-files-location-item.gds-files-location-item {
        flex: 0 0 auto;
        flex-shrink: 0;
        height: var(--gds-sidebar-item-height, 36px);
        min-height: var(--gds-sidebar-item-height, 36px);
        max-height: var(--gds-sidebar-item-height, 36px);
        box-sizing: border-box;
    }

    .gds-files-tab-item.gds-files-tab-item {
        flex: 0 0 auto;
        flex-shrink: 0;
        height: 44px;
        min-height: 44px;
        box-sizing: border-box;
    }

    .gds-files-tab-item .gds-nav-item__header,
    .gds-files-tab-item .gds-nav-item__details {
        padding-right: var(--gds-files-tab-reserve, 40px);
    }

    .gds-files-tab-action {
        position: absolute;
        right: 6px;
        top: 6px;
        display: flex;
        align-items: center;
        justify-content: center;
        opacity: 0;
        pointer-events: none;
        z-index: 2;
        transition: opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-tab-action[data-revealed="true"] {
        opacity: 1;
        pointer-events: auto;
    }

    .gds-sidebar[data-collapsed="true"] .gds-files-tab-action,
    .gds-nav-item[data-collapsed="true"] .gds-files-tab-action {
        opacity: 0;
        pointer-events: none;
    }

    .gds-sidebar:not([data-actions-visible="true"]) .gds-files-tab-action {
        opacity: 0;
        pointer-events: none;
        visibility: hidden;
    }

    .gds-files-volume-item.gds-files-volume-item {
        flex: 0 0 auto;
        flex-shrink: 0;
        padding: 6px 0px;
        gap: 0px;
        min-height: var(--gds-sidebar-item-height, 36px);
        box-sizing: border-box;
    }

    .gds-files-volume-col {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        gap: 4px;
    }

    .gds-files-volume-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        width: 100%;
        min-width: 0px;
        height: 20px;
        padding-left: var(--gds-nav-item-pad-left, 8px);
        padding-right: var(--gds-nav-item-pad-right, 8px);
        gap: var(--gds-nav-item-gap, 8px);
        overflow: hidden;
        box-sizing: border-box;
    }

    .gds-files-volume-row > .gds-nav-item__label {
        flex: 1 1 0px;
    }

    .gds-sidebar[data-collapsed="true"] .gds-files-volume-row {
        justify-content: center;
        padding-left: 0px;
        padding-right: 0px;
        gap: 0px;
    }

    .gds-sidebar[data-collapsed="true"] .gds-files-volume-row > .gds-nav-item__label {
        display: none;
    }

    .gds-sidebar:not([data-actions-visible="true"]) .gds-files-volume-spinner {
        display: none;
    }

    .gds-files-volume-details {
        width: 100%;
        min-width: 0px;
        padding: 0px var(--gds-files-volume-details-pad-x, 8px);
        overflow: hidden;
        box-sizing: border-box;
    }

    .gds-files-volume-unmounted-slot {
        display: flex;
        align-items: center;
        width: 100%;
        min-width: 0px;
        height: 16px;
        overflow: hidden;
    }

    .gds-files-nav-icon {
        flex: none;
    }

    .gds-nav-item.gds-nav-item[data-cursor="true"] {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }
    "#
);

pub(super) fn install_stylesheet() {
    install_sheet(SIDEBAR_ITEMS_SHEET, SIDEBAR_ITEMS_CSS);
}

fn quick_access_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| (!name.is_empty()).then(|| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::pages::workspace::shell::chrome) struct TabCycleTargets {
    pub previous: Option<u64>,
    pub next: Option<u64>,
}

pub(in crate::pages::workspace::shell::chrome) fn compute_tab_cycle_targets(
    ids: &[u64],
    active_id: u64,
) -> TabCycleTargets {
    if ids.len() <= 1 {
        return TabCycleTargets::default();
    }
    let Some(pos) = ids.iter().position(|&id| id == active_id) else {
        return TabCycleTargets::default();
    };
    let len = ids.len();
    TabCycleTargets {
        previous: Some(ids[(pos + len - 1) % len]),
        next: Some(ids[(pos + 1) % len]),
    }
}

pub(super) fn sidebar_quick_access_item(ctx: &Context, path: PathBuf) -> AnyView {
    let label = quick_access_label(&path);
    let icon = FileIcon::directory_icon(&path);
    let cursor = ctx.0.sidebar_cursor;
    let region_sig = ctx.0.sidebar_region;
    let quick_access_node = ctx.0.quick_access_node;
    let open_click = ctx.clone();
    let open_aux = ctx.clone();
    let path_click = path.clone();
    let path_aux = path.clone();
    let selected = Signal::derive_local({
        let ctx = ctx.clone();
        let path = path.clone();
        move || {
            ctx.pane() == Pane::Files
                && ctx.read(|state| state.tab().navigation.path() == Some(path.as_path()))
        }
    });
    let candidate = SidebarRow::QuickAccess(path.clone());
    let cursor_match = Signal::derive_local(move || {
        region_sig.get() == Some(SidebarRegion::QuickAccess)
            && region::is_focus_visible(quick_access_node)
            && cursor.with(|row| row.as_ref() == Some(&candidate))
    });
    let tooltip_text = wrap_tokens(&format!("{label}\n{}", path.display()));
    let a11y_label = label.clone();
    let row_label = label.clone();

    let item = AnyView::new(view! {
        NavItem(
            selected = selected,
            class = "gds-files-location-item",
            attr:data-cursor = move || cursor_match.get().then(|| "true".to_owned()),
            style:height = format!("{SIDEBAR_ITEM_HEIGHT}px"),
            a11y:label = a11y_label,
            on:click = move |ev: &mut EventCx<'_, events::Click>| {
                QuickAccess(&open_click).open_quick_access(&path_click, ev.modifiers.primary());
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                if ev.button == Some(PointerButton::Middle) {
                    ev.stop_propagation();
                    QuickAccess(&open_aux).open_quick_access(&path_aux, true);
                }
            }
        ) {
            Icon(name = icon, size = SIDEBAR_ICON_SIZE, class = "gds-files-nav-icon")
            NavItemLabel {{row_label}}
        }
    });

    let tipped = AnyView::new(view! {
        HintTooltip(text = tooltip_text) {{item}}
    });
    let drop_wrapped = DragHost(ctx).file_drag_target(tipped, path.clone());
    let dragged = DragHost(ctx).draggable_quick_access(drop_wrapped, path.clone(), label, icon);
    MenuHost(ctx).sidebar_quick_access_menu(dragged, path)
}

pub(super) fn sidebar_volume(
    ctx: &Context,
    volume: &volumes::Volume,
    snapshot: &volumes::Snapshot,
    active: Signal<Option<volumes::VolumeId>, LocalStorage>,
    compact_bar: Signal<bool, LocalStorage>,
) -> AnyView {
    let icon = if volume.locked {
        IconName::Lock
    } else {
        match volume
            .drive
            .as_deref()
            .and_then(|id| snapshot.drive(id))
            .map(|drive| drive.kind)
        {
            Some(volumes::DeviceKind::Usb) => IconName::Usb,
            Some(volumes::DeviceKind::Optical) => IconName::Disc,
            _ => IconName::HardDrive,
        }
    };

    let action = ctx.clone();
    let device = volume.id.clone();
    let candidate = SidebarRow::Volume(volume.id.clone());
    let cursor = ctx.0.sidebar_cursor;
    let region_sig = ctx.0.sidebar_region;
    let volumes_node = ctx.0.volumes_node;
    let selected = Signal::derive_local({
        let id = volume.id.clone();
        move || active.get().as_ref() == Some(&id)
    });
    let cursor_match = Signal::derive_local(move || {
        region_sig.get() == Some(SidebarRegion::Volumes)
            && region::is_focus_visible(volumes_node)
            && cursor.with(|row| row.as_ref() == Some(&candidate))
    });

    let status = ctx.0.volumes.capacity(&volume.id);
    let volume_busy = ctx.0.volumes.is_busy(&volume.id);
    let mut hint = vec![volume.name.clone(), volume.filesystem.clone()];
    if volume.system_mount {
        hint.push(i18n::text("device-system-volume"));
    }
    let identity = Rows::volume_identity(volume);
    if let Some(root) = Rows(ctx).resolve_path_untracked(&Location::Volume {
        identity,
        relative: std::ffi::OsString::new(),
    }) {
        hint.push(format!(
            "{}: {}",
            i18n::text("volume-default-mount"),
            root.display()
        ));
    }

    let details = if volume.mount_points.is_empty() {
        AnyView::new(view! {
            box(class = "gds-files-volume-unmounted-slot") {
                NavItemDetails {{i18n::text("device-not-mounted")}}
            }
        })
    } else {
        let (summary, fraction, unavailable) = match status {
            Some(volumes::CapacityState::Ready(capacity)) => {
                let available = EntryPresentation::format_size(capacity.available);
                let total = EntryPresentation::format_size(capacity.total);
                (
                    gds::t!("device-capacity", available = available, total = total),
                    Some(capacity.unavailable_fraction()),
                    false,
                )
            }
            Some(volumes::CapacityState::Unavailable(error)) => {
                hint.push(error);
                (i18n::text("device-capacity-unavailable"), None, true)
            }
            _ => (i18n::text("device-capacity-loading"), None, false),
        };
        hint.push(summary.clone());
        AnyView::new(view! {
            CapacityBar(
                label = summary,
                fraction = fraction,
                compact = compact_bar,
                embedded = true,
                unavailable = unavailable,
                selected = selected
            )
        })
    };

    let name = volume.name.clone();
    let a11y_label = volume.name.clone();
    let tooltip_text = wrap_tokens(&hint.join("\n"));

    let item = AnyView::new(view! {
        NavItem(
            selected = selected,
            class = "gds-files-volume-item",
            attr:data-cursor = move || cursor_match.get().then(|| "true".to_owned()),
            a11y:label = a11y_label,
            on:click = move |ev: &mut EventCx<'_, events::Click>| {
                DeviceOps(&action).open_volume(&device, ev.modifiers.primary());
                action.focus_content_after_mount();
            }
        ) {
            column(class = "gds-files-volume-col") {
                row(class = "gds-files-volume-row") {
                    Icon(
                        name = icon,
                        size = SIDEBAR_ICON_SIZE,
                        class = "gds-files-nav-icon"
                    )
                    NavItemLabel {{name}}
                    {volume_busy.then(|| AnyView::new(view! {
                        Spinner(class = "gds-files-volume-spinner")
                    }))}
                }
                box(class = "gds-files-volume-details") {
                    {details}
                }
            }
        }
    });

    let tipped = AnyView::new(view! {
        HintTooltip(text = tooltip_text) {{item}}
    });
    let menu_wrapped = MenuHost(ctx).sidebar_volume_menu(tipped, volume);
    AnyView::new(view! {
        box(class = "gds-files-sidebar-row-wrap") {
            {menu_wrapped}
        }
    })
}

pub(super) fn sidebar_location(
    ctx: &Context,
    target: LocationTarget,
    location: Location,
) -> AnyView {
    let label = Signal::derive_local(move || i18n::text(target.label_id()));
    let icon = target.icon();
    let cursor = ctx.0.sidebar_cursor;
    let region_sig = ctx.0.sidebar_region;
    let locations_node = ctx.0.locations_node;
    let open = ctx.clone();
    let selected = Signal::derive_local({
        let ctx = ctx.clone();
        let location = location.clone();
        move || {
            ctx.read(|state| {
                let navigation = &state.tab().navigation;
                navigation.location() == &location
                    || location
                        .local_path()
                        .is_some_and(|path| navigation.path() == Some(path))
            })
        }
    });
    let cursor_match = Signal::derive_local(move || {
        region_sig.get() == Some(SidebarRegion::Locations)
            && region::is_focus_visible(locations_node)
            && cursor.with(|row| row.as_ref() == Some(&SidebarRow::Location(target)))
    });

    AnyView::new(view! {
        box(class = "gds-files-sidebar-row-wrap") {
            NavItem(
                selected = selected,
                tooltip = label,
                class = "gds-files-location-item",
                attr:data-cursor = move || cursor_match.get().then(|| "true".to_owned()),
                style:height = format!("{SIDEBAR_ITEM_HEIGHT}px"),
                a11y:label = move || SharedString::from(label.get()),
                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                    if ev.modifiers.primary() {
                        Navigator(&open).new_tab(Some(location.clone()));
                    } else {
                        Navigator(&open).open(location.clone());
                        open.focus_content_after_mount();
                    }
                }
            ) {
                Icon(name = icon, size = SIDEBAR_ICON_SIZE, class = "gds-files-nav-icon")
                NavItemLabel {{move || label.get()}}
            }
        }
    })
}

pub(super) fn sidebar_tab(
    ctx: &Context,
    id: u64,
    pinned: bool,
    local_path: Option<PathBuf>,
    is_collapsed_rail: Signal<bool, LocalStorage>,
    actions_visible: Signal<bool, LocalStorage>,
) -> AnyView {
    let fallback_location = local_path
        .as_deref()
        .map_or(Location::Recent, Location::from);
    let tab_meta = local_memo({
        let ctx = ctx.clone();
        move || {
            ctx.read_structure(|state| {
                let ids: Vec<u64> = state.tabs.iter().map(|t| t.id).collect();
                let active_id = state.tab().id;
                let is_files_pane = ctx.pane() == Pane::Files;
                let cycle_targets = if is_files_pane {
                    compute_tab_cycle_targets(&ids, active_id)
                } else {
                    TabCycleTargets::default()
                };
                let (position, location) = state
                    .tabs
                    .iter()
                    .enumerate()
                    .find(|(_, tab)| tab.id == id)
                    .map_or_else(
                        || (0, fallback_location.clone()),
                        |(pos, tab)| (pos, tab.navigation.location().clone()),
                    );
                (position, cycle_targets, location)
            })
        }
    });

    let icon_sig = Signal::derive_local(move || tab_meta.with(|(_, _, loc)| loc.icon()));
    let title = Signal::derive_local({
        let ctx = ctx.clone();
        move || {
            let location = tab_meta.with(|(_, _, loc)| loc.clone());
            sidebar_tab_title(&ctx, &location)
        }
    });
    let detail_path = Signal::derive_local({
        let local_path = local_path.clone();
        move || {
            track_language();
            local_path.as_deref().map_or_else(
                || tab_meta.with(|(_, _, loc)| loc.label()),
                |p| p.display().to_string(),
            )
        }
    });
    let tooltip_text = Signal::derive_local(move || {
        wrap_tokens(&format!("{}\n{}", title.get(), detail_path.get()))
    });

    let select = ctx.clone();
    let action_expanded = ctx.clone();
    let action_collapsed = ctx.clone();
    let middle_close = ctx.clone();
    let hovered = RwSignal::new_local(false);
    let cursor = ctx.0.sidebar_cursor;
    let region_sig = ctx.0.sidebar_region;
    let tabs_node = ctx.0.tabs_node;

    let selected = Signal::derive_local({
        let ctx = ctx.clone();
        move || ctx.read_structure(|state| state.tab().id == id)
    });
    let cursor_match = Signal::derive_local(move || {
        region_sig.get() == Some(SidebarRegion::Tabs)
            && region::is_focus_visible(tabs_node)
            && cursor.with(|row| row.as_ref() == Some(&SidebarRow::Tab(id)))
    });

    let revealing_close = Signal::derive_local(|| {
        keytips::KeytipRegistry::current()
            .is_some_and(|reg| reg.held().get() == Some(HeldModifier::Ctrl))
    });

    let expanded_action_revealed = Signal::derive_local(move || {
        (pinned || revealing_close.get() || hovered.get()) && actions_visible.get()
    });
    let collapsed_action_revealed = Signal::derive_local(move || {
        (pinned || revealing_close.get() || hovered.get()) && is_collapsed_rail.get()
    });

    let action_icon = if pinned {
        IconName::Pin
    } else {
        IconName::Close
    };
    let action_label_key = if pinned {
        "unpin-tab"
    } else {
        "close-this-tab"
    };
    let close_shortcut = (!pinned).then(|| Command::CloseTab.shortcut());
    let expanded_close_shortcut_enabled =
        Signal::derive_local(move || selected.get() && actions_visible.get());
    let collapsed_close_shortcut_enabled =
        Signal::derive_local(move || selected.get() && is_collapsed_rail.get());

    let tab_ref = NodeRef::new();
    let corner_ref = NodeRef::new();
    #[cfg(test)]
    super::super::tests::record_tab_nodes(id, tab_ref, corner_ref);
    if let Some(ref key) = close_shortcut {
        keytips::anchor_when(corner_ref, key.clone(), collapsed_close_shortcut_enabled);
    }

    let on_middle = (!pinned).then(|| {
        UnsyncCallback::new(move |()| {
            let nav = Navigator(&middle_close);
            if let Some(index) = nav.tab_index(id) {
                nav.close_tab(index);
            }
        })
    });

    let no_drag_exp = ctx.0.window.as_ref().map(WindowHandle::no_drag_handler);

    let core_item = AnyView::new(view! {
        NavItem(
            node_ref = tab_ref,
            selected = selected,
            on_middle_click = on_middle,
            class = "gds-files-tab-item",
            attr:data-cursor = move || cursor_match.get().then(|| "true".to_owned()),
            style:height = format!("{TAB_HEIGHT}px"),
            a11y:label = move || SharedString::from(title.get()),
            on:pointer_enter = move |_| hovered.set(true),
            on:pointer_leave = move |_| hovered.set(false),
            on:click = move |_| {
                let nav = Navigator(&select);
                if let Some(index) = nav.tab_index(id) {
                    nav.select_tab(index);
                    select.focus_content_after_mount();
                }
            }
        ) {
            Icon(
                name = icon_sig,
                size = SIDEBAR_ICON_SIZE,
                class = "gds-files-nav-icon"
            )
            NavItemContent {
                NavItemHeader {
                    NavItemLabel {{move || title.get()}}
                }
                NavItemDetails {
                    {move || detail_path.get()}
                }
            }
            box(
                class = "gds-files-tab-action",
                attr:data-revealed = move || expanded_action_revealed.get().then(|| "true".to_owned()),
                a11y:hidden = move || !actions_visible.get(),
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    ev.stop_propagation();
                    if let Some(ref handler) = no_drag_exp {
                        handler(ev);
                    }
                },
                on:click = |ev: &mut EventCx<'_, events::Click>| {
                    ev.stop_propagation();
                }
            ) {
                IconButton(
                    icon = action_icon,
                    label = Signal::derive_local(move || i18n::text(action_label_key)),
                    shortcut = close_shortcut,
                    shortcut_enabled = expanded_close_shortcut_enabled,
                    on:click = move |ev: &mut EventCx<'_, events::Click>| {
                        ev.stop_propagation();
                        trigger_tab_action(&action_expanded, id, pinned);
                    }
                )
            }
            CornerAction(
                revealed = collapsed_action_revealed,
                button = AnyView::new(view! {
                    CornerButton(
                        node_ref = corner_ref,
                        icon = action_icon,
                        label = Signal::derive_local(move || i18n::text(action_label_key)),
                        on:click = move |_| {
                            trigger_tab_action(&action_collapsed, id, pinned);
                        }
                    )
                })
            ) {}
            {{
                let tab_shortcuts_key =
                    local_memo(move || tab_meta.with(|(pos, cycle, _)| (*pos, *cycle)));
                let placement = Signal::derive_local(move || {
                    let compact_now = is_collapsed_rail.get();
                    let show_action =
                        pinned || revealing_close.get() || hovered.get();
                    match (compact_now, show_action) {
                        (true, true) => HintPlacement::BottomLeadingCorner,
                        (true, false) => HintPlacement::Corner,
                        (false, _) => HintPlacement::TrailingBefore(TAB_CLOSE_RESERVE),
                    }
                });
                move || {
                    let (position, cycle_targets) = tab_shortcuts_key.get();
                    stage_tab_shortcuts(tab_ref, id, position, cycle_targets, placement)
                }
            }}
        }
    });

    let tipped = AnyView::new(view! {
        HintTooltip(text = Signal::derive_local(move || {
            if revealing_close.get() {
                String::new()
            } else {
                tooltip_text.get()
            }
        })) {
            {core_item}
        }
    });
    let wrapped = match local_path {
        Some(path) => DragHost(ctx).file_drag_target(tipped, path),
        None => DragHost(ctx).unsupported_file_drop_target(tipped),
    };
    let dragged = DragHost(ctx).draggable_tab(wrapped, id, pinned);
    MenuHost(ctx).tab_menu(dragged, id)
}

fn trigger_tab_action(ctx: &Context, id: u64, pinned: bool) {
    let nav = Navigator(ctx);
    let Some(index) = nav.tab_index(id) else {
        return;
    };
    if pinned {
        nav.toggle_pin(index);
        ctx.focus_content_after_mount();
    } else {
        nav.close_tab(index);
    }
}

fn sidebar_tab_title(ctx: &Context, location: &Location) -> String {
    let rows = Rows(ctx);
    if let Location::Volume { identity, relative } = location
        && relative.is_empty()
        && let Some(volume) = rows.volume_at_location(identity)
    {
        return volume.name;
    }
    rows.location_title(location)
}

fn stage_tab_shortcuts(
    tab_ref: NodeRef,
    id: u64,
    position: usize,
    cycle_targets: TabCycleTargets,
    placement: Signal<HintPlacement, LocalStorage>,
) -> AnyView {
    AnyView::new(Scoped::new(move || {
        let mut shortcuts: Vec<Shortcut> = Vec::with_capacity(3);
        if cycle_targets.previous == Some(id) {
            shortcuts.push(Command::PreviousTab.shortcut());
        }
        if cycle_targets.next == Some(id) {
            shortcuts.push(Command::NextTab.shortcut());
        }
        if let Some(ordinal) = ordinal_shortcut(position) {
            shortcuts.push(ordinal);
        }
        for shortcut in shortcuts {
            keytips::stage_shortcut_dynamic(
                tab_ref,
                shortcut,
                placement,
                Signal::stored_local(true),
            );
        }
        view! { box(a11y:hidden = true, style:display = "none") {} }
    }))
}
