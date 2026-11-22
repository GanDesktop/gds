//! Files chrome keeps address navigation, folder filtering, and sidebar regions distinct.

use std::cell::Cell;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use gds::appearance::motion::{WORKSPACE_DURATION_MS, use_transition};
use gds::appearance::tokens::{
    COLLAPSED_SIDEBAR_WIDTH, ICON_BUTTON_SIZE, SIDEBAR_ICON_SIZE, SIDEBAR_ITEM_HEIGHT,
    SIDEBAR_PADDING, SIDEBAR_SECTION_PADDING,
};
use gds::i18n;
use gds::ui::button::{ButtonSize, IconButtonProps};
use gds::ui::capacity::{CapacityBarProps, EXTRA_ROW_HEIGHT};
use gds::ui::disclosure::DisclosureIconProps;
use gds::ui::feedback::tooltip::{HintTooltipProps, TOOLTIP_DELAY, TooltipTextProps, wrap_tokens};
use gds::ui::field::InputFieldProps;
use gds::ui::frame::WindowDragAreaProps;
use gds::ui::interaction::{
    keytips::{self, HintPlacement},
    region,
};
use gds::ui::menu::{ContextMenuHostProps, MenuModel};
use gds::ui::navigation::{
    CornerActionProps, CornerButtonProps, NavItemContentProps, NavItemDetailsProps,
    NavItemHeaderProps, NavItemLabelProps, NavItemProps,
};
use gds::ui::sidebar::{
    SideBarContentProps, SideBarFooterProps, SideBarProps, SideBarSectionContentProps,
    SideBarSectionHeaderProps, SideBarSectionProps, SideBarSectionTitleProps, SidebarPresentation,
};
use gds::ui::split::{
    DIVIDER_HEIGHT as SPLIT_DIVIDER_HEIGHT, Options as SplitOptions, State as SplitState,
    VerticalSplitProps, constrained_height,
};
use gds::ui::{HeldModifier, IconName, IconProps, Shortcut};
use zgui::geom::{DevicePx, Point};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, UnsyncCallback, on_cleanup_local};
use zgui::view::{Scoped, ScrollBehavior, ScrollTarget, current_host, events, install_stylesheet};
use zgui::vocab::{Key, Modifiers, NamedKey, PointerButton, Role, ScrollDelta, SharedString};
use zgui::{css, view};
use zgui_ui::{InputProps, SpinnerProps, TooltipContentProps, TooltipProps, TooltipTriggerProps};
use zgui_ui_primitives::Binding;

use super::keyboard::{SidebarRegion, SidebarRow};
use super::model::{AddressPresentation, Pane};
use super::ui::Browser;
use super::view::local_memo;
use crate::entities::file::ui::EntryPresentation;
use crate::entities::location::{
    Collection, Location, LocationTarget,
    scheme::{self, TagAddress},
    volumes,
};
use crate::shared::ui::actions::{Command, ordinal_shortcut};

const MIN_TABS_HEIGHT: f32 = 94.0;
const MIN_VOLUMES_HEIGHT: f32 = 108.0;
const SECTION_HEADER_HEIGHT: f32 = 34.0;
const LOCATION_HEADER_HEIGHT: f32 = 28.0;
const GROUP_GAP: f32 = 6.0;
const ROW_GAP: f32 = 4.0;
const ITEM_GAP: f32 = 8.0;
const TAB_HEIGHT: f32 = 44.0;
const TAB_CLOSE_RESERVE: f32 = ITEM_GAP * 2.0 + ICON_BUTTON_SIZE;
const DEFAULT_DEVICE_ROWS: usize = 4;
const HEADING_ICON_SIZE: f32 = 12.0;
const HEADING_PADDING: f32 = 8.0;
const COLLAPSED_ACTION_INSET: f32 =
    (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING - ICON_BUTTON_SIZE) * 0.5;
const COLLAPSED_HEADING_INSET: f32 =
    (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING - HEADING_ICON_SIZE) * 0.5;
const COMPACT_CAPACITY_THRESHOLD: f32 = 120.0;

const CHROME_SHEET: &str = "gds-files-chrome";

pub(super) const CHROME_CSS: &str = css!(
    r#"
    .gds-files-toolbar-leading.gds-files-toolbar-leading {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
    }

    .gds-files-menu-controls.gds-files-menu-controls {
        display: flex;
        align-items: center;
        justify-content: center;
        flex: none;
    }

    .gds-files-toolbar.gds-files-toolbar {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        align-items: center;
        gap: 4px;
        min-width: 0px;
    }

    .gds-files-toolbar-files.gds-files-toolbar-files {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        gap: 4px;
        min-width: 0px;
    }

    .gds-files-nav-group.gds-files-nav-group {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
    }

    .gds-files-address-bar.gds-files-address-bar {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 96px;
        height: 30px;
        margin: 0px 6px;
        border-radius: var(--gds-radius-control, 6px);
        background: transparent;
        background-color: transparent;
        border: 1px solid var(--gds-border);
        box-sizing: border-box;
        overflow: hidden;
        transition: border-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-address-bar.gds-files-address-bar[data-editing="true"] {
        border-color: var(--gds-primary);
    }

    .gds-files-address-bar .zui-field.gds-files-address-input {
        flex: 1 1 0%;
        width: auto;
        min-width: 0px;
        height: 100%;
        padding: 0px 8px;
        border: 0px solid transparent;
        border-radius: 0px;
        background: transparent;
        background-color: transparent;
        color: var(--gds-foreground);
        font-family: var(--gds-font-sans);
        font-size: var(--gds-font-size-small, 14px);
        box-shadow: none;
        outline: none;
    }

    .gds-files-address-bar .zui-field.gds-files-address-input:focus,
    .gds-files-address-bar .zui-field.gds-files-address-input:focus-visible {
        border-color: transparent;
        box-shadow: none;
        outline: none;
    }

    .gds-files-address-loading.gds-files-address-loading {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 0px;
        height: 100%;
        padding: 0px 6px;
    }

    .gds-files-address-loading-text.gds-files-address-loading-text {
        width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--gds-foreground);
        font-size: var(--gds-font-size-small, 14px);
    }

    .gds-files-address-read.gds-files-address-read {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 0px;
        height: 100%;
        padding-right: 1px;
    }

    .zui-button.gds-icon-button.gds-files-address-edit {
        width: 26px;
        height: 26px;
        min-width: 26px;
        min-height: 26px;
        padding: 0px;
        flex-shrink: 0;
    }

    .gds-files-breadcrumbs.gds-files-breadcrumbs {
        flex: 1 1 0%;
        min-width: 0px;
        height: 100%;
        overflow-x: hidden;
        overflow-y: hidden;
    }

    .gds-files-breadcrumbs-track.gds-files-breadcrumbs-track {
        display: flex;
        align-items: center;
        height: 100%;
        gap: 2px;
        padding: 0px 4px;
    }

    .gds-files-crumb.gds-files-crumb {
        display: inline-flex;
        flex: none;
        align-items: center;
        height: 24px;
        padding: 0px 4px;
        border-radius: 4px;
        cursor: pointer;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-font-size-small, 14px);
        user-select: none;
    }

    .gds-files-crumb.gds-files-crumb[data-current="true"] {
        color: var(--gds-foreground);
        font-weight: 500;
    }

    .gds-files-crumb.gds-files-crumb:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-crumb.gds-files-crumb:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-files-crumb.gds-files-crumb:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-crumb-sep {
        flex: none;
        color: color-mix(in srgb, var(--gds-muted-foreground) 70%, transparent);
    }

    .gds-files-filter-wrap.gds-files-filter-wrap {
        display: flex;
        width: 200px;
        min-width: 96px;
        flex-shrink: 1;
    }

    .gds-files-filter-field.gds-files-filter-field {
        width: 100%;
        min-width: 0px;
    }

    .gds-files-sidebar-tabs-section.gds-files-sidebar-tabs-section {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        padding: 6px 0px 0px 0px;
        box-sizing: border-box;
        overflow: hidden;
    }

    .gds-files-sidebar-volumes-section.gds-files-sidebar-volumes-section {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        padding: 0px;
        box-sizing: border-box;
        overflow: hidden;
    }

    .gds-files-sidebar-locations-section.gds-files-sidebar-locations-section {
        display: flex;
        flex-direction: column;
        flex-shrink: 1;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        padding: 0px 0px var(--gds-sidebar-padding, 10px) 0px;
        box-sizing: border-box;
        overflow: hidden;
    }

    .gds-files-sidebar-footer.gds-files-sidebar-footer {
        flex-shrink: 1;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-sidebar-region-host.gds-files-sidebar-region-host {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-sidebar-scroll.gds-files-sidebar-scroll {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        padding: 0px var(--gds-sidebar-padding, 10px);
        box-sizing: border-box;
        overflow-y: auto;
        overflow-x: hidden;
        gap: 4px;
    }

    .gds-files-sidebar-locations.gds-files-sidebar-locations {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        padding: 0px var(--gds-sidebar-padding, 10px);
        box-sizing: border-box;
        overflow-y: auto;
        overflow-x: hidden;
    }

    .gds-files-sidebar-curtain.gds-files-sidebar-curtain {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        flex-shrink: 0;
        width: 100%;
        min-width: calc(var(--gds-collapsed-sidebar-width, 58px) - 2 * var(--gds-sidebar-padding, 10px));
        gap: 4px;
    }

    .gds-files-tabs-header.gds-files-tabs-header {
        height: 34px;
        padding-left: var(--gds-sidebar-padding, 10px);
        padding-right: calc(var(--gds-sidebar-padding, 10px) + var(--gds-files-tabs-header-pad-right, 0px));
        gap: var(--gds-files-tabs-header-gap, 4px);
        box-sizing: border-box;
    }

    .gds-files-sidebar-heading-header.gds-files-sidebar-heading-header {
        padding: 0px var(--gds-sidebar-padding, 10px);
        box-sizing: border-box;
    }

    .gds-files-tabs-title.gds-files-tabs-title {
        padding: 0px var(--gds-files-tabs-title-pad-x, 6px);
    }

    .gds-files-sidebar-empty {
        color: color-mix(
            in srgb,
            var(--gds-muted-foreground) var(--gds-sidebar-label-pct, 100%),
            transparent
        );
    }

    .gds-files-sidebar-heading.gds-files-sidebar-heading {
        display: flex;
        flex: 1 1 0px;
        width: 100%;
        height: 28px;
        min-width: 0px;
        align-items: center;
        padding-left: var(--gds-files-heading-pad-left, 8px);
        padding-right: var(--gds-files-heading-pad-right, 8px);
        gap: var(--gds-files-heading-gap, 6px);
        border-radius: var(--gds-radius-control, 6px);
        cursor: pointer;
        color: var(--gds-muted-foreground);
        overflow: hidden;
        box-sizing: border-box;
        user-select: none;
    }

    .gds-files-sidebar-heading.gds-files-sidebar-heading:hover {
        background-color: var(--gds-sidebar-hover);
        color: var(--gds-sidebar-foreground);
    }

    .gds-files-sidebar-heading.gds-files-sidebar-heading:hover .gds-sidebar__section-title {
        color: color-mix(
            in srgb,
            var(--gds-sidebar-foreground) var(--gds-sidebar-label-pct, 100%),
            transparent
        );
    }

    .gds-files-sidebar-heading.gds-files-sidebar-heading:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-files-sidebar-heading.gds-files-sidebar-heading:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-sidebar-heading .gds-sidebar__section-title {
        padding: 0px;
    }

    .gds-files-sidebar-row-wrap.gds-files-sidebar-row-wrap {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        flex: 0 0 auto;
        flex-shrink: 0;
        width: 100%;
        min-width: calc(var(--gds-collapsed-sidebar-width, 58px) - 2 * var(--gds-sidebar-padding, 10px));
        overflow: hidden;
        box-sizing: border-box;
    }

    .gds-files-sidebar-row-wrap .zui-tooltip__trigger,
    .gds-files-sidebar-row-wrap .zui-context-menu__trigger,
    .gds-files-sidebar-row-wrap .gds-menu-host {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        flex: 0 0 auto;
        flex-shrink: 0;
        width: 100%;
        min-width: 0px;
        overflow: hidden;
    }

    .gds-files-sidebar-heading-header .zui-tooltip__trigger {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        max-width: 100%;
        min-width: 0px;
        overflow: hidden;
    }

    .gds-files-tab-action .zui-tooltip__trigger {
        flex-direction: row;
        flex: none;
        width: auto;
        max-width: none;
        overflow: visible;
    }

    .gds-files-location-item.gds-files-location-item {
        flex: 0 0 auto;
        flex-shrink: 0;
        height: 38px;
        min-height: 38px;
        max-height: 38px;
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
        min-height: 38px;
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

pub(super) fn install_chrome_stylesheets() {
    gds::ui::styles::install_ui_stylesheets();
    if current_host().is_some() {
        install_stylesheet(CHROME_SHEET, CHROME_CSS);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Breadcrumb {
    pub label: String,
    pub location: Location,
    pub current: bool,
    pub separated: bool,
}

impl Breadcrumb {
    pub(super) fn for_location(location: &Location, path: Option<&Path>) -> Vec<Self> {
        if let Some(path) = path {
            let components = path.components().collect::<Vec<_>>();
            let last_visible = components
                .iter()
                .rposition(|part| !matches!(part, Component::CurDir));
            let mut target = PathBuf::new();
            let mut segments = Vec::new();
            for (index, part) in components.iter().enumerate() {
                target.push(part.as_os_str());
                if matches!(part, Component::CurDir) {
                    continue;
                }
                let current = last_visible == Some(index);
                let name = part.as_os_str().to_string_lossy();
                let label = match part {
                    Component::RootDir | Component::Prefix(_) => name.into_owned(),
                    _ if current => name.into_owned(),
                    _ => format!("{name}{}", std::path::MAIN_SEPARATOR),
                };
                segments.push(Self {
                    label,
                    location: Location::from(target.as_path()),
                    current,
                    separated: false,
                });
            }
            if segments.is_empty() {
                segments.push(Self {
                    label: path.display().to_string(),
                    location: Location::from(path),
                    current: true,
                    separated: false,
                });
            }
            return segments;
        }
        if let Location::Tags(address) = location {
            return Self::for_tags(address);
        }
        let label = match location {
            Location::Remote(address) => address.to_string(),
            _ => location.display_address(),
        };
        vec![Self {
            label,
            location: location.clone(),
            current: true,
            separated: false,
        }]
    }

    fn for_tags(address: &TagAddress) -> Vec<Self> {
        let mut chain = vec![address.clone()];
        let mut cursor = address.clone();
        while let Some(parent) = cursor.parent() {
            chain.push(parent.clone());
            cursor = parent;
        }
        chain.reverse();
        let total = chain.len();
        chain
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let loc = Location::Tags(item);
                Self {
                    label: loc.label(),
                    location: loc,
                    current: index + 1 == total,
                    separated: index > 0,
                }
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TabCycleTargets {
    pub previous: Option<u64>,
    pub next: Option<u64>,
}

pub(super) fn compute_tab_cycle_targets(ids: &[u64], active_id: u64) -> TabCycleTargets {
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

pub(super) fn default_volumes_bottom_height(visible_count: usize, mounted_count: usize) -> f32 {
    visible_count.min(DEFAULT_DEVICE_ROWS) as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP)
        + mounted_count.min(DEFAULT_DEVICE_ROWS) as f32 * EXTRA_ROW_HEIGHT
        + SECTION_HEADER_HEIGHT
}

fn pending_destination_label(location: &Location, path: Option<&Path>) -> String {
    match location {
        Location::Remote(address) => address.to_string(),
        Location::Directory(dir) => Path::new(dir).display().to_string(),
        _ => path.map_or_else(|| location.label(), |p| p.display().to_string()),
    }
}

impl Browser {
    pub(super) fn menu_controls(&self) -> AnyView {
        install_chrome_stylesheets();
        let browser = self.clone();
        let no_drag = self.0.window.as_ref().map(WindowHandle::no_drag_handler);
        AnyView::new(view! {
            row(
                class = "gds-files-menu-controls",
                on:pointer_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                    if let Some(ref handler) = no_drag {
                        handler(ev);
                    } else {
                        ev.stop_propagation();
                    }
                }
            ) {
                {{move || {
                    if browser.pane() == Pane::Files {
                        browser.app_menu()
                    } else {
                        let back = browser.clone();
                        AnyView::new(view! {
                            IconButton(
                                icon = IconName::ArrowLeft,
                                label = Signal::derive_local(|| i18n::text("page-return")),
                                shortcut = Some(Command::Cancel.shortcut()),
                                on:click = move |_| back.back_page()
                            )
                        })
                    }
                }}}
            }
        })
    }

    /// Leading `sidebar_width` toolbar cell (`[ApplicationMenuButton / ArrowLeft] + [WindowDragArea]`).
    pub(super) fn toolbar_leading(&self) -> AnyView {
        install_chrome_stylesheets();
        let menu = self.menu_controls();
        AnyView::new(view! {
            row(class = "gds-files-toolbar-leading") {
                {menu}
                WindowDragArea() { box() }
            }
        })
    }

    /// Right-column toolbar controls (`[PanelLeft, Back, Forward, Parent, flex:1 AddressBar]`).
    pub(super) fn toolbar_controls(&self) -> AnyView {
        install_chrome_stylesheets();
        let browser = self.clone();
        let toggle_b = self.clone();
        let compact_b = self.clone();
        let pane_b = self.clone();
        let compact = Signal::derive_local(move || compact_b.compact());
        let pane_sig = local_memo(move || pane_b.pane());
        let show_sidebar_toggle =
            Signal::derive_local(move || pane_sig.get() != Pane::Help && !compact.get());
        let no_drag = self.0.window.as_ref().map(WindowHandle::no_drag_handler);

        AnyView::new(view! {
            row(class = "gds-files-toolbar") {
                Show(when = move || show_sidebar_toggle.get()) {
                    {{
                        let click_b = toggle_b.clone();
                        let no_drag_btn = no_drag;
                        view! {
                            IconButton(
                                icon = IconName::PanelLeft,
                                label = Signal::derive_local(|| i18n::text("sidebar-toggle")),
                                shortcut = Some(Command::Sidebar.shortcut()),
                                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                                    if let Some(ref handler) = no_drag_btn {
                                        handler(ev);
                                    } else {
                                        ev.stop_propagation();
                                    }
                                },
                                on:click = move |_| {
                                    click_b.toggle_sidebar();
                                }
                            )
                        }
                    }}
                }
                {{move || match pane_sig.get() {
                    Pane::Files => {
                        let nav_b = browser.clone();
                        let addr_b = browser.clone();
                        AnyView::new(view! {
                            row(class = "gds-files-toolbar-files") {
                                Show(when = move || !compact.get()) {
                                    {{nav_b.navigation_buttons()}}
                                }
                                {{addr_b.address_bar()}}
                            }
                        })
                    }
                    Pane::Settings => Self::subpage_toolbar_title("settings"),
                    Pane::Help => AnyView::new(view! { box() }),
                }}}
            }
        })
    }

    fn navigation_buttons(&self) -> AnyView {
        let browser = self.clone();
        let state_sig = Signal::derive_local(move || {
            browser.read(|state| {
                let tab = state.tab();
                let history = tab.navigation.history();
                let index = tab.navigation.history_index();
                let backward = (0..index)
                    .rev()
                    .map(|i| (i, history[i].location.clone()))
                    .collect::<Vec<_>>();
                let forward = ((index + 1)..history.len())
                    .map(|i| (i, history[i].location.clone()))
                    .collect::<Vec<_>>();
                let can_up = scheme::parent(tab.navigation.location()).is_some()
                    || tab.navigation.path().and_then(Path::parent).is_some();
                (tab.id, backward, forward, can_up)
            })
        });
        let parent_browser = self.clone();

        AnyView::new(view! {
            row(class = "gds-files-nav-group") {
                {{move || {
                    let (tab, backward, forward, can_up) = state_sig.get();
                    let b_click = parent_browser.clone();
                    view! {
                        row(class = "gds-files-nav-group") {
                            {{parent_browser.history_button(tab, backward, false)}}
                            {{parent_browser.history_button(tab, forward, true)}}
                            IconButton(
                                icon = IconName::ArrowUp,
                                label = Signal::derive_local(|| i18n::text("parent")),
                                disabled = Signal::stored_local(!can_up),
                                shortcut = Some(Command::Parent.shortcut()),
                                on:click = move |_| b_click.parent()
                            )
                        }
                    }
                }}}
            }
        })
    }

    fn history_button(&self, tab: u64, visits: Vec<(usize, Location)>, forward: bool) -> AnyView {
        let disabled = visits.is_empty();
        let icon = if forward {
            IconName::ArrowRight
        } else {
            IconName::ArrowLeft
        };
        let label_key = if forward { "forward" } else { "back" };
        let shortcut = if forward {
            Command::Forward.shortcut()
        } else {
            Command::Back.shortcut()
        };
        let click_browser = self.clone();

        let button = AnyView::new(view! {
            IconButton(
                icon = icon,
                label = Signal::derive_local(move || i18n::text(label_key)),
                disabled = Signal::stored_local(disabled),
                shortcut = Some(shortcut),
                on:click = move |_| click_browser.history(forward)
            )
        });
        if disabled {
            return button;
        }

        let menu_browser = self.clone();
        let model = Rc::new(move || {
            let mut menu = MenuModel::new();
            for &(index, ref location) in &visits {
                let target_location = location.clone();
                let label = menu_browser.resolve_path_untracked(location).map_or_else(
                    || menu_browser.location_title(location),
                    |path| path.display().to_string(),
                );
                let weak = menu_browser.downgrade();
                menu.item(
                    label,
                    Some(location.icon()),
                    None,
                    false,
                    false,
                    UnsyncCallback::new(move |()| {
                        if let Some(browser) = weak.upgrade() {
                            browser.visit_history(tab, index, &target_location);
                            browser.focus_content_after_mount();
                        }
                    }),
                );
            }
            menu
        });

        AnyView::new(view! {
            ContextMenuHost(model = model) {{button}}
        })
    }

    fn address_bar(&self) -> AnyView {
        install_chrome_stylesheets();
        let editing = self.0.address_presentation;
        let is_editing = Signal::derive_local(move || {
            matches!(editing.get(), AddressPresentation::Editing { .. })
        });
        let destination = local_memo({
            let browser = self.clone();
            move || {
                browser.read(|state| {
                    let navigation = &state.tab().navigation;
                    if let Some(pending) = navigation.pending_location() {
                        let resolved = navigation
                            .pending_path()
                            .map(Path::to_owned)
                            .or_else(|| browser.resolve_path(pending));
                        (pending.clone(), resolved, true)
                    } else {
                        (
                            navigation.location().clone(),
                            navigation.path().map(Path::to_owned),
                            false,
                        )
                    }
                })
            }
        });

        let edit_click = self.clone();
        let blur = self.clone();
        let submit = self.clone();
        let keys = self.clone();
        let crumbs_browser = self.clone();
        let edit_btn = self.clone();
        let composing = Rc::new(Cell::new(false));
        let composing_start = Rc::clone(&composing);
        let composing_preedit = Rc::clone(&composing);
        let composing_commit = Rc::clone(&composing);
        let composing_end = Rc::clone(&composing);
        let address_sig = self.0.address;
        let address_node = self.0.address_node;
        let no_drag = self.0.window.as_ref().map(WindowHandle::no_drag_handler);

        AnyView::new(view! {
            row(
                class = "gds-files-address-bar",
                attr:data-editing = move || is_editing.get().then(|| "true".to_owned()),
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    if let Some(ref handler) = no_drag {
                        handler(ev);
                    } else if ev.button == Some(PointerButton::Primary) {
                        ev.stop_propagation();
                    }
                },
                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                    if !matches!(editing.get_untracked(), AddressPresentation::Editing { .. }) {
                        ev.stop_propagation();
                        edit_click.start_address_edit();
                    }
                }
            ) {
                {{move || {
                    if is_editing.get() {
                        let composing_key = Rc::clone(&composing);
                        let submit_key = submit.clone();
                        let cancel_key = keys.clone();
                        let blur_out = blur.clone();
                        let cs = Rc::clone(&composing_start);
                        let cp = Rc::clone(&composing_preedit);
                        let cc = Rc::clone(&composing_commit);
                        let ce = Rc::clone(&composing_end);
                        AnyView::new(Scoped::new(move || {
                            let focus_effect =
                                RenderEffect::new(move |mounted_before: Option<bool>| {
                                    let mounted = address_node.get().is_some();
                                    if mounted && mounted_before != Some(true) {
                                        let current = address_sig.get_untracked();
                                        address_node.set_value(&current);
                                        address_node.focus();
                                        address_node.select_all();
                                    }
                                    mounted
                                });
                            on_cleanup_local(move || drop(focus_effect));
                            view! {
                                Input(
                                    node_ref = address_node,
                                    value = Binding::from(address_sig),
                                    placeholder = i18n::text("path-placeholder"),
                                    label = i18n::text("place-location"),
                                    class = "gds-files-address-input",
                                    on:ime_start = move |_| cs.set(true),
                                    on:ime_preedit = move |ev: &mut EventCx<'_, events::ImePreedit>| cp.set(ev.is_composing()),
                                    on:ime_commit = move |_| cc.set(false),
                                    on:ime_end = move |_| ce.set(false),
                                    on:focus_out = move |_| blur_out.finish_address_edit(),
                                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                        if composing_key.get() {
                                            return;
                                        }
                                        if ev.key == Key::Named(NamedKey::Enter) && ev.modifiers == Modifiers::NONE {
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                            submit_key.open_address();
                                        } else if ev.key == Key::Named(NamedKey::Escape) && ev.modifiers == Modifiers::NONE {
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                            cancel_key.finish_address_edit();
                                            cancel_key.focus_content_after_mount();
                                        }
                                    }
                                )
                            }
                        }))
                    } else {
                        let (location, path, is_pending) = destination.get();
                        let b_click = edit_btn.clone();
                        let body = if is_pending {
                            let label = pending_destination_label(&location, path.as_deref());
                            let a11y_label = label.clone();
                            let edit_pending = edit_btn.clone();
                            let edit_key = edit_btn.clone();
                            AnyView::new(view! {
                                row(class = "gds-files-address-loading") {
                                    box(
                                        class = "gds-files-crumb",
                                        attr:data-current = "true",
                                        tabindex = Focus::Sequential,
                                        a11y:role = Role::Button,
                                        a11y:label = a11y_label,
                                        on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                            ev.stop_propagation();
                                            edit_pending.start_address_edit();
                                        },
                                        on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                            if ev.modifiers == Modifiers::NONE
                                                && (ev.key == Key::Named(NamedKey::Enter)
                                                    || ev.key == Key::Named(NamedKey::Space)
                                                    || ev.key == Key::character(" "))
                                            {
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                                edit_key.start_address_edit();
                                            }
                                        }
                                    ) {
                                        text(class = "gds-files-address-loading-text") {{label}}
                                    }
                                }
                            })
                        } else {
                            crumbs_browser.breadcrumbs(&location, path.as_deref())
                        };
                        AnyView::new(view! {
                            row(class = "gds-files-address-read") {
                                {body}
                                IconButton(
                                    icon = IconName::Pencil,
                                    label = Signal::derive_local(|| i18n::text("path-edit")),
                                    size = ButtonSize::IconSm,
                                    shortcut = Some(Command::EditLocation.shortcut()),
                                    class = "gds-files-address-edit",
                                    on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                        ev.stop_propagation();
                                        b_click.start_address_edit();
                                    }
                                )
                            }
                        })
                    }
                }}}
            }
        })
    }

    fn breadcrumbs(&self, location: &Location, path: Option<&Path>) -> AnyView {
        let browser = self.clone();
        let segments = Breadcrumb::for_location(location, path);
        AnyView::new(Scoped::new(move || {
            let scroller = NodeRef::new();
            let size_obs = scroller.observe_border_size();
            let scroll_fx = RenderEffect::new(move |_| {
                let _ = size_obs.get();
                scroller.scroll_to(
                    ScrollTarget::Offset(Point::new(DevicePx(f32::MAX), DevicePx(0.0))),
                    ScrollBehavior::Instant,
                );
            });
            on_cleanup_local(move || drop(scroll_fx));

            let crumbs = segments
                .into_iter()
                .flat_map(|crumb| {
                    let mut items = Vec::with_capacity(2);
                    if crumb.separated {
                        items.push(AnyView::new(view! {
                            Icon(
                                name = IconName::ChevronRight,
                                size = 12.0,
                                class = "gds-files-crumb-sep",
                                a11y:hidden = true
                            )
                        }));
                    }
                    let click_browser = browser.clone();
                    let key_browser = browser.clone();
                    let target_loc = crumb.location;
                    let key_loc = target_loc.clone();
                    let is_current = crumb.current;
                    let label = crumb.label;
                    let a11y_label = label.clone();
                    let crumb_ref = NodeRef::new();
                    items.push(AnyView::new(view! {
                        box(
                            node_ref = crumb_ref,
                            class = "gds-files-crumb",
                            attr:data-current = is_current.then(|| "true".to_owned()),
                            tabindex = Focus::Sequential,
                            a11y:role = Role::Button,
                            a11y:label = a11y_label,
                            on:focus_in = move |_| {
                                let (Some(port), Some(item)) =
                                    (scroller.window_bounds(), crumb_ref.window_bounds())
                                else {
                                    return;
                                };
                                let port_left = port.origin.x.0;
                                let port_right = port_left + port.size.width.0;
                                let item_left = item.origin.x.0;
                                let item_right = item_left + item.size.width.0;
                                let dx = if item_left < port_left {
                                    item_left - port_left
                                } else if item_right > port_right {
                                    item_right - port_right
                                } else {
                                    0.0
                                };
                                if dx.abs() > 0.5 {
                                    scroller.scroll_to(
                                        ScrollTarget::By(Point::new(DevicePx(dx), DevicePx(0.0))),
                                        ScrollBehavior::Instant,
                                    );
                                }
                            },
                            on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                ev.stop_propagation();
                                if is_current {
                                    click_browser.start_address_edit();
                                } else if ev.modifiers.primary() {
                                    click_browser.new_tab(Some(target_loc.clone()));
                                } else {
                                    click_browser.open(target_loc.clone());
                                    click_browser.focus_content_after_mount();
                                }
                            },
                            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                let is_activate = ev.key == Key::Named(NamedKey::Enter)
                                    || ev.key == Key::Named(NamedKey::Space)
                                    || ev.key == Key::character(" ");
                                if !is_activate {
                                    return;
                                }
                                ev.prevent_default();
                                ev.stop_propagation();
                                if is_current {
                                    key_browser.start_address_edit();
                                } else if ev.modifiers.primary() {
                                    key_browser.new_tab(Some(key_loc.clone()));
                                } else {
                                    key_browser.open(key_loc.clone());
                                    key_browser.focus_content_after_mount();
                                }
                            }
                        ) {
                            text(class = "gds-files-crumb-text") {{label}}
                        }
                    }));
                    items
                })
                .collect::<Vec<_>>();

            let handle = request_frame(move |_| {
                scroller.scroll_to(
                    ScrollTarget::Offset(Point::new(DevicePx(f32::MAX), DevicePx(0.0))),
                    ScrollBehavior::Instant,
                );
            });
            on_cleanup_local(move || drop(handle));

            view! {
                scroll(
                    node_ref = scroller,
                    class = "gds-files-breadcrumbs",
                    on:wheel = move |ev: &mut EventCx<'_, events::Wheel>| {
                        let scale = scroller.scale().max(1.0);
                        let dx = match ev.delta {
                            ScrollDelta::Lines { x, y } => {
                                let lines = if x.abs() > 0.001 { x } else { y };
                                lines * 28.0 * scale
                            }
                            ScrollDelta::Pixels(pixels) => {
                                if pixels.width.0.abs() > 0.001 {
                                    pixels.width.0
                                } else {
                                    pixels.height.0
                                }
                            }
                            _ => 0.0,
                        };
                        if dx.abs() > 0.001 {
                            scroller.scroll_to(
                                ScrollTarget::By(Point::new(DevicePx(dx), DevicePx(0.0))),
                                ScrollBehavior::Instant,
                            );
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    },
                ) {
                    row(class = "gds-files-breadcrumbs-track") {
                        {crumbs}
                    }
                }
            }
        }))
    }

    pub(super) fn start_address_edit(&self) {
        let value = self.read_untracked(|state| {
            let navigation = &state.tab().navigation;
            if let Some(pending) = navigation.pending_location() {
                Self::location_address(pending, navigation.pending_path())
            } else {
                Self::location_address(navigation.location(), navigation.path())
            }
        });
        self.0.address.set(value.clone());
        self.0
            .address_presentation
            .set(AddressPresentation::Editing {
                original: value.clone(),
            });
        let node = self.0.address_node;
        if node.is_bound() {
            node.set_value(&value);
            node.focus();
            node.select_all();
        }
    }

    pub(super) fn finish_address_edit(&self) {
        let AddressPresentation::Editing { original } = self.0.address_presentation.get_untracked()
        else {
            return;
        };
        self.0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        self.0.address.set(original);
    }

    pub(super) fn folder_filter(&self) -> AnyView {
        install_chrome_stylesheets();
        let filter_sig = self.0.filter;
        let filter_node = self.0.filter_node;
        let available_browser = self.clone();
        let change_browser = self.clone();
        let wrap_ref = NodeRef::new();

        let visible = Signal::derive_local(move || available_browser.filter_available());
        keytips::stage_shortcut(
            wrap_ref,
            Command::Filter.shortcut(),
            HintPlacement::Trailing,
            visible,
        );

        AnyView::new(view! {
            Show(when = move || visible.get()) {
                box(node_ref = wrap_ref, class = "gds-files-filter-wrap") {
                    {{
                        let change_browser = change_browser.clone();
                        move || {
                            GdsLang::track();
                            let change_b = change_browser.clone();
                            view! {
                                InputField(
                                    node_ref = filter_node,
                                    value = Binding::from(filter_sig),
                                    placeholder = i18n::text("filter-placeholder"),
                                    label = i18n::text("filter"),
                                    leading_icon = IconName::Search,
                                    clearable = true,
                                    compact = true,
                                    class = "gds-files-filter-field",
                                    on_change = UnsyncCallback::new(move |value: String| {
                                        change_b.filter(value);
                                    })
                                )
                            }
                        }
                    }}
                }
            }
        })
    }

    pub(super) fn filter_available(&self) -> bool {
        self.pane() == Pane::Files
            && self.read(|state| match state.tab().navigation.location() {
                Location::Recent | Location::Favorites | Location::Tags(_) => !self.compact(),
                Location::Directory(_) | Location::Volume { .. } | Location::Remote(_) => true,
                Location::Network | Location::Volumes | Location::Trash => false,
            })
    }

    pub(super) fn filter_available_untracked(&self) -> bool {
        self.pane_untracked() == Pane::Files
            && self.read_untracked(|state| match state.tab().navigation.location() {
                Location::Recent | Location::Favorites | Location::Tags(_) => {
                    !self.compact_untracked()
                }
                Location::Directory(_) | Location::Volume { .. } | Location::Remote(_) => true,
                Location::Network | Location::Volumes | Location::Trash => false,
            })
    }

    pub(super) fn sidebar(&self, _presentation: SidebarPresentation) -> AnyView {
        let browser = self.clone();
        AnyView::new(Scoped::new(move || browser.sidebar_scoped()))
    }

    fn sidebar_scoped(&self) -> AnyView {
        install_chrome_stylesheets();
        let presentation = self.sidebar_presentation();
        let is_collapsed_rail = local_memo(move || presentation.get().is_collapsed());
        let actions_visible = local_memo(move || presentation.get().actions_visible());
        let compact_capacity =
            local_memo(move || presentation.get().width() < COMPACT_CAPACITY_THRESHOLD);
        let browser = self.clone();

        let sections = local_memo({
            let browser = self.clone();
            move || {
                let preferences = browser.0.preferences.state().get();
                (
                    preferences.files.sidebar.show_volumes
                        || browser.0.sidebar_reveal_volumes.get(),
                    preferences.tags.enabled,
                )
            }
        });
        let show_volumes_sig = Signal::derive_local(move || sections.get().0);

        let vol_expanded = Signal::derive_local({
            let b = self.clone();
            move || {
                !b.0.preferences
                    .state()
                    .with(|p| p.files.sidebar.volumes_collapsed)
                    || b.0.sidebar_region.get() == Some(SidebarRegion::Volumes)
            }
        });
        let vol_progress = use_transition(
            Signal::derive_local(move || if vol_expanded.get() { 1.0 } else { 0.0 }),
            WORKSPACE_DURATION_MS,
        );

        let loc_expanded = Signal::derive_local({
            let b = self.clone();
            move || {
                !b.0.preferences
                    .state()
                    .with(|p| p.files.sidebar.locations_collapsed)
                    || b.0.sidebar_region.get() == Some(SidebarRegion::Locations)
            }
        });
        let loc_progress = use_transition(
            Signal::derive_local(move || if loc_expanded.get() { 1.0 } else { 0.0 }),
            WORKSPACE_DURATION_MS,
        );

        let tabs_region_ref = NodeRef::new();
        let volumes_region_ref = NodeRef::new();
        let locations_region_ref = self.0.locations_region_node;
        keytips::anchor(tabs_region_ref, Command::FocusTabs.shortcut());
        keytips::anchor_when(
            volumes_region_ref,
            Command::FocusVolumes.shortcut(),
            show_volumes_sig,
        );
        keytips::anchor(locations_region_ref, Command::FocusLocations.shortcut());

        let workspace_minimum = Signal::derive_local(move || {
            MIN_TABS_HEIGHT
                + if show_volumes_sig.get() {
                    SECTION_HEADER_HEIGHT
                        + (MIN_VOLUMES_HEIGHT + SPLIT_DIVIDER_HEIGHT - SECTION_HEADER_HEIGHT)
                            * vol_progress.get()
                } else {
                    0.0
                }
        });

        let content_ref = self.0.sidebar_content_node;
        let content_size = content_ref.observe_border_size();
        let available_height = Signal::derive_local(move || {
            let scale = content_ref.scale().max(1.0);
            content_size
                .get()
                .map_or(420.0, |sz| (sz.height.0 / scale).max(0.0))
        });

        let default_bottom = Signal::derive_local({
            let b = self.clone();
            move || {
                if !show_volumes_sig.get() {
                    return MIN_VOLUMES_HEIGHT;
                }
                let snapshot = b.0.volumes.snapshot();
                let visible_count = snapshot.volumes.iter().filter(|v| v.visible).count();
                let mounted_count = snapshot
                    .volumes
                    .iter()
                    .filter(|v| v.visible && !v.mount_points.is_empty())
                    .count();
                default_volumes_bottom_height(visible_count, mounted_count).max(MIN_VOLUMES_HEIGHT)
            }
        });

        let split_state = SplitState::new(SplitOptions {
            min: MIN_VOLUMES_HEIGHT,
            max: 2400.0,
            initial: MIN_VOLUMES_HEIGHT * 1.5,
            collapse_below: None,
        });
        let height_sig = self.0.sidebar_tabs_height;
        let sync_split_fx = RenderEffect::new(move |_| {
            let avail = available_height.get();
            let def_bot = default_bottom.get();
            let preferred_tabs = height_sig.get();
            if avail <= 0.0 {
                return;
            }
            let tabs_h = constrained_height(
                preferred_tabs,
                avail,
                MIN_TABS_HEIGHT,
                MIN_VOLUMES_HEIGHT,
                def_bot,
            );
            let usable = (avail - SPLIT_DIVIDER_HEIGHT).max(0.0);
            let target_bottom = (usable - tabs_h).max(MIN_VOLUMES_HEIGHT.min(usable * 0.5));
            if (split_state.width().get_untracked() - target_bottom).abs() > 0.5 {
                split_state.set_width(target_bottom);
            }
        });
        on_cleanup_local(move || drop(sync_split_fx));

        let commit_browser = self.clone();
        let on_split_commit = UnsyncCallback::new(move |bottom_h: f32| {
            let avail = available_height.get_untracked();
            let usable = (avail - SPLIT_DIVIDER_HEIGHT).max(0.0);
            let next_tabs = (usable - bottom_h).max(MIN_TABS_HEIGHT.min(usable * 0.5));
            height_sig.set(Some(next_tabs));
            commit_browser.persist_sidebar_height();
        });

        let tabs_section = {
            let tabs_b = browser.clone();
            let focus_b = browser.clone();
            let out_b = browser.clone();
            let new_tab_b = browser.clone();
            let tabs_node = self.0.tabs_node;
            move || {
                let tabs_b = tabs_b.clone();
                let focus_b = focus_b.clone();
                let out_b = out_b.clone();
                let new_tab_clk = new_tab_b.clone();
                view! {
                    column(
                        node_ref = tabs_region_ref,
                        style:display = "flex",
                        style:flex-direction = "column",
                        style:flex = "1 1 0px",
                        style:width = "100%",
                        style:height = "100%",
                        style:min-width = "0px",
                        style:min-height = "0px"
                    ) {
                    SideBarSection(
                        class = "gds-files-sidebar-tabs-section"
                    ) {
                        SideBarSectionHeader(class = "gds-files-tabs-header") {
                            SideBarSectionTitle(class = "gds-files-tabs-title") {
                                {move || i18n::text("tabs")}
                            }
                            IconButton(
                                icon = IconName::Plus,
                                label = Signal::derive_local(|| i18n::text("new-tab")),
                                shortcut = Some(Command::NewTab.shortcut()),
                                on:click = move |_| {
                                    new_tab_clk.new_tab(None);
                                    new_tab_clk.focus_content_after_mount();
                                }
                            )
                        }
                        SideBarSectionContent(class = "gds-files-sidebar-region-host") {
                            scroll(
                                node_ref = tabs_node,
                                class = "gds-files-sidebar-scroll",
                                a11y:label = move || SharedString::from(i18n::text("tabs")),
                                {..region::surface(tabs_node)},
                                on:focus_in = move |_| {
                                    focus_b.sidebar_focus_changed(SidebarRegion::Tabs, true);
                                },
                                on:focus_out = move |_| {
                                    if !node_contains_focus(tabs_node) {
                                        out_b.sidebar_focus_changed(SidebarRegion::Tabs, false);
                                    }
                                }
                            ) {
                                {{
                                    let rows_b = tabs_b.clone();
                                    let specs_b = tabs_b.clone();
                                    let tab_specs = local_memo(move || {
                                        specs_b.read_structure(|state| {
                                            state
                                                .tabs
                                                .iter()
                                                .map(|tab| {
                                                    (
                                                        tab.id,
                                                        tab.pinned,
                                                        tab.navigation.path().map(Path::to_owned),
                                                    )
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                    });
                                    view! {
                                        for spec in move || tab_specs.get(), key = |spec: &(u64, bool, Option<PathBuf>)| spec.clone() {
                                            {{rows_b.sidebar_tab(spec.0, spec.1, spec.2, is_collapsed_rail, actions_visible)}}
                                        }
                                    }
                                }}
                            }
                        }
                    }
                    }
                }
            }
        };

        let volumes_section = {
            let vol_b = browser.clone();
            let volumes_node = self.0.volumes_node;
            let active_volume = local_memo({
                let b = browser.clone();
                move || {
                    b.read(|state| match state.tab().navigation.location() {
                        Location::Volume { identity, .. } => {
                            b.volume_at_location(identity).map(|volume| volume.id)
                        }
                        _ => None,
                    })
                }
            });
            move || {
                let toggle_b = vol_b.clone();
                let focus_b = vol_b.clone();
                let out_b = vol_b.clone();
                let rows_b = vol_b.clone();
                let head_b = vol_b.clone();
                view! {
                    column(
                        node_ref = volumes_region_ref,
                        style:display = "flex",
                        style:flex-direction = "column",
                        style:flex = "1 1 0%",
                        style:width = "100%",
                        style:height = "100%",
                        style:min-width = "0px",
                        style:min-height = "0px"
                    ) {
                    SideBarSection(
                        class = "gds-files-sidebar-volumes-section"
                    ) {
                        {{head_b.sidebar_heading(
                            "volumes",
                            SECTION_HEADER_HEIGHT,
                            vol_expanded,
                            is_collapsed_rail,
                            move || {
                                let focused = toggle_b.0.sidebar_region.get_untracked()
                                    == Some(SidebarRegion::Volumes)
                                    || node_contains_focus(volumes_node);
                                toggle_b.edit_preferences(|p| {
                                    p.files.sidebar.volumes_collapsed =
                                        !p.files.sidebar.volumes_collapsed || focused;
                                });
                                if focused {
                                    toggle_b.sidebar_focus_changed(SidebarRegion::Volumes, false);
                                    toggle_b.focus_content();
                                }
                            },
                        )}}
                        SideBarSectionContent(
                            class = "gds-files-sidebar-region-host",
                            style:display = move || {
                                (vol_progress.get() <= 1e-4).then(|| "none".to_owned())
                            }
                        ) {
                            scroll(
                                node_ref = volumes_node,
                                class = "gds-files-sidebar-scroll",
                                a11y:label = move || SharedString::from(i18n::text("volumes")),
                                style:overflow-y = move || {
                                    Some(section_overflow_y(vol_progress.get()).to_owned())
                                },
                                {..region::surface(volumes_node)},
                                on:focus_in = move |_| {
                                    focus_b.sidebar_focus_changed(SidebarRegion::Volumes, true);
                                },
                                on:focus_out = move |_| {
                                    if !node_contains_focus(volumes_node) {
                                        out_b.sidebar_focus_changed(SidebarRegion::Volumes, false);
                                    }
                                }
                            ) {
                                column(
                                    class = "gds-files-sidebar-curtain",
                                    style:opacity = move || {
                                        Some(format!("{:.3}", vol_progress.get().clamp(0.0, 1.0)))
                                    },
                                    style:transform = move || {
                                        section_curtain_transform(vol_progress.get())
                                    }
                                ) {
                                    {{move || {
                                        let snapshot = rows_b.0.volumes.snapshot();
                                        let visible_volumes = snapshot
                                            .volumes
                                            .iter()
                                            .filter(|volume| volume.visible)
                                            .collect::<Vec<_>>();
                                        if visible_volumes.is_empty() {
                                            vec![AnyView::new(view! {
                                                text(class = "gds-files-sidebar-empty") {
                                                    {move || i18n::text("device-none")}
                                                }
                                            })]
                                        } else {
                                            visible_volumes
                                                .into_iter()
                                                .map(|volume| {
                                                    rows_b.sidebar_volume(
                                                        volume,
                                                        &snapshot,
                                                        active_volume,
                                                        compact_capacity,
                                                    )
                                                })
                                                .collect::<Vec<_>>()
                                        }
                                    }}}
                                }
                            }
                        }
                    }
                    }
                }
            }
        };

        let locations_node = self.0.locations_node;
        let locs_browser = browser.clone();
        let locs_toggle = browser.clone();

        AnyView::new(view! {
            SideBar(
                presentation = presentation,
                class = "gds-files-sidebar",
                var:--gds-files-tabs-header-pad-right = move || {
                    Some(format!(
                        "{:.2}px",
                        presentation.get().interpolate(0.0, COLLAPSED_ACTION_INSET)
                    ))
                },
                var:--gds-files-tabs-header-gap = move || {
                    Some(format!("{:.2}px", presentation.get().interpolate(4.0, 0.0)))
                },
                var:--gds-files-tabs-title-pad-x = move || {
                    Some(format!(
                        "{:.2}px",
                        presentation.get().interpolate(SIDEBAR_SECTION_PADDING, 0.0)
                    ))
                },
                var:--gds-files-heading-pad-left = move || {
                    Some(format!(
                        "{:.2}px",
                        presentation.get().interpolate(HEADING_PADDING, COLLAPSED_HEADING_INSET)
                    ))
                },
                var:--gds-files-heading-pad-right = move || {
                    Some(format!("{:.2}px", presentation.get().interpolate(HEADING_PADDING, 0.0)))
                },
                var:--gds-files-heading-gap = move || {
                    Some(format!("{:.2}px", presentation.get().interpolate(GROUP_GAP, 0.0)))
                },
                var:--gds-files-tab-reserve = move || {
                    Some(format!(
                        "{:.2}px",
                        presentation.get().interpolate(ICON_BUTTON_SIZE + ITEM_GAP, 0.0)
                    ))
                },
                var:--gds-files-volume-details-pad-x = move || {
                    Some(format!("{:.2}px", presentation.get().interpolate(ITEM_GAP, 2.0)))
                },
                a11y:label = move || SharedString::from(i18n::text("locations"))
            ) {
                column(
                    node_ref = content_ref,
                    style:display = "flex",
                    style:flex-direction = "column",
                    style:flex = "1 1 0px",
                    style:width = "100%",
                    style:min-width = "0px",
                    style:min-height = move || Some(format!("{:.1}px", workspace_minimum.get())),
                    style:overflow = "hidden"
                ) {
                SideBarContent(
                    scrollable = false,
                    class = "gds-files-sidebar-content"
                ) {
                    {{move || {
                        if show_volumes_sig.get() {
                            let tabs_fn = tabs_section.clone();
                            let vols_fn = volumes_section.clone();
                            AnyView::new(view! {
                                VerticalSplit(
                                    state = split_state,
                                    on_commit = Some(on_split_commit),
                                    label = i18n::text("sidebar-resize"),
                                    bottom_progress = Some(vol_progress),
                                    collapsed_bottom_height = SECTION_HEADER_HEIGHT,
                                    leading = move || AnyView::new(tabs_fn()),
                                    trailing = move || AnyView::new(vols_fn())
                                )
                            })
                        } else {
                            AnyView::new(tabs_section())
                        }
                    }}}
                }
                }

                SideBarFooter(class = "gds-files-sidebar-footer") {
                    column(
                        node_ref = locations_region_ref,
                        style:display = "flex",
                        style:flex-direction = "column",
                        style:flex-shrink = "1",
                        style:width = "100%",
                        style:min-width = "0px",
                        style:min-height = "0px",
                        style:overflow = "hidden"
                    ) {
                    SideBarSection(
                        class = "gds-files-sidebar-locations-section",
                        style:gap = move || Some(format!("{:.2}px", ROW_GAP * loc_progress.get()))
                    ) {
                        {{locs_toggle.sidebar_heading(
                            "locations",
                            LOCATION_HEADER_HEIGHT,
                            loc_expanded,
                            is_collapsed_rail,
                            {
                                let toggle_b = locs_browser.clone();
                                move || {
                                    let focused = toggle_b.0.sidebar_region.get_untracked()
                                        == Some(SidebarRegion::Locations)
                                        || node_contains_focus(locations_node);
                                    toggle_b.edit_preferences(|p| {
                                        p.files.sidebar.locations_collapsed =
                                            !p.files.sidebar.locations_collapsed || focused;
                                    });
                                    if focused {
                                        toggle_b.sidebar_focus_changed(SidebarRegion::Locations, false);
                                        toggle_b.focus_content();
                                    }
                                }
                            },
                        )}}
                        {{
                            let focus_b = locs_browser.clone();
                            let out_b = locs_browser.clone();
                            let rows_b = locs_browser.clone();
                            let home = rows_b.read_untracked(|state| state.home.clone());
                            view! {
                                scroll(
                                    node_ref = locations_node,
                                    class = "gds-files-sidebar-locations",
                                    a11y:label = move || SharedString::from(i18n::text("locations")),
                                    style:display = move || {
                                        (loc_progress.get() <= 1e-4).then(|| "none".to_owned())
                                    },
                                    style:height = move || {
                                        let tags_enabled = sections.get().1;
                                        let count = LocationTarget::ALL
                                            .iter()
                                            .filter(|t| {
                                                tags_enabled
                                                    || !matches!(
                                                        t,
                                                        LocationTarget::Collection(Collection::Tags)
                                                    )
                                            })
                                            .count();
                                        let full = (count as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP)
                                            - ROW_GAP)
                                            .max(0.0);
                                        Some(format!("{:.2}px", full * loc_progress.get()))
                                    },
                                    style:overflow-y = move || {
                                        Some(section_overflow_y(loc_progress.get()).to_owned())
                                    },
                                    {..region::surface(locations_node)},
                                    on:focus_in = move |_| {
                                        focus_b.sidebar_focus_changed(SidebarRegion::Locations, true);
                                    },
                                    on:focus_out = move |_| {
                                        if !node_contains_focus(locations_node) {
                                            out_b.sidebar_focus_changed(SidebarRegion::Locations, false);
                                        }
                                    }
                                ) {
                                    column(
                                        class = "gds-files-sidebar-curtain",
                                        style:opacity = move || {
                                            Some(format!("{:.3}", loc_progress.get().clamp(0.0, 1.0)))
                                        },
                                        style:transform = move || {
                                            section_curtain_transform(loc_progress.get())
                                        }
                                    ) {
                                        for entry in move || {
                                            let tags_enabled = sections.get().1;
                                            LocationTarget::ALL
                                                .iter()
                                                .copied()
                                                .enumerate()
                                                .filter(|(_, target)| {
                                                    tags_enabled
                                                        || !matches!(
                                                            target,
                                                            LocationTarget::Collection(Collection::Tags)
                                                        )
                                                })
                                                .collect::<Vec<_>>()
                                        }, key = |entry: &(usize, LocationTarget)| entry.0 {
                                            {{
                                                let target = entry.1;
                                                let loc = target.location(&home);
                                                let item = rows_b.sidebar_location(
                                                    target,
                                                    loc.clone(),
                                                );
                                                match loc.local_path() {
                                                    Some(path) => {
                                                        rows_b.file_drag_target(item, path.to_owned())
                                                    }
                                                    None => item,
                                                }
                                            }}
                                        }
                                    }
                                }
                            }
                        }}
                    }
                    }
                }
            }
        })
    }

    fn persist_sidebar_height(&self) {
        let height = self.0.sidebar_tabs_height.get_untracked();
        self.edit_preferences(|preferences| preferences.files.sidebar.tabs_height = height);
    }

    fn sidebar_heading(
        &self,
        label: &'static str,
        header_height: f32,
        expanded: Signal<bool, LocalStorage>,
        is_collapsed_rail: Signal<bool, LocalStorage>,
        toggle: impl Fn() + 'static,
    ) -> AnyView {
        let toggle = Rc::new(toggle);
        let clk = Rc::clone(&toggle);
        let key = Rc::clone(&toggle);
        let heading_ref = NodeRef::new();
        #[cfg(test)]
        tests::record_heading_node(label, heading_ref);

        let tip_requested = RwSignal::new_local(false);
        let tip_open = Signal::derive_local(move || tip_requested.get() && is_collapsed_rail.get());
        let reset_tip_fx = RenderEffect::new(move |_| {
            if !is_collapsed_rail.get() && tip_requested.get_untracked() {
                tip_requested.set(false);
            }
        });
        on_cleanup_local(move || drop(reset_tip_fx));

        AnyView::new(view! {
            SideBarSectionHeader(
                class = "gds-files-sidebar-heading-header",
                style:height = format!("{header_height}px")
            ) {
                Tooltip(
                    open = Binding::controlled(tip_open, move |next: bool| {
                        tip_requested.set(next && is_collapsed_rail.get_untracked());
                    }),
                    delay = TOOLTIP_DELAY
                ) {
                    TooltipTrigger {
                        row(
                            node_ref = heading_ref,
                            class = "gds-files-sidebar-heading",
                            tabindex = Focus::Sequential,
                            a11y:role = Role::Button,
                            a11y:label = move || SharedString::from(i18n::text(label)),
                            a11y:expanded = move || expanded.get(),
                            on:click = move |_| clk(),
                            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                if ev.modifiers == Modifiers::NONE
                                    && (ev.key == Key::Named(NamedKey::Enter)
                                        || ev.key == Key::Named(NamedKey::Space)
                                        || ev.key == Key::character(" "))
                                {
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                    key();
                                }
                            }
                        ) {
                            DisclosureIcon(expanded = expanded, size = HEADING_ICON_SIZE)
                            SideBarSectionTitle {{move || i18n::text(label)}}
                        }
                    }
                    TooltipContent(class = "gds-tooltip-content", arrow = false) {
                        TooltipText(text = Signal::derive_local(move || i18n::text(label)))
                    }
                }
            }
        })
    }

    fn sidebar_volume(
        &self,
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

        let action = self.clone();
        let device = volume.id.clone();
        let candidate = SidebarRow::Volume(volume.id.clone());
        let cursor = self.0.sidebar_cursor;
        let region_sig = self.0.sidebar_region;
        let volumes_node = self.0.volumes_node;
        let selected = Signal::derive_local({
            let id = volume.id.clone();
            move || active.get().as_ref() == Some(&id)
        });
        let cursor_match = Signal::derive_local(move || {
            region_sig.get() == Some(SidebarRegion::Volumes)
                && region::is_focus_visible(volumes_node)
                && cursor.with(|row| row.as_ref() == Some(&candidate))
        });

        let status = self.0.volumes.capacity(&volume.id);
        let volume_busy = self.0.volumes.is_busy(&volume.id);
        let mut hint = vec![volume.name.clone(), volume.filesystem.clone()];
        if volume.system_mount {
            hint.push(i18n::text("device-system-volume"));
        }
        let identity = Self::volume_identity(volume);
        if let Some(root) = self.resolve_path_untracked(&Location::Volume {
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
                    action.open_volume(&device, ev.modifiers.primary());
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
        let menu_wrapped = self.sidebar_volume_menu(tipped, volume);
        AnyView::new(view! {
            box(class = "gds-files-sidebar-row-wrap") {
                {menu_wrapped}
            }
        })
    }

    fn sidebar_location(&self, target: LocationTarget, location: Location) -> AnyView {
        let label = Signal::derive_local(move || i18n::text(target.label_id()));
        let icon = target.icon();
        let cursor = self.0.sidebar_cursor;
        let region_sig = self.0.sidebar_region;
        let locations_node = self.0.locations_node;
        let open = self.clone();
        let selected = Signal::derive_local({
            let browser = self.clone();
            let location = location.clone();
            move || {
                browser.read(|state| {
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
                            open.new_tab(Some(location.clone()));
                        } else {
                            open.open(location.clone());
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

    fn sidebar_tab(
        &self,
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
            let browser = self.clone();
            move || {
                browser.read_structure(|state| {
                    let ids: Vec<u64> = state.tabs.iter().map(|t| t.id).collect();
                    let active_id = state.tab().id;
                    let is_files_pane = browser.pane() == Pane::Files;
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
            let browser = self.clone();
            move || {
                let location = tab_meta.with(|(_, _, loc)| loc.clone());
                browser.sidebar_tab_title(&location)
            }
        });
        let detail_path = Signal::derive_local({
            let local_path = local_path.clone();
            move || {
                GdsLang::track();
                local_path.as_deref().map_or_else(
                    || tab_meta.with(|(_, _, loc)| loc.label()),
                    |p| p.display().to_string(),
                )
            }
        });
        let tooltip_text = Signal::derive_local(move || {
            wrap_tokens(&format!("{}\n{}", title.get(), detail_path.get()))
        });

        let select = self.clone();
        let action_expanded = self.clone();
        let action_collapsed = self.clone();
        let middle_close = self.clone();
        let hovered = RwSignal::new_local(false);
        let cursor = self.0.sidebar_cursor;
        let region_sig = self.0.sidebar_region;
        let tabs_node = self.0.tabs_node;

        let selected = Signal::derive_local({
            let browser = self.clone();
            move || browser.read_structure(|state| state.tab().id == id)
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
        tests::record_tab_nodes(id, tab_ref, corner_ref);
        if let Some(ref key) = close_shortcut {
            keytips::anchor_when(corner_ref, key.clone(), collapsed_close_shortcut_enabled);
        }

        let on_middle = (!pinned).then(|| {
            UnsyncCallback::new(move |()| {
                if let Some(index) = middle_close.tab_index(id) {
                    middle_close.close_tab(index);
                }
            })
        });

        let no_drag_exp = self.0.window.as_ref().map(WindowHandle::no_drag_handler);

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
                    if let Some(index) = select.tab_index(id) {
                        select.select_tab(index);
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
                            action_expanded.trigger_tab_action(id, pinned);
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
                                action_collapsed.trigger_tab_action(id, pinned);
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
            Some(path) => self.file_drag_target(tipped, path),
            None => tipped,
        };
        let dragged = self.draggable_tab(wrapped, id, pinned);
        self.tab_menu(dragged, id)
    }

    fn trigger_tab_action(&self, id: u64, pinned: bool) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        if pinned {
            self.toggle_pin(index);
            self.focus_content_after_mount();
        } else {
            self.close_tab(index);
        }
    }

    fn sidebar_tab_title(&self, location: &Location) -> String {
        if let Location::Volume { identity, relative } = location
            && relative.is_empty()
            && let Some(volume) = self.volume_at_location(identity)
        {
            return volume.name;
        }
        self.location_title(location)
    }
}

struct GdsLang;

impl GdsLang {
    fn track() {
        if let Some(gds) = gds::app::Gds::try_current() {
            let _ = gds.i18n().language();
        }
    }
}

pub(super) fn section_overflow_y(progress: f32) -> &'static str {
    if progress < 0.999 { "hidden" } else { "auto" }
}

fn section_curtain_transform(progress: f32) -> Option<String> {
    let p = progress.clamp(0.0, 1.0);
    ((1.0 - p) > 1e-3).then(|| format!("translateY({:.2}px)", -(1.0 - p) * 8.0))
}

fn node_contains_focus(node: NodeRef) -> bool {
    current_host().is_some_and(|host| {
        host.focused()
            .get_untracked()
            .is_some_and(|active| node.get_untracked() == Some(active) || node.contains(active))
    })
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

#[cfg(test)]
pub(super) mod tests;
