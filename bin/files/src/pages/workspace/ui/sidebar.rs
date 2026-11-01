//! Locations stay at the bottom; tabs and devices divide the remaining height.

use super::*;
use gds::appearance::tokens;
use gds::ui::sidebar::*;
use gds::{i18n::t, ui::button::Button};
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Icon, Sizable, button::ButtonVariants},
    prelude::*,
};

pub(super) const MIN_TABS_HEIGHT: f32 = 94.;
pub(super) const MIN_DEVICES_HEIGHT: f32 = 108.;
const HEADER_HEIGHT: f32 = 34.;
const LOCATION_HEADER_HEIGHT: f32 = 28.;
const GROUP_GAP: f32 = 6.;
const ROW_GAP: f32 = 4.;
const DEFAULT_DEVICE_ROWS: usize = 4;

impl Browser {
    pub(super) fn on_sidebar_split(
        &mut self,
        _: Entity<ui::split::State>,
        event: &ui::split::Changed,
        cx: &mut Context<Self>,
    ) {
        self.preferences.files.sidebar.tabs_height = Some(event.0);
        self.save_sidebar(cx);
    }

    fn save_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = configuration::save(&self.preferences) {
            report_settings_failure("settings-save-failed", &error, cx);
        }
        cx.notify();
    }

    fn toggle_devices(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.regions.volumes.contains_focused(window, cx);
        self.preferences.files.sidebar.volumes_collapsed =
            !self.preferences.files.sidebar.volumes_collapsed || focused;
        if focused {
            self.focus.focus(window, cx);
        }
        self.save_sidebar(cx);
    }

    fn device_heading(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let collapsed = self.preferences.files.sidebar.volumes_collapsed
            && !self.regions.volumes.is_focused(window)
            && !self.regions.volumes.contains_focused(window, cx);
        let on_click = cx.listener(Self::toggle_devices);
        let heading = el! {
            .div { flex; items_center; gap: GROUP_GAP.px; w_full; min_w_0;
                @if (self.sidebar_collapsed) justify_center; @end;
            } [
                $(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown }).Icon::new { size: 12.0.px },
                @if (!self.sidebar_collapsed) cx.t!("volumes").SideBarSectionTitle::new, @end,
            ]
        };
        el! {
            .SideBarSectionHeader::default { h: HEADER_HEIGHT.px } [
                "volumes".Button::new { ghost; small; flex_1; min_w_0; justify_start;
                    accessibility_label: t!(@cx, "volumes"); on_click: on_click;
                    aria_expanded: $(!collapsed);
                    @if (self.sidebar_collapsed) tooltip: t!(@cx, "volumes"); @end;
                } [heading],
            ]
        }
        .into_any_element()
    }

    fn sidebar_tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let cycle_targets = self.tab_cycle_targets();
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(position, slot)| self.render_tab(slot, position, cycle_targets, window, cx))
            .collect();
        let on_new_tab = cx.listener(Self::click_command(Command::NewTab));
        let content = el! {
            .SideBarSection::default { size_full; min_h_0; px: tokens::SIDEBAR_PADDING.px; pt: GROUP_GAP.px } [
                .SideBarSectionHeader::default { h: HEADER_HEIGHT.px;
                    @if (self.sidebar_collapsed) justify_center; @end;
                } [
                    @if (!self.sidebar_collapsed)
                        cx.t!("tabs").ui::sidebar::SideBarSectionTitle::new { flex_1; px: tokens::SIDEBAR_SECTION_PADDING.px },
                    @end,
                    "new-tab".ui::IconButton::new { icon: IconName::Plus; label: t!(@cx, "new-tab");
                        command: Command::NewTab; on_click: on_new_tab;
                    },
                ],
                .SideBarSectionContent::default { # "vertical-tabs"; gap: ROW_GAP.px; flex_1; min_h_0; overflow_y_scroll;
                    track_scroll: $&self.regions.tabs_scroll;
                } [..tabs],
            ]
        };
        ui::region::surface(
            "tabs-region",
            &self.regions.tabs,
            Command::FocusTabs,
            window,
            cx,
        )
        .flex()
        .flex_col()
        .size_full()
        .child(content)
        .into_any_element()
    }

    fn toggle_locations(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.regions.locations.is_focused(window)
            || self.regions.locations.contains_focused(window, cx);
        self.preferences.files.sidebar.locations_collapsed =
            !self.preferences.files.sidebar.locations_collapsed || focused;
        if focused {
            self.focus.focus(window, cx);
        }
        self.save_sidebar(cx);
    }

    fn sidebar_locations(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let expanded = !self.preferences.files.sidebar.locations_collapsed
            || self.regions.locations.is_focused(window)
            || self.regions.locations.contains_focused(window, cx);
        let on_toggle = cx.listener(Self::toggle_locations);
        let heading = {
            let label = el! {
                .div { flex; items_center; gap: GROUP_GAP.px; w_full; min_w_0;
                    @if (self.sidebar_collapsed) justify_center; @end;
                } [
                    $(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }).Icon::new { size: 12.0.px; },
                    @if (!self.sidebar_collapsed) cx.t!("locations").SideBarSectionTitle::new, @end,
                ]
            };
            el! {
                .SideBarSectionHeader::default { h: LOCATION_HEADER_HEIGHT.px; } [
                    "locations-toggle".Button::new { ghost; small; flex_1; min_w_0; justify_start;
                        accessibility_label: t!(@cx, "locations"); aria_expanded: expanded;
                        on_click: on_toggle;
                        @if (self.sidebar_collapsed) tooltip: t!(@cx, "locations"); @end;
                    } [label],
                ]
            }
        };
        let content = expanded.then(|| {
            let locations = LocationTarget::ALL
                .into_iter()
                .map(|target| {
                    let location = self.render_location(target, window, cx);
                    el! { .div { flex_shrink_0; w_full; } [location] }
                })
                .collect::<Vec<_>>();
            let rows = el! {
                .SideBarSectionContent::default { # "locations-list"; min_h_0; overflow_y_scroll;
                    track_scroll: $&self.regions.locations_scroll; gap: ROW_GAP.px;
                } [..locations]
            };
            ui::region::surface(
                "locations-region",
                &self.regions.locations,
                Command::FocusLocations,
                window,
                cx,
            )
            .flex()
            .flex_col()
            .w_full()
            .min_h_0()
            .flex_shrink_1()
            .child(rows)
        });
        el! {
            .SideBarSection::default { # "sidebar-locations"; flex_shrink_1; min_h_0; overflow_hidden;
                px: tokens::SIDEBAR_PADDING.px; pb: tokens::SIDEBAR_PADDING.px; gap: ROW_GAP.px;
            } [heading, ..content]
        }.into_any_element()
    }

    pub(super) fn render_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tabs = self.sidebar_tabs(window, cx);
        let locations = self.sidebar_locations(window, cx);
        let show_volumes =
            self.preferences.files.sidebar.show_volumes || self.regions.reveal_volumes;
        let devices = if show_volumes {
            self.render_device_groups(true, window, cx)
        } else {
            Vec::new()
        };
        let device_row_count = devices.len();
        let mounted_count = self
            .storage
            .read(cx)
            .snapshot
            .volumes
            .iter()
            .filter(|volume| volume.visible && !volume.mount_points.is_empty())
            .count();
        let expanded = !self.preferences.files.sidebar.volumes_collapsed
            || self.regions.volumes.is_focused(window)
            || self.regions.volumes.contains_focused(window, cx);
        let heading = self.device_heading(window, cx);
        // A large terminal dock must not let the fixed locations consume every
        // pixel of the tab strip. Locations scroll only when space is scarce.
        let workspace_minimum = MIN_TABS_HEIGHT
            + if show_volumes && expanded {
                MIN_DEVICES_HEIGHT + ui::split::DIVIDER_HEIGHT
            } else if show_volumes {
                HEADER_HEIGHT
            } else {
                0.
            };
        let workspace = if show_volumes && expanded {
            let device_area = el! {
                .SideBarSection::default { size_full; min_h_0; px: tokens::SIDEBAR_PADDING.px } [
                    heading,
                    .SideBarSectionContent::default { # "sidebar-devices"; flex_1; min_h_0; overflow_y_scroll;
                        gap: ROW_GAP.px; pb: ROW_GAP.px;
                        track_scroll: $&self.regions.volumes_scroll;
                    } [..devices,
                        @if (device_row_count == 0)
                            .div { px: GROUP_GAP.px; text_xs; truncate; text_color: $cx.theme().muted_foreground;
                                @if (self.sidebar_collapsed) invisible; @end;
                            } [t!(@cx, "device-none")],
                        @end,
                    ],
                ]
            }
            .into_any_element();
            let device_area = ui::region::surface(
                "volumes-region",
                &self.regions.volumes,
                Command::FocusVolumes,
                window,
                cx,
            )
            .flex()
            .flex_col()
            .size_full()
            .child(device_area)
            .into_any_element();
            let default_bottom = device_row_count.min(DEFAULT_DEVICE_ROWS) as f32
                * (tokens::SIDEBAR_ITEM_HEIGHT + ROW_GAP)
                + mounted_count.min(DEFAULT_DEVICE_ROWS) as f32 * ui::capacity::EXTRA_ROW_HEIGHT
                + HEADER_HEIGHT;
            self.sidebar_split
                .update(cx, |state, _| state.default_bottom(default_bottom));
            ui::split::VerticalSplit::new(self.sidebar_split.clone())
                .top(tabs)
                .bottom(device_area)
                .label(i18n::text("sidebar-resize", cx))
                .into_any_element()
        } else {
            el! {
                .div { flex; flex_col; size_full; min_h_0 } [
                    .div { flex_1; min_h_0; overflow_hidden } [tabs],
                    @if (show_volumes)
                        .div { flex_shrink_0; px: tokens::SIDEBAR_PADDING.px } [heading],
                    @end,
                ]
            }
            .into_any_element()
        };
        let state = if self.sidebar_collapsed {
            SidebarState::Collapsed
        } else {
            SidebarState::Expanded
        };
        el! {
            .SideBar::default { # "file-sidebar-content"; state: state; } [
                .SideBarContent::default { # "file-sidebar-workspace"; min_h: workspace_minimum.px; overflow_hidden } [workspace],
                .SideBarFooter::default { flex_shrink_1; min_h_0; overflow_hidden } [locations],
            ]
        }
        .into_any_element()
    }
}
