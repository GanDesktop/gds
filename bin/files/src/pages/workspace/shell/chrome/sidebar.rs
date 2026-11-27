//! Sidebar layout, collapsible section accordion allocation, and section headers.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gds::appearance::motion::{WORKSPACE_DURATION_MS, use_transition};
use gds::appearance::tokens::{
    COLLAPSED_SIDEBAR_WIDTH, ICON_BUTTON_SIZE, SIDEBAR_ITEM_HEIGHT, SIDEBAR_PADDING,
    SIDEBAR_SECTION_PADDING,
};
use gds::i18n;
use gds::ui::IconName;
use gds::ui::button::{ButtonSize, IconButtonProps};
use gds::ui::capacity::EXTRA_ROW_HEIGHT;
use gds::ui::disclosure::DisclosureIconProps;
use gds::ui::feedback::tooltip::{TOOLTIP_DELAY, TooltipTextProps};
use gds::ui::interaction::{keytips, region};
use gds::ui::sidebar::{
    SideBarFooterProps, SideBarProps, SideBarSectionContentProps, SideBarSectionHeaderProps,
    SideBarSectionProps, SideBarSectionTitleProps, SidebarPresentation,
};
use gds::ui::window::HeightTier;
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, on_cleanup_local};
use zgui::view::{Scoped, events, install_stylesheet as install_sheet};
use zgui::vocab::{Key, Modifiers, NamedKey, Role, SharedString};
use zgui::{css, view};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipTriggerProps};
use zgui_ui_primitives::Binding;

use super::{Chrome, install_chrome_stylesheets, node_contains_focus};
use crate::entities::location::{Collection, Location, LocationTarget};
use crate::pages::workspace::{
    navigation::{Navigator, QuickAccess},
    panes::{DragHost, MenuHost},
    projection::{Geometry, Rows, UiState, geometry::ITEM_GAP},
    state::{
        context::{Context, local_memo},
        ui_flags::{AccordionSelection, CollapsibleSection, SidebarRegion},
    },
};
use crate::shared::ui::actions::Command;

mod items;

#[cfg(test)]
pub(super) use items::{TabCycleTargets, compute_tab_cycle_targets};

const MIN_TABS_HEIGHT: f32 = 94.0;
const COLLAPSIBLE_HEADER_HEIGHT: f32 = 28.0;
const SECTION_STACK_GAP: f32 = 6.0;
const EMPTY_SECTION_HEIGHT: f32 = 32.0;
const GROUP_GAP: f32 = 6.0;
const ROW_GAP: f32 = 4.0;
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

const SIDEBAR_SHEET: &str = "gds-files-chrome-sidebar";

const SIDEBAR_CSS: &str = css!(
    r#"
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
    "#
);

pub(super) fn install_stylesheet() {
    install_sheet(SIDEBAR_SHEET, SIDEBAR_CSS);
    items::install_stylesheet();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CollapsibleMeta {
    pub visible: bool,
    pub pref_expanded: bool,
    pub has_items: bool,
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

fn default_volumes_body_height(visible_count: usize, mounted_count: usize) -> f32 {
    if visible_count == 0 {
        return EMPTY_SECTION_HEIGHT;
    }
    let rows = visible_count.min(DEFAULT_DEVICE_ROWS);
    let mounted = mounted_count.min(rows);
    (rows as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP) + mounted as f32 * EXTRA_ROW_HEIGHT - ROW_GAP)
        .max(EMPTY_SECTION_HEIGHT)
}

impl Chrome<'_> {
    pub(in crate::pages::workspace) fn sidebar(
        &self,
        _presentation: SidebarPresentation,
    ) -> AnyView {
        sidebar(self.0, _presentation)
    }
}

fn sidebar(ctx: &Context, _presentation: SidebarPresentation) -> AnyView {
    let ctx = ctx.clone();
    AnyView::new(Scoped::new(move || sidebar_scoped(&ctx)))
}

fn sidebar_scoped(ctx: &Context) -> AnyView {
    install_chrome_stylesheets();
    let presentation = ctx.sidebar_presentation();
    let is_collapsed_rail = local_memo(move || presentation.get().is_collapsed());
    let actions_visible = local_memo(move || presentation.get().actions_visible());
    let compact_capacity =
        local_memo(move || presentation.get().width() < COMPACT_CAPACITY_THRESHOLD);

    let sections = local_memo({
        let ctx = ctx.clone();
        move || {
            let preferences = ctx.0.preferences.state().get();
            (
                preferences.files.sidebar.show_quick_access
                    || ctx.0.sidebar_reveal_quick_access.get(),
                preferences.files.sidebar.show_volumes || ctx.0.sidebar_reveal_volumes.get(),
                preferences.tags.enabled,
            )
        }
    });
    let show_quick_access_sig = local_memo(move || sections.get().0);
    let show_volumes_sig = local_memo(move || sections.get().1);

    let quick_access_paths = local_memo({
        let c = ctx.clone();
        move || QuickAccess(&c).quick_access_paths()
    });
    let volumes_counts = local_memo({
        let c = ctx.clone();
        move || {
            let snapshot = c.0.volumes.snapshot();
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
            .filter(|t| tags_enabled || !matches!(t, LocationTarget::Collection(Collection::Tags)))
            .count()
    });

    let sidebar_root_ref = ctx.0.sidebar_content_node;
    let sidebar_size = sidebar_root_ref.observe_border_size();
    let sidebar_height = Signal::derive_local({
        let win_h = ctx.0.window_height;
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
        let c = ctx.clone();
        move || Geometry(&c).height_tier()
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

    let accordion_sig = ctx.0.sidebar_accordion;
    let in_accordion_sig = ctx.0.sidebar_in_accordion;
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
        let c = ctx.clone();
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
            match c.0.sidebar_accordion.get() {
                AccordionSelection::Expanded(sec) if is_visible(sec) => Some(sec),
                AccordionSelection::AllCollapsed => None,
                AccordionSelection::Auto | AccordionSelection::Expanded(_) => {
                    if single_micro_collapse.get() {
                        return None;
                    }
                    let (qa_pref, vol_pref, loc_pref) = c.0.preferences.state().with(|p| {
                        (
                            !p.files.sidebar.quick_access_collapsed,
                            !p.files.sidebar.volumes_collapsed,
                            !p.files.sidebar.locations_collapsed,
                        )
                    });
                    let focused =
                        c.0.sidebar_region
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
        let c = ctx.clone();
        move || {
            if !show_quick_access_sig.get() {
                return false;
            }
            let focused = c.0.sidebar_region.get() == Some(SidebarRegion::QuickAccess);
            if transient_mode.get() {
                accordion_active.get() == Some(CollapsibleSection::QuickAccess) || focused
            } else {
                !c.0.preferences
                    .state()
                    .with(|p| p.files.sidebar.quick_access_collapsed)
                    || focused
            }
        }
    });
    let vol_expanded = Signal::derive_local({
        let c = ctx.clone();
        move || {
            if !show_volumes_sig.get() {
                return false;
            }
            let focused = c.0.sidebar_region.get() == Some(SidebarRegion::Volumes);
            if transient_mode.get() {
                accordion_active.get() == Some(CollapsibleSection::Volumes) || focused
            } else {
                !c.0.preferences
                    .state()
                    .with(|p| p.files.sidebar.volumes_collapsed)
                    || focused
            }
        }
    });
    let loc_expanded = Signal::derive_local({
        let c = ctx.clone();
        move || {
            let focused = c.0.sidebar_region.get() == Some(SidebarRegion::Locations);
            if transient_mode.get() {
                accordion_active.get() == Some(CollapsibleSection::Locations) || focused
            } else {
                !c.0.preferences
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
            (qa_count as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP) - ROW_GAP).max(EMPTY_SECTION_HEIGHT)
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
    let locations_region_ref = ctx.0.locations_region_node;
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
        let tabs_c = ctx.clone();
        let focus_c = ctx.clone();
        let out_c = ctx.clone();
        let new_tab_c = ctx.clone();
        let tabs_node = ctx.0.tabs_node;
        move || {
            let tabs_c = tabs_c.clone();
            let focus_c = focus_c.clone();
            let out_c = out_c.clone();
            let new_tab_clk = new_tab_c.clone();
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
                                Navigator(&new_tab_clk).new_tab(None);
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
                                UiState(&focus_c).sidebar_focus_changed(SidebarRegion::Tabs, true);
                            },
                            on:focus_out = move |_| {
                                if !node_contains_focus(tabs_node) {
                                    UiState(&out_c).sidebar_focus_changed(SidebarRegion::Tabs, false);
                                }
                            }
                        ) {
                            {{
                                let rows_c = tabs_c.clone();
                                let specs_c = tabs_c.clone();
                                let tab_specs = local_memo(move || {
                                    specs_c.read_structure(|state| {
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
                                        {{items::sidebar_tab(&rows_c, spec.0, spec.1, spec.2, is_collapsed_rail, actions_visible)}}
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
        let qa_c = ctx.clone();
        let quick_access_node = ctx.0.quick_access_node;
        move || {
            let toggle_c = qa_c.clone();
            let focus_c = qa_c.clone();
            let out_c = qa_c.clone();
            let rows_c = qa_c.clone();
            let menu_c = qa_c.clone();
            let drop_c = qa_c.clone();

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
                        {{sidebar_heading(
                            "quick-access",
                            COLLAPSIBLE_HEADER_HEIGHT,
                            qa_expanded,
                            is_collapsed_rail,
                            move || {
                                let focused = toggle_c.0.sidebar_region.get_untracked()
                                    == Some(SidebarRegion::QuickAccess)
                                    || node_contains_focus(quick_access_node);
                                if transient_mode.get_untracked() {
                                    let currently_open = accordion_active.get_untracked()
                                        == Some(CollapsibleSection::QuickAccess)
                                        || focused;
                                    toggle_c.0.sidebar_accordion.set(if currently_open {
                                        AccordionSelection::AllCollapsed
                                    } else {
                                        AccordionSelection::Expanded(
                                            CollapsibleSection::QuickAccess,
                                        )
                                    });
                                } else {
                                    toggle_c.edit_preferences(|p| {
                                        p.files.sidebar.quick_access_collapsed =
                                            !p.files.sidebar.quick_access_collapsed || focused;
                                    });
                                }
                                if focused {
                                    UiState(&toggle_c).sidebar_focus_changed(
                                        SidebarRegion::QuickAccess,
                                        false,
                                    );
                                    toggle_c.focus_content();
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
                                UiState(&focus_c).sidebar_focus_changed(SidebarRegion::QuickAccess, true);
                            },
                            on:focus_out = move |_| {
                                if !node_contains_focus(quick_access_node) {
                                    UiState(&out_c).sidebar_focus_changed(
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
                                            .map(|path| items::sidebar_quick_access_item(&rows_c, path))
                                            .collect::<Vec<_>>()
                                    }
                                }}}
                            }
                        }
                    }
                }
            });
            DragHost(&drop_c)
                .quick_access_drop_zone(MenuHost(&menu_c).sidebar_quick_access_section_menu(inner))
        }
    };

    let volumes_section = {
        let vol_c = ctx.clone();
        let volumes_node = ctx.0.volumes_node;
        let active_volume = local_memo({
            let c = ctx.clone();
            move || {
                c.read(|state| match state.tab().navigation.location() {
                    Location::Volume { identity, .. } => Rows(&c)
                        .volume_at_location(identity)
                        .map(|volume| volume.id),
                    _ => None,
                })
            }
        });
        move || {
            let toggle_c = vol_c.clone();
            let focus_c = vol_c.clone();
            let out_c = vol_c.clone();
            let rows_c = vol_c.clone();
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
                    {{sidebar_heading(
                        "volumes",
                        COLLAPSIBLE_HEADER_HEIGHT,
                        vol_expanded,
                        is_collapsed_rail,
                        move || {
                            let focused = toggle_c.0.sidebar_region.get_untracked()
                                == Some(SidebarRegion::Volumes)
                                || node_contains_focus(volumes_node);
                            if transient_mode.get_untracked() {
                                let currently_open = accordion_active.get_untracked()
                                    == Some(CollapsibleSection::Volumes)
                                    || focused;
                                toggle_c.0.sidebar_accordion.set(if currently_open {
                                    AccordionSelection::AllCollapsed
                                } else {
                                    AccordionSelection::Expanded(CollapsibleSection::Volumes)
                                });
                            } else {
                                toggle_c.edit_preferences(|p| {
                                    p.files.sidebar.volumes_collapsed =
                                        !p.files.sidebar.volumes_collapsed || focused;
                                });
                            }
                            if focused {
                                UiState(&toggle_c).sidebar_focus_changed(SidebarRegion::Volumes, false);
                                toggle_c.focus_content();
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
                            UiState(&focus_c).sidebar_focus_changed(SidebarRegion::Volumes, true);
                        },
                        on:focus_out = move |_| {
                            if !node_contains_focus(volumes_node) {
                                UiState(&out_c).sidebar_focus_changed(SidebarRegion::Volumes, false);
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
                                let snapshot = rows_c.0.volumes.snapshot();
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
                                            items::sidebar_volume(
                                                &rows_c,
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

    let locations_node = ctx.0.locations_node;
    let locs_ctx = ctx.clone();

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
                                {{sidebar_heading(
                                    "locations",
                                    COLLAPSIBLE_HEADER_HEIGHT,
                                    loc_expanded,
                                    is_collapsed_rail,
                                    {
                                        let toggle_c = locs_ctx.clone();
                                        move || {
                                            let focused = toggle_c.0.sidebar_region.get_untracked()
                                                == Some(SidebarRegion::Locations)
                                                || node_contains_focus(locations_node);
                                            if transient_mode.get_untracked() {
                                                let currently_open = accordion_active.get_untracked()
                                                    == Some(CollapsibleSection::Locations)
                                                    || focused;
                                                toggle_c.0.sidebar_accordion.set(if currently_open {
                                                    AccordionSelection::AllCollapsed
                                                } else {
                                                    AccordionSelection::Expanded(
                                                        CollapsibleSection::Locations,
                                                    )
                                                });
                                            } else {
                                                toggle_c.edit_preferences(|p| {
                                                    p.files.sidebar.locations_collapsed =
                                                        !p.files.sidebar.locations_collapsed
                                                            || focused;
                                                });
                                            }
                                            if focused {
                                                UiState(&toggle_c).sidebar_focus_changed(
                                                    SidebarRegion::Locations,
                                                    false,
                                                );
                                                toggle_c.focus_content();
                                            }
                                        }
                                    },
                                )}}
                                {{
                                    let focus_c = locs_ctx.clone();
                                    let out_c = locs_ctx.clone();
                                    let rows_c = locs_ctx.clone();
                                    let home = rows_c.read_untracked(|state| state.home.clone());
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
                                                UiState(&focus_c).sidebar_focus_changed(
                                                    SidebarRegion::Locations,
                                                    true,
                                                );
                                            },
                                            on:focus_out = move |_| {
                                                if !node_contains_focus(locations_node) {
                                                    UiState(&out_c).sidebar_focus_changed(
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
                                                        let item = items::sidebar_location(
                                                            &rows_c,
                                                            target,
                                                            loc.clone(),
                                                        );
                                                        match loc.local_path() {
                                                            Some(path) => DragHost(&rows_c)
                                                                .file_drag_target(
                                                                    item,
                                                                    path.to_owned(),
                                                                ),
                                                            None => DragHost(&rows_c)
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

fn sidebar_heading(
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
    super::tests::record_heading_node(label, heading_ref);

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

fn section_overflow_y(progress: f32) -> &'static str {
    if progress < 0.999 { "hidden" } else { "auto" }
}

fn section_curtain_transform(progress: f32) -> Option<String> {
    let p = progress.clamp(0.0, 1.0);
    ((1.0 - p) > 1e-3).then(|| format!("translateY({:.2}px)", -(1.0 - p) * 8.0))
}
