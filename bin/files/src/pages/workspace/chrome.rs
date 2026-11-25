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
    SideBarFooterProps, SideBarProps, SideBarSectionContentProps, SideBarSectionHeaderProps,
    SideBarSectionProps, SideBarSectionTitleProps, SidebarPresentation,
};
use gds::ui::window::{HeightTier, WidthTier};
use gds::ui::{HeldModifier, IconName, IconProps, Shortcut};
use zgui::geom::{DevicePx, Point};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, on_cleanup_local};
use zgui::view::{Scoped, ScrollBehavior, ScrollTarget, current_host, events, install_stylesheet};
use zgui::vocab::{Key, Modifiers, NamedKey, PointerButton, Role, ScrollDelta, SharedString};
use zgui::{css, view};
use zgui_ui::{InputProps, SpinnerProps, TooltipContentProps, TooltipProps, TooltipTriggerProps};
use zgui_ui_primitives::Binding;

use super::keyboard::{SidebarRegion, SidebarRow};
use super::model::{AddressPresentation, Pane};
use super::ui::{Browser, FilterFocusMode};
use super::view::local_memo;
use crate::entities::file::ui::{EntryPresentation, FileIcon};
use crate::entities::location::{
    Collection, Location, LocationTarget,
    scheme::{self, TagAddress},
    volumes,
};
use crate::shared::ui::actions::{Command, ordinal_shortcut};

const MIN_TABS_HEIGHT: f32 = 94.0;
const COLLAPSIBLE_HEADER_HEIGHT: f32 = 28.0;
const SECTION_STACK_GAP: f32 = 6.0;
const EMPTY_SECTION_HEIGHT: f32 = 32.0;
const GROUP_GAP: f32 = 6.0;
const ROW_GAP: f32 = 4.0;
pub(super) const ITEM_GAP: f32 = 8.0;
pub(super) const TAB_HEIGHT: f32 = 44.0;
const TAB_CLOSE_RESERVE: f32 = ITEM_GAP * 2.0 + ICON_BUTTON_SIZE;
const DEFAULT_DEVICE_ROWS: usize = 4;
const HEADER_ACTION_SIZE: f32 = 28.0;
const HEADING_ICON_SIZE: f32 = 12.0;
const HEADING_PADDING: f32 = 8.0;
const COLLAPSED_ACTION_INSET: f32 =
    (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING - HEADER_ACTION_SIZE) * 0.5;
const COLLAPSED_HEADING_INSET: f32 =
    (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING - HEADING_ICON_SIZE) * 0.5;
const COMPACT_CAPACITY_THRESHOLD: f32 = 120.0;
const ACCORDION_THRESHOLD_TWO_SECTIONS: f32 = 540.0;
const ACCORDION_THRESHOLD_THREE_SECTIONS: f32 = 680.0;
const TABS_COMFORT_FRACTION: f32 = 0.24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CollapsibleSection {
    QuickAccess,
    Volumes,
    Locations,
}

impl CollapsibleSection {
    pub(super) fn from_region(region: SidebarRegion) -> Option<Self> {
        match region {
            SidebarRegion::Tabs => None,
            SidebarRegion::QuickAccess => Some(Self::QuickAccess),
            SidebarRegion::Volumes => Some(Self::Volumes),
            SidebarRegion::Locations => Some(Self::Locations),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AccordionSelection {
    #[default]
    Auto,
    Expanded(CollapsibleSection),
    AllCollapsed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CollapsibleMeta {
    pub visible: bool,
    pub pref_expanded: bool,
    pub has_items: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuickAccessAddState {
    CanAdd,
    AlreadyAdded,
    NotLocal,
}

pub(super) fn sidebar_accordion_threshold(visible_collapsible_count: usize) -> f32 {
    match visible_collapsible_count {
        0 | 1 => 0.0,
        2 => ACCORDION_THRESHOLD_TWO_SECTIONS,
        _ => ACCORDION_THRESHOLD_THREE_SECTIONS,
    }
}

pub(super) fn default_accordion_section(
    focused: Option<CollapsibleSection>,
    qa: CollapsibleMeta,
    vol: CollapsibleMeta,
    loc: CollapsibleMeta,
) -> Option<CollapsibleSection> {
    let entries = [
        (CollapsibleSection::QuickAccess, qa),
        (CollapsibleSection::Volumes, vol),
        (CollapsibleSection::Locations, loc),
    ];
    if let Some(sec) = focused
        && entries.iter().any(|(s, meta)| *s == sec && meta.visible)
    {
        return Some(sec);
    }
    if let Some((sec, _)) = entries
        .iter()
        .find(|(_, meta)| meta.visible && meta.pref_expanded && meta.has_items)
    {
        return Some(*sec);
    }
    entries
        .iter()
        .find(|(_, meta)| meta.visible && meta.pref_expanded)
        .map(|(sec, _)| *sec)
}

pub(super) fn allocate_collapsible_heights(budget: f32, desired: [f32; 3]) -> [f32; 3] {
    let budget = budget.max(0.0);
    let mut out = [0.0_f32; 3];
    let mut remaining_budget = budget;
    let mut active = [desired[0] > 0.0, desired[1] > 0.0, desired[2] > 0.0];
    loop {
        let active_count = active.iter().filter(|&&flag| flag).count();
        if active_count == 0 || remaining_budget <= 0.0 {
            break;
        }
        let share = remaining_budget / active_count as f32;
        let mut settled_any = false;
        for i in 0..3 {
            if active[i] && desired[i] <= share {
                out[i] = desired[i];
                remaining_budget = (remaining_budget - desired[i]).max(0.0);
                active[i] = false;
                settled_any = true;
            }
        }
        if !settled_any {
            for i in 0..3 {
                if active[i] {
                    out[i] = share;
                }
            }
            break;
        }
    }
    out
}

fn quick_access_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| (!name.is_empty()).then(|| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| path.display().to_string())
}

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
        height: var(--gds-icon-button-size, 32px);
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
        font-size: var(--gds-text-sm, 14px);
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
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-files-address-read.gds-files-address-read {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 0px;
        height: 100%;
        padding-right: 2px;
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
        gap: 4px;
        height: 24px;
        padding: 0px 4px;
        border-radius: 4px;
        cursor: pointer;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-text-sm, 14px);
        user-select: none;
    }

    .gds-files-crumb.gds-files-crumb[data-current="true"] {
        color: var(--gds-foreground);
        font-weight: 500;
    }

    .gds-files-crumb.gds-files-crumb:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"] {
        height: 22px;
        padding: 0px 7px;
        border-radius: 5px;
        background-color: color-mix(in srgb, var(--gds-primary) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--gds-primary) 24%, transparent);
        color: var(--gds-primary);
        font-size: var(--gds-text-xs, 12px);
        font-weight: 500;
        box-sizing: border-box;
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"][data-interactive="true"]:hover {
        background-color: color-mix(in srgb, var(--gds-primary) 18%, transparent);
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"][data-interactive="false"] {
        cursor: default;
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"][data-interactive="false"]:hover {
        background-color: color-mix(in srgb, var(--gds-primary) 12%, transparent);
    }

    .gds-files-crumb-icon {
        flex: none;
        color: currentColor;
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

    .gds-files-micro-filter.gds-files-micro-filter {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        min-width: 0px;
        flex: 0 1 auto;
    }

    .gds-files-micro-filter.gds-files-micro-filter[data-floating="true"] {
        position: absolute;
        left: 12px;
        bottom: 10px;
        z-index: 6;
        padding: 3px;
        border-radius: var(--gds-radius-control, 6px);
        background-color: var(--gds-panel);
        border: 1px solid var(--gds-border);
    }

    .gds-files-micro-filter-input-wrap.gds-files-micro-filter-input-wrap {
        display: flex;
        width: 184px;
        min-width: 88px;
        max-width: 220px;
        flex: 0 1 auto;
        transition: width var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-input-field.gds-input-field.gds-files-micro-filter-field {
        width: 100%;
        min-width: 0px;
        height: 24px;
        min-height: 24px;
        border-radius: 5px;
    }

    .gds-input-field.gds-files-micro-filter-field .gds-input-field__leading {
        padding-left: 7px;
    }

    .gds-input-field.gds-files-micro-filter-field .gds-input-field__trailing {
        padding-right: 2px;
    }

    .gds-input-field.gds-files-micro-filter-field .gds-input-field__clear {
        width: 18px;
        height: 18px;
    }

    .gds-input-field.gds-files-micro-filter-field .zui-field.gds-input-field__input {
        font-size: var(--gds-text-xs, 12px);
        padding: 0px 6px;
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

    .gds-files-sidebar-collapsible-stack.gds-files-sidebar-collapsible-stack {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        padding-top: 6px;
        padding-bottom: var(--gds-sidebar-padding, 10px);
        gap: 6px;
        box-sizing: border-box;
        overflow: hidden;
    }

    .gds-files-sidebar-collapsible-section.gds-files-sidebar-collapsible-section {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        padding: 0px;
        box-sizing: border-box;
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
        flex: 0 0 auto;
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
        min-width: calc(var(--gds-collapsed-sidebar-width, 56px) - 2 * var(--gds-sidebar-padding, 10px));
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
        min-height: 28px;
        height: 28px;
        padding: 0px var(--gds-sidebar-padding, 10px);
        gap: 2px;
        box-sizing: border-box;
    }

    .gds-files-tabs-title.gds-files-tabs-title {
        padding: 0px var(--gds-files-tabs-title-pad-x, 6px);
    }

    .gds-files-sidebar-content {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-sidebar-empty {
        padding: 6px;
        font-size: var(--gds-text-xs, 12px);
        color: color-mix(
            in srgb,
            var(--gds-muted-foreground) var(--gds-sidebar-label-pct, 100%),
            transparent
        );
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
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
        min-width: calc(var(--gds-collapsed-sidebar-width, 56px) - 2 * var(--gds-sidebar-padding, 10px));
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

pub(super) fn install_chrome_stylesheets() {
    gds::ui::styles::install_ui_stylesheets();
    if current_host().is_some() {
        install_stylesheet(CHROME_SHEET, CHROME_CSS);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Breadcrumb {
    pub label: String,
    pub location: Option<Location>,
    pub icon: Option<IconName>,
    pub scheme: bool,
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
                    location: Some(Location::from(target.as_path())),
                    icon: None,
                    scheme: false,
                    current,
                    separated: false,
                });
            }
            if segments.is_empty() {
                segments.push(Self {
                    label: path.display().to_string(),
                    location: Some(Location::from(path)),
                    icon: None,
                    scheme: false,
                    current: true,
                    separated: false,
                });
            }
            return segments;
        }
        match location {
            Location::Tags(address) => Self::for_tags(address),
            Location::Remote(address) => Self::for_remote(address),
            Location::Recent | Location::Network | Location::Trash => vec![Self {
                label: location.label(),
                location: Some(location.clone()),
                icon: Some(location.icon()),
                scheme: true,
                current: true,
                separated: false,
            }],
            Location::Favorites | Location::Volumes => vec![
                Self::scheme_prefix(i18n::text_untracked("scheme-location"), IconName::Bookmark),
                Self {
                    label: location.label(),
                    location: Some(location.clone()),
                    icon: None,
                    scheme: false,
                    current: true,
                    separated: true,
                },
            ],
            Location::Volume { identity, relative } => {
                let mut segments = vec![
                    Self::scheme_prefix(
                        i18n::text_untracked("scheme-location"),
                        IconName::Bookmark,
                    ),
                    Self {
                        label: i18n::text_untracked(Collection::Volumes.label_id()),
                        location: Some(Location::Volumes),
                        icon: None,
                        scheme: false,
                        current: relative.is_empty(),
                        separated: true,
                    },
                ];
                if !relative.is_empty() {
                    let parts: Vec<&std::ffi::OsStr> = Path::new(relative)
                        .components()
                        .filter_map(|part| match part {
                            Component::Normal(seg) => Some(seg),
                            _ => None,
                        })
                        .collect();
                    let total = parts.len();
                    let mut acc = PathBuf::new();
                    for (idx, part) in parts.into_iter().enumerate() {
                        acc.push(part);
                        segments.push(Self {
                            label: part.to_string_lossy().into_owned(),
                            location: Some(Location::Volume {
                                identity: identity.clone(),
                                relative: acc.as_os_str().to_owned(),
                            }),
                            icon: None,
                            scheme: false,
                            current: idx + 1 == total,
                            separated: true,
                        });
                    }
                }
                segments
            }
            Location::Directory(_) => vec![Self {
                label: location.display_address(),
                location: Some(location.clone()),
                icon: None,
                scheme: false,
                current: true,
                separated: false,
            }],
        }
    }

    fn scheme_prefix(label: String, icon: IconName) -> Self {
        Self {
            label,
            location: None,
            icon: Some(icon),
            scheme: true,
            current: false,
            separated: false,
        }
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
                let is_root = index == 0;
                let loc = Location::Tags(item);
                Self {
                    label: if is_root {
                        i18n::text_untracked("scheme-tags")
                    } else {
                        loc.label()
                    },
                    location: Some(loc),
                    icon: is_root.then_some(IconName::Tag),
                    scheme: is_root,
                    current: index + 1 == total,
                    separated: index > 0,
                }
            })
            .collect()
    }

    fn for_remote(address: &gds::service::remote::Address) -> Vec<Self> {
        let (scheme_key, scheme_icon) = match address.protocol() {
            gds::service::remote::Protocol::Sftp => ("scheme-sftp", IconName::Server),
            gds::service::remote::Protocol::Smb => ("scheme-smb", IconName::Network),
        };
        let mut chain = vec![address.clone()];
        let mut cursor = address.clone();
        while let Some(parent) = cursor.parent() {
            chain.push(parent.clone());
            cursor = parent;
        }
        chain.reverse();
        let total = chain.len();
        let mut segments = Vec::with_capacity(total + 1);
        segments.push(Self::scheme_prefix(
            i18n::text_untracked(scheme_key),
            scheme_icon,
        ));
        for (index, item) in chain.into_iter().enumerate() {
            segments.push(Self {
                label: item.name(),
                location: Some(Location::Remote(item)),
                icon: None,
                scheme: false,
                current: index + 1 == total,
                separated: true,
            });
        }
        segments
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

fn default_volumes_body_height(visible_count: usize, mounted_count: usize) -> f32 {
    if visible_count == 0 {
        return EMPTY_SECTION_HEIGHT;
    }
    let rows = visible_count.min(DEFAULT_DEVICE_ROWS);
    let mounted = mounted_count.min(rows);
    (rows as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP) + mounted as f32 * EXTRA_ROW_HEIGHT - ROW_GAP)
        .max(EMPTY_SECTION_HEIGHT)
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

    /// Right-column toolbar controls (`[PanelLeft, Back, Forward, Parent, flex:1 AddressBar, optional Micro ... menu]`).
    pub(super) fn toolbar_controls(&self) -> AnyView {
        install_chrome_stylesheets();
        let browser = self.clone();
        let toggle_b = self.clone();
        let width_b = self.clone();
        let height_b = self.clone();
        let pane_b = self.clone();
        let micro_width = Signal::derive_local(move || width_b.width_tier() == WidthTier::Micro);
        let micro_height =
            Signal::derive_local(move || height_b.height_tier() == HeightTier::Micro);
        let pane_sig = local_memo(move || pane_b.pane());
        let show_sidebar_toggle =
            Signal::derive_local(move || pane_sig.get() != Pane::Help && !micro_width.get());
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
                        let menu_b = browser.clone();
                        let no_drag_menu = no_drag;
                        AnyView::new(view! {
                            row(class = "gds-files-toolbar-files") {
                                Show(when = move || !micro_width.get()) {
                                    {{nav_b.navigation_buttons()}}
                                }
                                {{addr_b.address_bar()}}
                                Show(when = move || micro_height.get()) {
                                    {{
                                        let mb = menu_b.clone();
                                        view! {
                                            row(
                                                class = "gds-files-menu-controls",
                                                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                                                    if let Some(ref handler) = no_drag_menu {
                                                        handler(ev);
                                                    } else {
                                                        ev.stop_propagation();
                                                    }
                                                }
                                            ) {
                                                {mb.page_menu()}
                                            }
                                        }
                                    }}
                                }
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
                        GdsLang::track();
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
                    let is_current = crumb.current;
                    let is_scheme = crumb.scheme;
                    let icon_view = crumb.icon.map(|icon_name| {
                        AnyView::new(view! {
                            Icon(
                                name = icon_name,
                                size = 13.0,
                                class = "gds-files-crumb-icon",
                                a11y:hidden = true
                            )
                        })
                    });
                    let label = crumb.label;
                    if let Some(target_loc) = crumb.location {
                        let click_browser = browser.clone();
                        let key_browser = browser.clone();
                        let key_loc = target_loc.clone();
                        let a11y_label = label.clone();
                        let crumb_ref = NodeRef::new();
                        items.push(AnyView::new(view! {
                            box(
                                node_ref = crumb_ref,
                                class = "gds-files-crumb",
                                attr:data-scheme = is_scheme.then(|| "true".to_owned()),
                                attr:data-interactive = is_scheme.then(|| "true".to_owned()),
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
                                {icon_view}
                                text(class = "gds-files-crumb-text") {{label}}
                            }
                        }));
                    } else {
                        items.push(AnyView::new(view! {
                            box(
                                class = "gds-files-crumb",
                                attr:data-scheme = is_scheme.then(|| "true".to_owned()),
                                attr:data-interactive = is_scheme.then(|| "false".to_owned()),
                            ) {
                                {icon_view}
                                text(class = "gds-files-crumb-text") {{label}}
                            }
                        }));
                    }
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

    pub(super) fn activate_filter(&self) {
        if !self.filter_available_untracked() {
            return;
        }
        if self.height_tier_untracked() == HeightTier::Micro {
            self.0.footer_filter_open.set(true);
        }
        let node = self.0.filter_node;
        if node.is_bound() {
            node.focus();
            node.select_all();
        } else {
            self.0
                .filter_focus_request
                .set(Some(FilterFocusMode::SelectAll));
        }
    }

    pub(super) fn folder_filter(&self) -> AnyView {
        install_chrome_stylesheets();
        let filter_sig = self.0.filter;
        let filter_node = self.0.filter_node;
        let focus_req = self.0.filter_focus_request;
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
                        attach_filter_focus_effect(filter_node, filter_sig, focus_req);
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

    /// Subordinate `24px` directory filter used in `HeightTier::Micro` (`H < 520px`), mounted
    /// either inside the `32px` footer's status slot (`floating = false`) or as a bottom-left
    /// floating pill when Save-picker mode occupies the footer status slot (`floating = true`).
    pub(super) fn micro_filter(&self, floating: bool) -> AnyView {
        install_chrome_stylesheets();
        let filter_sig = self.0.filter;
        let filter_node = self.0.filter_node;
        let footer_open = self.0.footer_filter_open;
        let focus_req = self.0.filter_focus_request;
        let open_browser = self.clone();
        let change_browser = self.clone();
        let clear_browser = self.clone();
        let expanded =
            Signal::derive_local(move || footer_open.get() || !filter_sig.get().is_empty());

        AnyView::new(view! {
            row(
                class = "gds-files-micro-filter",
                attr:data-floating = floating.then(|| "true".to_owned()),
            ) {
                {{move || {
                    if expanded.get() {
                        let wrap_ref = NodeRef::new();
                        keytips::stage_shortcut(
                            wrap_ref,
                            Command::Filter.shortcut(),
                            HintPlacement::Trailing,
                            Signal::stored_local(true),
                        );
                        attach_filter_focus_effect(filter_node, filter_sig, focus_req);
                        let cb = change_browser.clone();
                        let clb = clear_browser.clone();
                        AnyView::new(view! {
                            box(node_ref = wrap_ref, class = "gds-files-micro-filter-input-wrap") {
                                {{move || {
                                    GdsLang::track();
                                    let change_b = cb.clone();
                                    let clear_b = clb.clone();
                                    view! {
                                        InputField(
                                            node_ref = filter_node,
                                            value = Binding::from(filter_sig),
                                            placeholder = i18n::text("filter-placeholder"),
                                            label = i18n::text("filter"),
                                            leading_icon = IconName::Search,
                                            clearable = true,
                                            class = "gds-files-micro-filter-field",
                                            on_change = UnsyncCallback::new(move |value: String| {
                                                change_b.filter(value);
                                            }),
                                            on_clear = UnsyncCallback::new(move |()| {
                                                clear_b.filter(String::new());
                                                clear_b.0.footer_filter_open.set(false);
                                                clear_b.focus_content();
                                            }),
                                            on:focus_out = move |_| {
                                                if filter_sig.get_untracked().is_empty()
                                                    && !node_contains_focus(filter_node)
                                                {
                                                    footer_open.set(false);
                                                }
                                            },
                                        )
                                    }
                                }}}
                            }
                        })
                    } else {
                        let trigger_b = open_browser.clone();
                        AnyView::new(view! {
                            IconButton(
                                icon = IconName::Search,
                                size = ButtonSize::IconXs,
                                label = Signal::derive_local(|| i18n::text("filter")),
                                shortcut = Some(Command::Filter.shortcut()),
                                on:click = move |_| trigger_b.activate_filter(),
                            )
                        })
                    }
                }}}
            }
        })
    }

    pub(super) fn filter_available(&self) -> bool {
        self.pane() == Pane::Files
            && self.read(|state| match state.tab().navigation.location() {
                Location::Recent | Location::Favorites | Location::Tags(_) => {
                    self.height_tier() == HeightTier::Micro || !self.actions_collapsed()
                }
                Location::Directory(_) | Location::Volume { .. } | Location::Remote(_) => true,
                Location::Network | Location::Volumes | Location::Trash => false,
            })
    }

    pub(super) fn filter_available_untracked(&self) -> bool {
        self.pane_untracked() == Pane::Files
            && self.read_untracked(|state| match state.tab().navigation.location() {
                Location::Recent | Location::Favorites | Location::Tags(_) => {
                    self.height_tier_untracked() == HeightTier::Micro
                        || !self.actions_collapsed_untracked()
                }
                Location::Directory(_) | Location::Volume { .. } | Location::Remote(_) => true,
                Location::Network | Location::Volumes | Location::Trash => false,
            })
    }

    pub(super) fn sidebar(&self, _presentation: SidebarPresentation) -> AnyView {
        let browser = self.clone();
        AnyView::new(Scoped::new(move || browser.sidebar_scoped()))
    }

    pub(super) fn quick_access_paths(&self) -> Vec<PathBuf> {
        self.read(|state| {
            state
                .places
                .quick_access
                .iter()
                .filter_map(|loc| loc.local_path().map(Path::to_path_buf))
                .collect()
        })
    }

    pub(super) fn quick_access_paths_untracked(&self) -> Vec<PathBuf> {
        self.read_untracked(|state| {
            state
                .places
                .quick_access
                .iter()
                .filter_map(|loc| loc.local_path().map(Path::to_path_buf))
                .collect()
        })
    }

    pub(super) fn tab_local_path(&self, id: u64) -> Option<PathBuf> {
        self.read_untracked(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .and_then(|tab| tab.navigation.path().map(Path::to_path_buf))
        })
    }

    pub(super) fn add_quick_access_path(&self, path: PathBuf) -> bool {
        let location = Location::from(path.as_path());
        let added = self.update(|state| {
            if state.places.contains_quick_access(&location) {
                return false;
            }
            state.places.add_quick_access(location.clone());
            true
        });
        if added {
            self.save_places();
        }
        let prefs = self.0.preferences.snapshot();
        if !prefs.files.sidebar.show_quick_access {
            self.0.sidebar_reveal_quick_access.set(true);
        }
        if self.0.sidebar_in_accordion.get_untracked() {
            self.0.sidebar_accordion.set(AccordionSelection::Expanded(
                CollapsibleSection::QuickAccess,
            ));
        } else if prefs.files.sidebar.quick_access_collapsed {
            self.edit_preferences(|p| p.files.sidebar.quick_access_collapsed = false);
        }
        added
    }

    pub(super) fn add_current_to_quick_access(&self) {
        if self.pane_untracked() != Pane::Files {
            self.report_notice(i18n::text_untracked("quick-access-local-only"));
            return;
        }
        let path =
            self.read_untracked(|state| state.tab().navigation.path().map(Path::to_path_buf));
        let Some(path) = path else {
            self.report_notice(i18n::text_untracked("quick-access-local-only"));
            return;
        };
        let _ = self.add_quick_access_path(path);
    }

    pub(super) fn add_quick_access_from_tab(&self, tab_id: u64) {
        let Some(path) = self.tab_local_path(tab_id) else {
            self.report_notice(i18n::text_untracked("quick-access-local-only"));
            return;
        };
        let _ = self.add_quick_access_path(path);
    }

    pub(super) fn remove_quick_access(&self, path: &Path) {
        let location = Location::from(path);
        let removed = self.update(|state| {
            if !state.places.contains_quick_access(&location) {
                return false;
            }
            state.places.remove_quick_access(location.clone());
            true
        });
        if removed {
            self.save_places();
        }
    }

    pub(super) fn reorder_quick_access_item(&self, from: &Path, to: &Path) {
        let from_loc = Location::from(from);
        let to_loc = Location::from(to);
        let changed = self.update(|state| {
            let mut order = state.places.quick_access.clone();
            let Some(from_idx) = order.iter().position(|loc| loc == &from_loc) else {
                return false;
            };
            let Some(to_idx) = order.iter().position(|loc| loc == &to_loc) else {
                return false;
            };
            if from_idx == to_idx {
                return false;
            }
            let item = order.remove(from_idx);
            order.insert(to_idx, item);
            state.places.reorder_quick_access(order);
            true
        });
        if changed {
            self.save_places();
        }
    }

    pub(super) fn quick_access_add_state_untracked(&self) -> QuickAccessAddState {
        if self.pane_untracked() != Pane::Files {
            return QuickAccessAddState::NotLocal;
        }
        self.read_untracked(|state| match state.tab().navigation.path() {
            Some(path) if state.places.contains_quick_access(Location::from(path)) => {
                QuickAccessAddState::AlreadyAdded
            }
            Some(_) => QuickAccessAddState::CanAdd,
            None => QuickAccessAddState::NotLocal,
        })
    }

    pub(super) fn open_quick_access(&self, path: &Path, in_new_tab: bool) {
        if !path.is_dir() {
            self.report_notice(gds::t!(
                "quick-access-missing",
                path = path.display().to_string()
            ));
            return;
        }
        if in_new_tab {
            self.new_tab(Some(Location::from(path)));
        } else {
            self.open(Location::from(path));
        }
        self.focus_content_after_mount();
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
                    preferences.files.sidebar.show_quick_access
                        || browser.0.sidebar_reveal_quick_access.get(),
                    preferences.files.sidebar.show_volumes
                        || browser.0.sidebar_reveal_volumes.get(),
                    preferences.tags.enabled,
                )
            }
        });
        let show_quick_access_sig = local_memo(move || sections.get().0);
        let show_volumes_sig = local_memo(move || sections.get().1);

        let quick_access_paths = local_memo({
            let b = self.clone();
            move || b.quick_access_paths()
        });
        let volumes_counts = local_memo({
            let b = self.clone();
            move || {
                let snapshot = b.0.volumes.snapshot();
                let visible_count = snapshot.volumes.iter().filter(|v| v.visible).count();
                let mounted_count = snapshot
                    .volumes
                    .iter()
                    .filter(|v| v.visible && !v.mount_points.is_empty())
                    .count();
                (visible_count, mounted_count)
            }
        });
        let locations_count = local_memo(move || {
            let tags_enabled = sections.get().2;
            LocationTarget::ALL
                .iter()
                .filter(|t| {
                    tags_enabled || !matches!(t, LocationTarget::Collection(Collection::Tags))
                })
                .count()
        });

        let sidebar_root_ref = self.0.sidebar_content_node;
        let sidebar_size = sidebar_root_ref.observe_border_size();
        let sidebar_height = Signal::derive_local({
            let win_h = self.0.window_height;
            move || {
                let scale = sidebar_root_ref.scale().max(1.0);
                sidebar_size
                    .get()
                    .map(|sz| (sz.height.0 / scale).max(0.0))
                    .filter(|h| *h > 1.0)
                    .unwrap_or_else(|| (win_h.get() - 48.0).max(0.0))
            }
        });

        let height_tier = Signal::derive_local({
            let b = self.clone();
            move || b.height_tier()
        });
        let visible_collapsible_count = local_memo(move || {
            1 + usize::from(show_quick_access_sig.get()) + usize::from(show_volumes_sig.get())
        });
        let in_accordion = local_memo(move || {
            let count = visible_collapsible_count.get();
            count >= 2 && sidebar_height.get() < sidebar_accordion_threshold(count)
        });
        let single_micro_collapse = local_memo(move || {
            visible_collapsible_count.get() == 1 && height_tier.get() == HeightTier::Micro
        });
        let transient_mode = local_memo(move || in_accordion.get() || single_micro_collapse.get());

        let accordion_sig = self.0.sidebar_accordion;
        let in_accordion_sig = self.0.sidebar_in_accordion;
        let sync_accordion_fx = RenderEffect::new(move |_| {
            let transient = transient_mode.get();
            if in_accordion_sig.get_untracked() != transient {
                in_accordion_sig.set(transient);
            }
            if !transient && accordion_sig.get_untracked() != AccordionSelection::Auto {
                accordion_sig.set(AccordionSelection::Auto);
            }
        });
        on_cleanup_local(move || drop(sync_accordion_fx));

        let accordion_active = local_memo({
            let b = self.clone();
            move || {
                if !transient_mode.get() {
                    return None;
                }
                let show_qa = show_quick_access_sig.get();
                let show_vol = show_volumes_sig.get();
                let is_visible = |sec: CollapsibleSection| match sec {
                    CollapsibleSection::QuickAccess => show_qa,
                    CollapsibleSection::Volumes => show_vol,
                    CollapsibleSection::Locations => true,
                };
                match b.0.sidebar_accordion.get() {
                    AccordionSelection::Expanded(sec) if is_visible(sec) => Some(sec),
                    AccordionSelection::AllCollapsed => None,
                    AccordionSelection::Auto | AccordionSelection::Expanded(_) => {
                        if single_micro_collapse.get() {
                            return None;
                        }
                        let (qa_pref, vol_pref, loc_pref) = b.0.preferences.state().with(|p| {
                            (
                                !p.files.sidebar.quick_access_collapsed,
                                !p.files.sidebar.volumes_collapsed,
                                !p.files.sidebar.locations_collapsed,
                            )
                        });
                        let focused =
                            b.0.sidebar_region
                                .get()
                                .and_then(CollapsibleSection::from_region);
                        default_accordion_section(
                            focused,
                            CollapsibleMeta {
                                visible: show_qa,
                                pref_expanded: qa_pref,
                                has_items: !quick_access_paths.with(Vec::is_empty),
                            },
                            CollapsibleMeta {
                                visible: show_vol,
                                pref_expanded: vol_pref,
                                has_items: volumes_counts.get().0 > 0,
                            },
                            CollapsibleMeta {
                                visible: true,
                                pref_expanded: loc_pref,
                                has_items: locations_count.get() > 0,
                            },
                        )
                    }
                }
            }
        });

        let qa_expanded = Signal::derive_local({
            let b = self.clone();
            move || {
                if !show_quick_access_sig.get() {
                    return false;
                }
                let focused = b.0.sidebar_region.get() == Some(SidebarRegion::QuickAccess);
                if transient_mode.get() {
                    accordion_active.get() == Some(CollapsibleSection::QuickAccess) || focused
                } else {
                    !b.0.preferences
                        .state()
                        .with(|p| p.files.sidebar.quick_access_collapsed)
                        || focused
                }
            }
        });
        let vol_expanded = Signal::derive_local({
            let b = self.clone();
            move || {
                if !show_volumes_sig.get() {
                    return false;
                }
                let focused = b.0.sidebar_region.get() == Some(SidebarRegion::Volumes);
                if transient_mode.get() {
                    accordion_active.get() == Some(CollapsibleSection::Volumes) || focused
                } else {
                    !b.0.preferences
                        .state()
                        .with(|p| p.files.sidebar.volumes_collapsed)
                        || focused
                }
            }
        });
        let loc_expanded = Signal::derive_local({
            let b = self.clone();
            move || {
                let focused = b.0.sidebar_region.get() == Some(SidebarRegion::Locations);
                if transient_mode.get() {
                    accordion_active.get() == Some(CollapsibleSection::Locations) || focused
                } else {
                    !b.0.preferences
                        .state()
                        .with(|p| p.files.sidebar.locations_collapsed)
                        || focused
                }
            }
        });

        let qa_progress = use_transition(
            Signal::derive_local(move || if qa_expanded.get() { 1.0 } else { 0.0 }),
            WORKSPACE_DURATION_MS,
        );
        let vol_progress = use_transition(
            Signal::derive_local(move || if vol_expanded.get() { 1.0 } else { 0.0 }),
            WORKSPACE_DURATION_MS,
        );
        let loc_progress = use_transition(
            Signal::derive_local(move || if loc_expanded.get() { 1.0 } else { 0.0 }),
            WORKSPACE_DURATION_MS,
        );

        let allocated_heights = Signal::derive_local(move || {
            let show_qa = show_quick_access_sig.get();
            let show_vol = show_volumes_sig.get();
            let qa_p = if show_qa { qa_progress.get() } else { 0.0 };
            let vol_p = if show_vol { vol_progress.get() } else { 0.0 };
            let loc_p = loc_progress.get();

            let qa_count = quick_access_paths.with(Vec::len);
            let qa_natural = if qa_count == 0 {
                EMPTY_SECTION_HEIGHT
            } else {
                (qa_count as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP) - ROW_GAP)
                    .max(EMPTY_SECTION_HEIGHT)
            };
            let (vis_vols, mounted_vols) = volumes_counts.get();
            let vol_natural = default_volumes_body_height(vis_vols, mounted_vols);
            let loc_natural =
                (locations_count.get() as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP) - ROW_GAP).max(0.0);

            let visible_count = visible_collapsible_count.get();
            let stack_overhead = visible_count as f32 * COLLAPSIBLE_HEADER_HEIGHT
                + visible_count.saturating_sub(1) as f32 * SECTION_STACK_GAP
                + SECTION_STACK_GAP
                + SIDEBAR_PADDING
                + (qa_p + vol_p + loc_p) * ROW_GAP;
            let total_h = sidebar_height.get();
            let tabs_reserve = if transient_mode.get() {
                MIN_TABS_HEIGHT
            } else {
                (total_h * TABS_COMFORT_FRACTION).max(MIN_TABS_HEIGHT)
            };
            let budget = (total_h - tabs_reserve - stack_overhead).max(0.0);
            allocate_collapsible_heights(
                budget,
                [qa_natural * qa_p, vol_natural * vol_p, loc_natural * loc_p],
            )
        });

        let tabs_region_ref = NodeRef::new();
        let quick_access_region_ref = NodeRef::new();
        let volumes_region_ref = NodeRef::new();
        let locations_region_ref = self.0.locations_region_node;
        keytips::anchor(tabs_region_ref, Command::FocusTabs.shortcut());
        keytips::anchor_when(
            quick_access_region_ref,
            Command::FocusQuickAccess.shortcut(),
            show_quick_access_sig,
        );
        keytips::anchor_when(
            volumes_region_ref,
            Command::FocusVolumes.shortcut(),
            show_volumes_sig,
        );
        keytips::anchor(locations_region_ref, Command::FocusLocations.shortcut());

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
                                size = ButtonSize::IconSm,
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

        let quick_access_section = {
            let qa_b = browser.clone();
            let quick_access_node = self.0.quick_access_node;
            move || {
                let toggle_b = qa_b.clone();
                let focus_b = qa_b.clone();
                let out_b = qa_b.clone();
                let rows_b = qa_b.clone();
                let head_b = qa_b.clone();
                let menu_b = qa_b.clone();
                let drop_b = qa_b.clone();

                let inner = AnyView::new(view! {
                    column(
                        node_ref = quick_access_region_ref,
                        style:display = "flex",
                        style:flex-direction = "column",
                        style:flex = "0 0 auto",
                        style:width = "100%",
                        style:min-width = "0px",
                        style:min-height = "0px",
                        style:overflow = "hidden"
                    ) {
                        SideBarSection(
                            class = "gds-files-sidebar-collapsible-section",
                            style:gap = move || Some(format!("{:.2}px", ROW_GAP * qa_progress.get()))
                        ) {
                            {{head_b.sidebar_heading(
                                "quick-access",
                                COLLAPSIBLE_HEADER_HEIGHT,
                                qa_expanded,
                                is_collapsed_rail,
                                move || {
                                    let focused = toggle_b.0.sidebar_region.get_untracked()
                                        == Some(SidebarRegion::QuickAccess)
                                        || node_contains_focus(quick_access_node);
                                    if transient_mode.get_untracked() {
                                        let currently_open = accordion_active.get_untracked()
                                            == Some(CollapsibleSection::QuickAccess)
                                            || focused;
                                        toggle_b.0.sidebar_accordion.set(if currently_open {
                                            AccordionSelection::AllCollapsed
                                        } else {
                                            AccordionSelection::Expanded(
                                                CollapsibleSection::QuickAccess,
                                            )
                                        });
                                    } else {
                                        toggle_b.edit_preferences(|p| {
                                            p.files.sidebar.quick_access_collapsed =
                                                !p.files.sidebar.quick_access_collapsed || focused;
                                        });
                                    }
                                    if focused {
                                        toggle_b.sidebar_focus_changed(
                                            SidebarRegion::QuickAccess,
                                            false,
                                        );
                                        toggle_b.focus_content();
                                    }
                                },
                            )}}
                            scroll(
                                node_ref = quick_access_node,
                                class = "gds-files-sidebar-locations",
                                a11y:label = move || SharedString::from(i18n::text("quick-access")),
                                style:display = move || {
                                    (qa_progress.get() <= 1e-4).then(|| "none".to_owned())
                                },
                                style:height = move || {
                                    Some(format!("{:.2}px", allocated_heights.get()[0]))
                                },
                                style:overflow-y = move || {
                                    Some(section_overflow_y(qa_progress.get()).to_owned())
                                },
                                {..region::surface(quick_access_node)},
                                on:focus_in = move |_| {
                                    focus_b.sidebar_focus_changed(SidebarRegion::QuickAccess, true);
                                },
                                on:focus_out = move |_| {
                                    if !node_contains_focus(quick_access_node) {
                                        out_b.sidebar_focus_changed(
                                            SidebarRegion::QuickAccess,
                                            false,
                                        );
                                    }
                                }
                            ) {
                                column(
                                    class = "gds-files-sidebar-curtain",
                                    style:opacity = move || {
                                        Some(format!("{:.3}", qa_progress.get().clamp(0.0, 1.0)))
                                    },
                                    style:transform = move || {
                                        section_curtain_transform(qa_progress.get())
                                    }
                                ) {
                                    {{move || {
                                        let paths = quick_access_paths.get();
                                        if paths.is_empty() {
                                            vec![AnyView::new(view! {
                                                text(class = "gds-files-sidebar-empty") {
                                                    {move || i18n::text("quick-access-empty")}
                                                }
                                            })]
                                        } else {
                                            paths
                                                .into_iter()
                                                .map(|path| rows_b.sidebar_quick_access_item(path))
                                                .collect::<Vec<_>>()
                                        }
                                    }}}
                                }
                            }
                        }
                    }
                });
                drop_b.quick_access_drop_zone(menu_b.sidebar_quick_access_section_menu(inner))
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
                AnyView::new(view! {
                    column(
                        node_ref = volumes_region_ref,
                        style:display = "flex",
                        style:flex-direction = "column",
                        style:flex = "0 0 auto",
                        style:width = "100%",
                        style:min-width = "0px",
                        style:min-height = "0px",
                        style:overflow = "hidden"
                    ) {
                    SideBarSection(
                        class = "gds-files-sidebar-collapsible-section",
                        style:gap = move || Some(format!("{:.2}px", ROW_GAP * vol_progress.get()))
                    ) {
                        {{head_b.sidebar_heading(
                            "volumes",
                            COLLAPSIBLE_HEADER_HEIGHT,
                            vol_expanded,
                            is_collapsed_rail,
                            move || {
                                let focused = toggle_b.0.sidebar_region.get_untracked()
                                    == Some(SidebarRegion::Volumes)
                                    || node_contains_focus(volumes_node);
                                if transient_mode.get_untracked() {
                                    let currently_open = accordion_active.get_untracked()
                                        == Some(CollapsibleSection::Volumes)
                                        || focused;
                                    toggle_b.0.sidebar_accordion.set(if currently_open {
                                        AccordionSelection::AllCollapsed
                                    } else {
                                        AccordionSelection::Expanded(CollapsibleSection::Volumes)
                                    });
                                } else {
                                    toggle_b.edit_preferences(|p| {
                                        p.files.sidebar.volumes_collapsed =
                                            !p.files.sidebar.volumes_collapsed || focused;
                                    });
                                }
                                if focused {
                                    toggle_b.sidebar_focus_changed(SidebarRegion::Volumes, false);
                                    toggle_b.focus_content();
                                }
                            },
                        )}}
                        scroll(
                            node_ref = volumes_node,
                            class = "gds-files-sidebar-locations",
                            a11y:label = move || SharedString::from(i18n::text("volumes")),
                            style:display = move || {
                                (vol_progress.get() <= 1e-4).then(|| "none".to_owned())
                            },
                            style:height = move || {
                                Some(format!("{:.2}px", allocated_heights.get()[1]))
                            },
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
                })
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
                    node_ref = sidebar_root_ref,
                    style:display = "flex",
                    style:flex-direction = "column",
                    style:flex = "1 1 0px",
                    style:width = "100%",
                    style:height = "100%",
                    style:min-width = "0px",
                    style:min-height = "0px",
                    style:overflow = "hidden"
                ) {
                    column(
                        style:display = "flex",
                        style:flex-direction = "column",
                        style:flex = "1 1 0px",
                        style:width = "100%",
                        style:min-width = "0px",
                        style:min-height = format!("{MIN_TABS_HEIGHT:.1}px"),
                        style:overflow = "hidden"
                    ) {
                        {tabs_section()}
                    }

                    SideBarFooter(class = "gds-files-sidebar-collapsible-stack") {
                        {move || show_quick_access_sig.get().then(&quick_access_section)}
                        {move || show_volumes_sig.get().then(&volumes_section)}
                        column(
                            node_ref = locations_region_ref,
                            style:display = "flex",
                            style:flex-direction = "column",
                            style:flex = "0 0 auto",
                            style:width = "100%",
                            style:min-width = "0px",
                            style:min-height = "0px",
                            style:overflow = "hidden"
                        ) {
                            SideBarSection(
                                class = "gds-files-sidebar-collapsible-section",
                                style:gap = move || Some(format!("{:.2}px", ROW_GAP * loc_progress.get()))
                            ) {
                                {{locs_toggle.sidebar_heading(
                                    "locations",
                                    COLLAPSIBLE_HEADER_HEIGHT,
                                    loc_expanded,
                                    is_collapsed_rail,
                                    {
                                        let toggle_b = locs_browser.clone();
                                        move || {
                                            let focused = toggle_b.0.sidebar_region.get_untracked()
                                                == Some(SidebarRegion::Locations)
                                                || node_contains_focus(locations_node);
                                            if transient_mode.get_untracked() {
                                                let currently_open = accordion_active.get_untracked()
                                                    == Some(CollapsibleSection::Locations)
                                                    || focused;
                                                toggle_b.0.sidebar_accordion.set(if currently_open {
                                                    AccordionSelection::AllCollapsed
                                                } else {
                                                    AccordionSelection::Expanded(
                                                        CollapsibleSection::Locations,
                                                    )
                                                });
                                            } else {
                                                toggle_b.edit_preferences(|p| {
                                                    p.files.sidebar.locations_collapsed =
                                                        !p.files.sidebar.locations_collapsed
                                                            || focused;
                                                });
                                            }
                                            if focused {
                                                toggle_b.sidebar_focus_changed(
                                                    SidebarRegion::Locations,
                                                    false,
                                                );
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
                                                Some(format!("{:.2}px", allocated_heights.get()[2]))
                                            },
                                            style:overflow-y = move || {
                                                Some(section_overflow_y(loc_progress.get()).to_owned())
                                            },
                                            {..region::surface(locations_node)},
                                            on:focus_in = move |_| {
                                                focus_b.sidebar_focus_changed(
                                                    SidebarRegion::Locations,
                                                    true,
                                                );
                                            },
                                            on:focus_out = move |_| {
                                                if !node_contains_focus(locations_node) {
                                                    out_b.sidebar_focus_changed(
                                                        SidebarRegion::Locations,
                                                        false,
                                                    );
                                                }
                                            }
                                        ) {
                                            column(
                                                class = "gds-files-sidebar-curtain",
                                                style:opacity = move || {
                                                    Some(format!(
                                                        "{:.3}",
                                                        loc_progress.get().clamp(0.0, 1.0)
                                                    ))
                                                },
                                                style:transform = move || {
                                                    section_curtain_transform(loc_progress.get())
                                                }
                                            ) {
                                                for entry in move || {
                                                    let tags_enabled = sections.get().2;
                                                    LocationTarget::ALL
                                                        .iter()
                                                        .copied()
                                                        .enumerate()
                                                        .filter(|(_, target)| {
                                                            tags_enabled
                                                                || !matches!(
                                                                    target,
                                                                    LocationTarget::Collection(
                                                                        Collection::Tags
                                                                    )
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
                                                            Some(path) => rows_b.file_drag_target(
                                                                item,
                                                                path.to_owned(),
                                                            ),
                                                            None => rows_b
                                                                .unsupported_file_drop_target(item),
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
            }
        })
    }

    fn sidebar_quick_access_item(&self, path: PathBuf) -> AnyView {
        let label = quick_access_label(&path);
        let icon = FileIcon::directory_icon(&path);
        let cursor = self.0.sidebar_cursor;
        let region_sig = self.0.sidebar_region;
        let quick_access_node = self.0.quick_access_node;
        let open_click = self.clone();
        let open_aux = self.clone();
        let path_click = path.clone();
        let path_aux = path.clone();
        let selected = Signal::derive_local({
            let browser = self.clone();
            let path = path.clone();
            move || {
                browser.pane() == Pane::Files
                    && browser.read(|state| state.tab().navigation.path() == Some(path.as_path()))
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
                    open_click.open_quick_access(&path_click, ev.modifiers.primary());
                },
                on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                    if ev.button == Some(PointerButton::Middle) {
                        ev.stop_propagation();
                        open_aux.open_quick_access(&path_aux, true);
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
        let drop_wrapped = self.file_drag_target(tipped, path.clone());
        let dragged = self.draggable_quick_access(drop_wrapped, path.clone(), label, icon);
        self.sidebar_quick_access_menu(dragged, path)
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
            None => self.unsupported_file_drop_target(tipped),
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

    pub(super) fn sidebar_tab_title(&self, location: &Location) -> String {
        if let Location::Volume { identity, relative } = location
            && relative.is_empty()
            && let Some(volume) = self.volume_at_location(identity)
        {
            return volume.name;
        }
        self.location_title(location)
    }

    pub(super) fn tab_drag_snapshot(&self, id: u64) -> Option<(IconName, String, String, bool)> {
        self.read_untracked(|state| {
            let active_id = state.tab().id;
            let tab = state.tabs.iter().find(|tab| tab.id == id)?;
            let location = tab.navigation.location();
            let icon = location.icon();
            let title = self.sidebar_tab_title(location);
            let detail_path = tab
                .navigation
                .path()
                .map_or_else(|| location.label(), |path| path.display().to_string());
            Some((icon, title, detail_path, active_id == id))
        })
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

fn attach_filter_focus_effect(
    filter_node: NodeRef,
    filter_sig: RwSignal<String, LocalStorage>,
    focus_request: RwSignal<Option<FilterFocusMode>, LocalStorage>,
) {
    let focus_fx = RenderEffect::new(move |_| {
        let Some(mode) = focus_request.get() else {
            return;
        };
        if filter_node.get().is_none() {
            return;
        }
        focus_request.set(None);
        let current = filter_sig.get_untracked();
        filter_node.focus();
        filter_node.set_value(&current);
        match mode {
            FilterFocusMode::SelectAll => filter_node.select_all(),
            FilterFocusMode::CaretEnd => filter_node.set_selection(current.len()..current.len()),
        }
    });
    on_cleanup_local(move || drop(focus_fx));
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
