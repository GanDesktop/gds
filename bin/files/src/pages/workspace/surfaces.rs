//! Page-specific table metadata uses each domain's own values and service state.

mod network;

use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::SystemTime;

use gds::service::remote;
use gds::ui::button::{ButtonSize, ButtonVariant};
use gds::ui::capacity::CapacityBarProps;
use gds::ui::feedback::tooltip::{HintTooltipProps, wrap_tokens};
use gds::ui::menu::MenuFocusParkProps;
use gds::ui::page::PageNoticeProps;
use gds::ui::time::Timestamp;
use gds::ui::{IconButtonProps, IconName, IconProps};
use gds::{i18n, t};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Signal};
use zgui::view::{current_host, install_stylesheet};
use zgui::vocab::{Role, SharedString};
use zgui::{css, view};
use zgui_ui::{
    ButtonProps, DropdownMenuContentProps, DropdownMenuProps, DropdownMenuTriggerProps,
    MenuCheckboxItemProps, SpinnerProps,
};
use zgui_ui_primitives::Binding;

use crate::entities::file::{
    columns::Column,
    directory::{ContentType, EntryKind},
    sorting::Direction,
    ui::{EntryPresentation, FileIcon, UNKNOWN_VALUE},
};
use crate::entities::location::{Location, scheme::TagAddress, volumes};

use super::{
    model::{Contents, Row},
    ui::Browser,
};

const SURFACES_SHEET: &str = "gds-files-workspace-surfaces";
pub(super) const COLLECTION_MAX_COLUMNS: usize = 5;

pub(super) const fn collection_column_class(index: usize) -> &'static str {
    match index {
        0 => "gds-files-col-0",
        1 => "gds-files-col-1",
        2 => "gds-files-col-2",
        3 => "gds-files-col-3",
        _ => "gds-files-col-4",
    }
}

const SURFACES_CSS: &str = css!(
    r#"
    .gds-files-collection-state {
        flex: 1 1 0%;
        min-height: 220px;
        align-items: center;
        justify-content: center;
        gap: 10px;
        padding: 24px;
    }

    .gds-files-collection-state__icon {
        width: 32px;
        height: 32px;
        color: var(--gds-muted-foreground);
    }

    .gds-files-collection-state__title {
        font-size: var(--gds-text-md, 16px);
        font-weight: 500;
        color: var(--gds-foreground);
        text-align: center;
    }

    .gds-files-collection-state__detail {
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-muted-foreground);
        text-align: center;
    }

    .gds-files-collection-notice {
        align-items: center;
        justify-content: space-between;
        gap: 10px;
    }

    .gds-files-collection-notice--error {
        border-color: var(--gds-error);
    }

    .gds-files-collection-notice__lead {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0;
        flex: 1 1 0%;
    }

    .gds-files-collection-notice__text {
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
    }

    .gds-files-collection-header {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        width: 100%;
        height: var(--gds-list-header-height, 30px);
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-files-collection-header__cell {
        display: flex;
        flex-direction: row;
        align-items: center;
        align-self: stretch;
        min-width: 0;
        flex-shrink: 0;
        box-sizing: border-box;
    }

    .gds-files-collection-header__static {
        display: flex;
        flex-direction: row;
        align-items: center;
        align-self: stretch;
        flex: 1 1 0px;
        min-width: 0;
        padding: 0 12px;
    }

    .gds-files-collection-header__static[data-name-col="true"] {
        padding-left: 40px;
    }

    .gds-files-collection-header__static[data-align="end"] {
        justify-content: flex-end;
        text-align: right;
    }

    .gds-files-collection-header__button {
        display: flex;
        flex-direction: row;
        align-items: center;
        align-self: stretch;
        flex: 1 1 0px;
        min-width: 0;
        gap: 4px;
        padding: 0 8px 0 12px;
        cursor: pointer;
    }

    .gds-files-collection-header__button[data-name-col="true"] {
        padding-left: 40px;
    }

    .gds-files-collection-header__button[data-align="end"] {
        justify-content: flex-end;
        text-align: right;
    }

    .gds-files-collection-header__button:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-collection-header__label {
        min-width: 0;
        font-size: var(--gds-text-xs, 12px);
        font-weight: 400;
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-collection-header__button:not([data-align="end"]) .gds-files-collection-header__label,
    .gds-files-collection-header__static:not([data-align="end"]) .gds-files-collection-header__label {
        flex: 1 1 0px;
    }

    .gds-files-collection-header__button[data-active="true"] .gds-files-collection-header__label {
        color: var(--gds-foreground);
    }

    .gds-files-collection-header__sort {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        width: 12px;
        height: 18px;
        flex-shrink: 0;
        opacity: 0;
        color: var(--gds-muted-foreground);
    }

    .gds-files-collection-header__button:hover .gds-files-collection-header__sort,
    .gds-files-collection-header__button:focus-visible .gds-files-collection-header__sort {
        opacity: 1;
    }

    .gds-files-collection-header__sort[data-active="true"] {
        opacity: 1;
        color: var(--gds-foreground);
    }

    .gds-files-collection-cell {
        display: flex;
        flex-direction: row;
        align-items: center;
        align-self: stretch;
        gap: 10px;
        min-width: 0;
        flex-shrink: 0;
        padding: 0 12px;
        overflow: hidden;
        box-sizing: border-box;
    }

    .gds-files-col-0.gds-files-col-0 {
        flex: none;
        width: var(--gds-files-col-0);
        min-width: var(--gds-files-col-0);
        max-width: var(--gds-files-col-0);
        box-sizing: border-box;
    }

    .gds-files-col-1.gds-files-col-1 {
        flex: none;
        width: var(--gds-files-col-1);
        min-width: var(--gds-files-col-1);
        max-width: var(--gds-files-col-1);
        box-sizing: border-box;
    }

    .gds-files-col-2.gds-files-col-2 {
        flex: none;
        width: var(--gds-files-col-2);
        min-width: var(--gds-files-col-2);
        max-width: var(--gds-files-col-2);
        box-sizing: border-box;
    }

    .gds-files-col-3.gds-files-col-3 {
        flex: none;
        width: var(--gds-files-col-3);
        min-width: var(--gds-files-col-3);
        max-width: var(--gds-files-col-3);
        box-sizing: border-box;
    }

    .gds-files-col-4.gds-files-col-4 {
        flex: none;
        width: var(--gds-files-col-4);
        min-width: var(--gds-files-col-4);
        max-width: var(--gds-files-col-4);
        box-sizing: border-box;
    }

    .gds-files-collection-cell > * {
        min-width: 0;
        max-width: 100%;
    }

    .gds-files-collection-cell--meta {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-collection-cell--split {
        justify-content: space-between;
    }

    .gds-files-collection-cell[data-align="end"] {
        justify-content: flex-end;
        text-align: right;
    }

    .gds-files-collection-artwork {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 18px;
        height: 18px;
        flex-shrink: 0;
        overflow: visible;
        color: var(--gds-muted-foreground);
    }

    .gds-files-collection-artwork[data-directory="true"] {
        color: var(--gds-primary);
    }

    .gds-files-collection-artwork[data-selected="true"] {
        color: var(--gds-selection-foreground);
    }

    .gds-files-collection-artwork__badge {
        position: absolute;
        right: -4px;
        bottom: -4px;
        background: transparent;
        color: inherit;
        padding: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        pointer-events: none;
    }

    .gds-files-collection-cell__name {
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
        flex: 1 1 0%;
    }

    .gds-files-row-content[data-selected="true"] .gds-files-collection-cell__name {
        color: var(--gds-selection-foreground);
    }

    .gds-files-collection-cell__text {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
        width: 100%;
    }

    .gds-files-collection-cell__text[data-align="end"] {
        text-align: right;
    }

    .gds-files-collection-cell__text[data-emphasis="true"] {
        color: var(--gds-foreground);
    }

    .gds-files-row-content[data-selected="true"] .gds-files-collection-cell--meta,
    .gds-files-row-content[data-selected="true"] .gds-files-collection-cell__text,
    .gds-files-row-content[data-selected="true"] .gds-files-collection-cell__badge {
        color: var(--gds-selection-foreground);
    }

    .gds-files-collection-cell__badge {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        flex-shrink: 0;
    }

    .gds-files-collection-mount-picker {
        max-width: 100%;
        justify-content: space-between;
        gap: 4px;
        padding: 2px 6px;
        height: 24px;
    }

    .gds-files-collection-mount-picker__label {
        font-size: var(--gds-text-xs, 12px);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
    }
    "#
);

fn install_surfaces_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(SURFACES_SHEET, SURFACES_CSS);
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct CollectionBodyState {
    pub icon: Option<IconName>,
    pub title: String,
    pub detail: Option<String>,
    pub retry: bool,
    pub loading: bool,
    pub reconnect: bool,
}

impl CollectionBodyState {
    pub(super) fn view(self, browser: &Browser) -> AnyView {
        install_surfaces_stylesheet();
        let retry_browser = browser.clone();
        let reconnect_browser = browser.clone();
        let title_for_spinner = self.title.clone();
        let status_mark = if self.loading {
            Some(AnyView::new(view! { Spinner(label = title_for_spinner) }))
        } else {
            self.icon.map(|icon| {
                AnyView::new(view! {
                    Icon(name = icon, class = "gds-files-collection-state__icon")
                })
            })
        };

        let detail_view = self.detail.map(|detail| {
            view! {
                text(class = "gds-files-collection-state__detail") {{detail}}
            }
        });

        let action_view = if self.reconnect {
            Some(AnyView::new(view! {
                Button(
                    variant = ButtonVariant::Outline,
                    on:click = move |_| reconnect_browser.reconnect_volume(),
                ) {{i18n::text("device-reconnect")}}
            }))
        } else if self.retry {
            Some(AnyView::new(view! {
                Button(
                    variant = ButtonVariant::Outline,
                    on:click = move |_| retry_browser.refresh(),
                ) {{i18n::text("retry")}}
            }))
        } else {
            None
        };

        AnyView::new(view! {
            column(class = "gds-files-collection-state") {
                {status_mark}
                text(class = "gds-files-collection-state__title") {{self.title}}
                {detail_view}
                {action_view}
            }
        })
    }
}

fn location_table_columns(viewport: f32, compact: bool) -> Vec<(&'static str, f32)> {
    let modified_w = if compact { 140.0 } else { 160.0 };
    let type_w = if compact { 96.0 } else { 110.0 };
    let size_w = if compact { 88.0 } else { 100.0 };
    let loc_min = if compact { 180.0 } else { 240.0 };
    let fixed = modified_w + type_w + size_w;
    let remaining = (viewport - fixed).max(180.0 + loc_min);
    let name_w = (remaining * 0.45).max(180.0);
    let loc_w = (viewport - fixed - name_w).max(loc_min);
    vec![
        ("name", name_w),
        ("modified", modified_w),
        ("type", type_w),
        ("size", size_w),
        ("place-location", loc_w),
    ]
}

fn collection_artwork(
    base: IconName,
    badge: Option<IconName>,
    is_directory: bool,
    selected: Signal<bool, LocalStorage>,
) -> AnyView {
    let badge_view = badge.map(|badge_icon| {
        view! {
            box(class = "gds-files-collection-artwork__badge") {
                Icon(name = badge_icon, size = 10.0)
            }
        }
        .into_view()
    });
    AnyView::new(view! {
        box(
            class = "gds-files-collection-artwork",
            attr:data-directory = is_directory.then(|| "true".to_owned()),
            attr:data-selected = move || selected.get().then(|| "true".to_owned()),
        ) {
            Icon(name = base, size = 18.0) {}
            {badge_view}
        }
    })
}

fn collection_name_label(label: String, selected: Signal<bool, LocalStorage>) -> AnyView {
    let a11y_value = SharedString::from(label.clone());
    AnyView::new(view! {
        text(
            class = "gds-files-collection-cell__name",
            a11y:role = Role::Label,
            a11y:value = a11y_value,
            a11y:selected = move || selected.get(),
        ) {{label}}
    })
}

fn name_cell_view(
    artwork: AnyView,
    label: String,
    tooltip: Option<String>,
    _hidden: bool,
    selected: Signal<bool, LocalStorage>,
    col_class: &'static str,
) -> AnyView {
    let tip = tooltip.unwrap_or_else(|| wrap_tokens(&label));
    let name_label = collection_name_label(label, selected);

    let inner = AnyView::new(view! {
        row(
            class = format!("gds-files-collection-cell {col_class}"),
        ) {
            {artwork}
            {name_label}
        }
    });
    AnyView::new(view! {
        HintTooltip(text = tip, trigger_class = col_class) {
            {inner}
        }
    })
}

fn meta_text_cell(
    text: String,
    right_aligned: bool,
    emphasis: bool,
    tooltip: Option<String>,
    col_index: usize,
) -> AnyView {
    let label = view! {
        text(
            class = "gds-files-collection-cell__text",
            attr:data-align = right_aligned.then(|| "end".to_owned()),
            attr:data-emphasis = emphasis.then(|| "true".to_owned()),
        ) {{text}}
    };
    let align_attr = right_aligned.then(|| "end".to_owned());
    let col_class = collection_column_class(col_index);

    let cell = AnyView::new(view! {
        row(
            class = format!("gds-files-collection-cell gds-files-collection-cell--meta {col_class}"),
            attr:data-align = align_attr,
        ) {
            {label}
        }
    });
    match tooltip {
        Some(tip) => AnyView::new(view! {
            HintTooltip(text = tip, trigger_class = col_class) {
                {cell}
            }
        }),
        None => cell,
    }
}

fn timestamp_cell(value: Option<SystemTime>, col_index: usize) -> AnyView {
    let ts_view = view! { {Timestamp::new(value)} };
    let col_class = collection_column_class(col_index);

    AnyView::new(view! {
        row(
            class = format!("gds-files-collection-cell gds-files-collection-cell--meta {col_class}"),
        ) {
            {ts_view}
        }
    })
}

fn remote_entry_artwork(entry: &remote::Entry) -> (IconName, Option<IconName>, bool) {
    match entry.kind {
        remote::EntryKind::Directory => (IconName::Folder, None, true),
        remote::EntryKind::File => {
            let category =
                ContentType::from_filename(OsStr::new(&entry.name)).map(|ct| ct.category);
            (FileIcon::category_icon(category), None, false)
        }
        remote::EntryKind::SymbolicLink => {
            if let Some(ct) = ContentType::from_filename(OsStr::new(&entry.name)) {
                (
                    FileIcon::category_icon(Some(ct.category)),
                    Some(IconName::Link),
                    false,
                )
            } else {
                (IconName::Link, None, false)
            }
        }
        remote::EntryKind::Other => (IconName::FileQuestionMark, None, false),
    }
}

fn remote_entry_type_label(entry: &remote::Entry) -> String {
    match entry.kind {
        remote::EntryKind::Directory => i18n::text("kind-directory"),
        remote::EntryKind::File => ContentType::from_filename(OsStr::new(&entry.name)).map_or_else(
            || i18n::text("kind-file"),
            |ct| EntryPresentation::category_label(ct.category),
        ),
        remote::EntryKind::SymbolicLink => i18n::text("kind-link"),
        remote::EntryKind::Other => i18n::text("kind-other"),
    }
}

fn fallback_place_type_label(location: &Location) -> String {
    match location {
        Location::Directory(_) | Location::Volume { .. } | Location::Remote(_) => {
            i18n::text("kind-directory")
        }
        Location::Tags(_) => i18n::text("tags-title"),
        Location::Network => i18n::text("network"),
        Location::Trash => i18n::text("trash"),
        Location::Volumes => i18n::text("volumes"),
        Location::Recent | Location::Favorites => i18n::text("locations"),
    }
}

fn trash_entry_artwork(
    entry: &crate::features::file_operations::recycle::Entry,
) -> (IconName, Option<IconName>, bool) {
    match entry.kind {
        EntryKind::Directory => (IconName::Folder, None, true),
        EntryKind::SymbolicLink => (IconName::Link, None, false),
        EntryKind::File => {
            let category = ContentType::from_filename(&entry.name).map(|ct| ct.category);
            (FileIcon::category_icon(category), None, false)
        }
        EntryKind::Other | EntryKind::Unknown => (IconName::File, None, false),
    }
}

fn volume_device_icon(volume: &volumes::Volume, snapshot: &volumes::Snapshot) -> IconName {
    if volume.locked {
        return IconName::Lock;
    }
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
}

fn active_mount_point(browser: &Browser, volume: &volumes::Volume) -> Option<PathBuf> {
    let preferred = volume.id.stable.as_ref().and_then(|id| {
        browser
            .0
            .preferences
            .snapshot()
            .files
            .volume_mount_points
            .get(id)
            .cloned()
    });
    preferred
        .filter(|candidate| volume.mount_points.contains(candidate))
        .or_else(|| {
            volume
                .mount_points
                .iter()
                .min_by(|left, right| left.components().count().cmp(&right.components().count()))
                .cloned()
        })
}

fn volume_capacity_cell(
    browser: &Browser,
    volume: &volumes::Volume,
    selected: Signal<bool, LocalStorage>,
    col_index: usize,
) -> AnyView {
    let state = browser.0.volumes.capacity(&volume.id);
    let (label, fraction, unavailable) = match state {
        None | Some(volumes::CapacityState::Loading) => {
            if volume.mount_points.is_empty() {
                (i18n::text("device-not-mounted"), None, true)
            } else {
                (i18n::text("device-capacity-loading"), None, false)
            }
        }
        Some(volumes::CapacityState::Ready(capacity)) => {
            let available = EntryPresentation::format_size(capacity.available);
            let total = EntryPresentation::format_size(capacity.total);
            (
                t!("device-capacity", available, total),
                Some(capacity.unavailable_fraction()),
                false,
            )
        }
        Some(volumes::CapacityState::Unavailable(reason)) => {
            let message = if volume.mount_points.is_empty() {
                i18n::text("device-not-mounted")
            } else {
                reason
            };
            (message, None, true)
        }
    };

    let col_class = collection_column_class(col_index);

    AnyView::new(view! {
        row(
            class = format!("gds-files-collection-cell {col_class}"),
        ) {
            CapacityBar(
                label = label,
                fraction = fraction,
                unavailable = unavailable,
                selected = selected,
            )
        }
    })
}

fn volume_mount_point_cell(
    browser: &Browser,
    volume: &volumes::Volume,
    col_index: usize,
) -> AnyView {
    let busy = browser.device_is_busy(&volume.id);
    let action_button = if volume.mountable && volume.mount_points.is_empty() {
        let b = browser.clone();
        let id = volume.id.clone();
        Some(AnyView::new(view! {
            IconButton(
                icon = IconName::Plug,
                label = i18n::text("device-mount"),
                disabled = busy,
                on:pointer_down = move |ev| ev.stop_propagation(),
                on:click = move |ev| {
                    ev.stop_propagation();
                    b.request_device_action(volumes::DeviceAction::Mount(id.clone()));
                },
            )
        }))
    } else if volume.can_unmount() {
        let b = browser.clone();
        let id = volume.id.clone();
        Some(AnyView::new(view! {
            IconButton(
                icon = IconName::Unplug,
                label = i18n::text("device-unmount"),
                disabled = busy,
                on:pointer_down = move |ev| ev.stop_propagation(),
                on:click = move |ev| {
                    ev.stop_propagation();
                    b.request_device_action(volumes::DeviceAction::Unmount(id.clone()));
                },
            )
        }))
    } else {
        None
    };

    let lead_view = if volume.mount_points.is_empty() {
        let text = if !volume.mountable && volume.filesystem.is_empty() {
            i18n::text("device-not-browsable")
        } else {
            i18n::text("device-not-mounted")
        };
        AnyView::new(view! {
            text(class = "gds-files-collection-cell__text") {{text}}
        })
    } else if volume.mount_points.len() == 1 {
        let path_text = volume.mount_points[0].display().to_string();
        let tooltip = path_text.clone();
        AnyView::new(view! {
            HintTooltip(text = tooltip) {
                text(
                    class = "gds-files-collection-cell__text",
                    attr:data-emphasis = "true",
                ) {{path_text}}
            }
        })
    } else {
        let current = active_mount_point(browser, volume);
        let current_label = current
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| UNKNOWN_VALUE.to_owned());
        let stable_id = volume.id.stable.clone();
        let can_select = stable_id.is_some();

        let mount_points = volume.mount_points.clone();
        let menu_browser = browser.clone();

        let trigger = AnyView::new(view! {
            DropdownMenu {
                DropdownMenuTrigger(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    class = "gds-files-collection-mount-picker",
                    on:pointer_down = move |ev| ev.stop_propagation(),
                    on:click = move |ev| ev.stop_propagation(),
                ) {
                    row(style:gap = "4px", style:min-width = "0") {
                        text(class = "gds-files-collection-mount-picker__label") {{current_label}}
                        Icon(name = IconName::ChevronDown) {}
                    }
                }
                DropdownMenuContent(class = "gds-menu-content") {
                    MenuFocusPark {}
                    {mount_points
                        .iter()
                        .map(|mount| {
                            let is_current = current.as_ref() == Some(mount);
                            let label = mount.display().to_string();
                            let target_mount = mount.clone();
                            let b = menu_browser.clone();
                            let stable = stable_id.clone();
                            AnyView::new(view! {
                                MenuCheckboxItem(
                                    default_checked = is_current,
                                    disabled = !can_select,
                                    on_change = UnsyncCallback::new(move |_| {
                                        if let Some(id) = stable.clone() {
                                            let chosen = target_mount.clone();
                                            b.edit_preferences(|prefs| {
                                                prefs.files.volume_mount_points.insert(id, chosen);
                                            });
                                            b.changed();
                                        }
                                    }),
                                ) {
                                    {label}
                                }
                            })
                        })
                        .collect::<Vec<_>>()}
                }
            }
        });

        if can_select {
            trigger
        } else {
            AnyView::new(view! {
                HintTooltip(text = i18n::text("volume-default-unavailable")) {
                    {trigger}
                }
            })
        }
    };

    let col_class = collection_column_class(col_index);

    AnyView::new(view! {
        row(
            class = format!("gds-files-collection-cell gds-files-collection-cell--split {col_class}"),
        ) {
            {lead_view}
            {action_button}
        }
    })
}

impl Browser {
    pub(super) fn collection_notice(&self) -> Option<AnyView> {
        install_surfaces_stylesheet();
        let (location, contents_kind, has_rows) = self.read(|state| {
            let tab = state.tab();
            let kind = match &tab.contents {
                Contents::Tags { .. } => 1_u8,
                Contents::Volumes => 2_u8,
                _ => 0_u8,
            };
            (
                tab.navigation.location().clone(),
                kind,
                !tab.rows.is_empty(),
            )
        });

        if matches!(location, Location::Tags(_))
            && !self.0.preferences.state().with(|p| p.tags.enabled)
            && contents_kind == 1
        {
            let b = self.clone();
            return Some(AnyView::new(view! {
                PageNotice(class = "gds-files-collection-notice") {
                    row(class = "gds-files-collection-notice__lead") {
                        Icon(name = IconName::Tag)
                        text(class = "gds-files-collection-notice__text") {{i18n::text("tags-disabled")}}
                    }
                    Button(
                        variant = ButtonVariant::Outline,
                        on:click = move |_| b.show_settings(),
                    ) {{i18n::text("settings")}}
                }
            }));
        }

        if contents_kind == 2 && has_rows {
            if let Some(error) = self.0.volumes.error() {
                let b = self.clone();
                return Some(AnyView::new(view! {
                    PageNotice(class = "gds-files-collection-notice gds-files-collection-notice--error") {
                        row(class = "gds-files-collection-notice__lead") {
                            Icon(name = IconName::CircleAlert)
                            text(class = "gds-files-collection-notice__text") {{error}}
                        }
                        Button(
                            variant = ButtonVariant::Outline,
                            on:click = move |_| b.refresh(),
                        ) {{i18n::text("retry")}}
                    }
                }));
            }
            if self.0.volumes.discovering() {
                let discovering_label = i18n::text("device-discovering");
                return Some(AnyView::new(view! {
                    PageNotice(class = "gds-files-collection-notice") {
                        row(class = "gds-files-collection-notice__lead") {
                            Spinner(label = discovering_label.clone())
                            text(class = "gds-files-collection-notice__text") {{discovering_label}}
                        }
                    }
                }));
            }
        }

        None
    }

    pub(super) fn collection_body_state(&self) -> Option<CollectionBodyState> {
        let (location, is_loading, unavailable, is_volumes, empty_rows) = self.read(|state| {
            let tab = state.tab();
            let location = tab.navigation.location().clone();
            let (is_loading, unavailable, is_volumes) = match &tab.contents {
                Contents::LoadingCollection => (true, None, false),
                Contents::Unavailable(msg) => (false, Some(msg.clone()), false),
                Contents::Volumes => (false, None, true),
                _ => (false, None, false),
            };
            (
                location,
                is_loading,
                unavailable,
                is_volumes,
                tab.rows.is_empty(),
            )
        });

        if is_loading {
            let title = match &location {
                Location::Trash => i18n::text("recycle-loading"),
                Location::Network => i18n::text("network-loading"),
                Location::Volumes => i18n::text("device-discovering"),
                Location::Tags(_) => i18n::text("tags-loading"),
                _ => i18n::text("loading"),
            };
            return Some(CollectionBodyState {
                icon: None,
                title,
                detail: None,
                retry: false,
                loading: true,
                reconnect: false,
            });
        }

        if let Some(error) = unavailable {
            return Some(match &location {
                Location::Trash => CollectionBodyState {
                    icon: Some(IconName::Trash),
                    title: i18n::text("recycle-load-failed"),
                    detail: Some(error),
                    retry: true,
                    loading: false,
                    reconnect: false,
                },
                Location::Tags(_) => {
                    let enabled = self.0.preferences.state().with(|p| p.tags.enabled);
                    CollectionBodyState {
                        icon: Some(IconName::Tag),
                        title: if enabled {
                            i18n::text("tags-unavailable")
                        } else {
                            i18n::text("tags-disabled")
                        },
                        detail: enabled.then_some(error),
                        retry: enabled,
                        loading: false,
                        reconnect: false,
                    }
                }
                Location::Volumes => CollectionBodyState {
                    icon: Some(IconName::HardDrive),
                    title: i18n::text("device-operation-failed"),
                    detail: Some(error),
                    retry: true,
                    loading: false,
                    reconnect: false,
                },
                Location::Volume { identity, .. } => {
                    let attached = self.volume_at_location(identity).is_some();
                    CollectionBodyState {
                        icon: Some(IconName::HardDrive),
                        title: if attached {
                            error.clone()
                        } else {
                            i18n::text("device-disconnected")
                        },
                        detail: (!attached).then_some(error),
                        retry: attached,
                        loading: false,
                        reconnect: !attached,
                    }
                }
                _ => CollectionBodyState {
                    icon: Some(IconName::CircleAlert),
                    title: error,
                    detail: None,
                    retry: true,
                    loading: false,
                    reconnect: false,
                },
            });
        }

        if is_volumes && empty_rows {
            if self.0.volumes.discovering() {
                return Some(CollectionBodyState {
                    icon: None,
                    title: i18n::text("device-discovering"),
                    detail: None,
                    retry: false,
                    loading: true,
                    reconnect: false,
                });
            }
            if let Some(error) = self.0.volumes.error() {
                return Some(CollectionBodyState {
                    icon: Some(IconName::HardDrive),
                    title: i18n::text("device-operation-failed"),
                    detail: Some(error),
                    retry: true,
                    loading: false,
                    reconnect: false,
                });
            }
        }

        None
    }

    pub(super) fn collection_empty_label(&self) -> Option<&'static str> {
        let filtered = !self.0.filter.with(String::is_empty);
        self.read(|state| {
            let tab = state.tab();
            if !tab.rows.is_empty() {
                return None;
            }
            if filtered {
                return Some("empty-filter");
            }
            match &tab.contents {
                Contents::Places(_) | Contents::Tags { .. } => Some("empty-collection"),
                Contents::Trash(_) => Some("recycle-empty"),
                Contents::Remote { .. } => Some("empty-directory"),
                Contents::Volumes => Some("device-none"),
                Contents::Network { .. } => Some("network-empty"),
                Contents::Directory | Contents::LoadingCollection | Contents::Unavailable(_) => {
                    None
                }
            }
        })
    }

    pub(super) fn collection_column_ids(&self) -> Option<Vec<&'static str>> {
        self.read_structure(|state| {
            let tab = state.tab();
            match &tab.contents {
                Contents::Places(_) => {
                    Some(vec!["name", "modified", "type", "size", "place-location"])
                }
                Contents::Trash(_) => Some(vec![
                    "name",
                    "recycle-original-location",
                    "recycle-deleted-at",
                    "size",
                ]),
                Contents::Tags { .. } => {
                    let has_files = tab.rows.iter().any(|row| matches!(row, Row::File(_)));
                    if has_files {
                        Some(vec!["name", "modified", "type", "size", "place-location"])
                    } else {
                        Some(vec!["name"])
                    }
                }
                Contents::Remote { .. } => Some(vec!["name", "modified", "type", "size"]),
                Contents::Volumes => Some(vec![
                    "name",
                    "volume-format",
                    "volume-space",
                    "volume-default-mount",
                ]),
                Contents::Directory
                | Contents::LoadingCollection
                | Contents::Unavailable(_)
                | Contents::Network { .. } => None,
            }
        })
    }

    pub(super) fn collection_columns(&self) -> Option<Vec<(&'static str, f32)>> {
        let dimensions = || {
            (
                self.table_viewport_width().max(240.0),
                self.narrow_collection(),
            )
        };
        let _ = self.0.revision.get();
        let state = self.0.state.borrow();
        let tab = state.tab();
        match &tab.contents {
            Contents::Places(_) => {
                let (viewport, compact) = dimensions();
                Some(location_table_columns(viewport, compact))
            }
            Contents::Trash(_) => {
                let (viewport, compact) = dimensions();
                let orig_min = if compact { 170.0 } else { 240.0 };
                let deleted_w = if compact { 140.0 } else { 160.0 };
                let size_w = if compact { 96.0 } else { 120.0 };
                let fixed = deleted_w + size_w;
                let remaining = (viewport - fixed).max(200.0 + orig_min);
                let name_w = (remaining * 0.46).max(200.0);
                let orig_w = (viewport - fixed - name_w).max(orig_min);
                Some(vec![
                    ("name", name_w),
                    ("recycle-original-location", orig_w),
                    ("recycle-deleted-at", deleted_w),
                    ("size", size_w),
                ])
            }
            Contents::Tags { .. } => {
                let (viewport, compact) = dimensions();
                // Pure tag catalogues never show file metadata columns (testing.md:73).
                let has_files = tab.rows.iter().any(|row| matches!(row, Row::File(_)));
                if has_files {
                    Some(location_table_columns(viewport, compact))
                } else {
                    Some(vec![("name", viewport)])
                }
            }
            Contents::Remote { .. } => {
                let (viewport, compact) = dimensions();
                let modified_w = if compact { 140.0 } else { 170.0 };
                let type_w = if compact { 100.0 } else { 120.0 };
                let size_w = if compact { 90.0 } else { 110.0 };
                let name_w = (viewport - modified_w - type_w - size_w).max(200.0);
                Some(vec![
                    ("name", name_w),
                    ("modified", modified_w),
                    ("type", type_w),
                    ("size", size_w),
                ])
            }
            Contents::Volumes => {
                let (viewport, compact) = dimensions();
                let format_w = if compact { 90.0 } else { 110.0 };
                let space_w = if compact { 170.0 } else { 220.0 };
                let mount_w = if compact { 160.0 } else { 210.0 };
                let name_w = (viewport - format_w - space_w - mount_w).max(180.0);
                Some(vec![
                    ("name", name_w),
                    ("volume-format", format_w),
                    ("volume-space", space_w),
                    ("volume-default-mount", mount_w),
                ])
            }
            Contents::Directory
            | Contents::LoadingCollection
            | Contents::Unavailable(_)
            | Contents::Network { .. } => None,
        }
    }

    pub(super) fn collection_header(&self) -> Option<AnyView> {
        install_surfaces_stylesheet();
        let columns = self.collection_column_ids()?;
        let sortable = self.sortable_columns();
        let sort = self.read(|state| state.tab().sort);

        let cells: Vec<AnyView> = columns
            .into_iter()
            .enumerate()
            .map(|(idx, label_id)| {
                let is_name_col = idx == 0;
                let is_right_aligned = label_id == "size";
                let label = i18n::text(label_id);
                let column = Column::from_id(label_id).filter(|col| sortable.contains(col));
                let col_class = collection_column_class(idx);


                if let Some(col) = column {
                    let active = sort.key == col;
                    let ascending = sort.direction == Direction::Ascending;
                    let b = self.clone();
                    let a11y_label = SharedString::from(label.clone());
                    let label_node = AnyView::new(view! {
                        text(class = "gds-files-collection-header__label") {{label}}
                    });
                    let sort_node = AnyView::new(view! {
                        column(
                            class = "gds-files-collection-header__sort",
                            attr:data-active = active.then(|| "true".to_owned()),
                        ) {
                            {{if active {
                                let icon = if ascending {
                                    IconName::ChevronUp
                                } else {
                                    IconName::ChevronDown
                                };
                                AnyView::new(view! { Icon(name = icon, size = 12.0) })
                            } else {
                                AnyView::new(view! {
                                    column(style:align-items = "center", style:justify-content = "center") {
                                        Icon(name = IconName::ChevronUp, size = 8.0) {}
                                        Icon(name = IconName::ChevronDown, size = 8.0) {}
                                    }
                                })
                            }}}
                        }
                    });
                    AnyView::new(view! {
                        row(
                            class = format!("gds-files-collection-header__cell {col_class}"),
                        ) {
                            control(
                                class = "gds-files-collection-header__button",
                                attr:data-name-col = is_name_col.then(|| "true".to_owned()),
                                attr:data-align = is_right_aligned.then(|| "end".to_owned()),
                                attr:data-active = active.then(|| "true".to_owned()),
                                tabindex = Focus::Sequential,
                                a11y:role = Role::Button,
                                a11y:label = a11y_label,
                                on:click = move |_| b.column_sort(col),
                            ) {
                                {if is_right_aligned {
                                    AnyView::new(view! {
                                        {sort_node}
                                        {label_node}
                                    })
                                } else {
                                    AnyView::new(view! {
                                        {label_node}
                                        {sort_node}
                                    })
                                }}
                            }
                        }
                    })
                } else {
                    AnyView::new(view! {
                        row(
                            class = format!("gds-files-collection-header__cell {col_class}"),
                        ) {
                            row(
                                class = "gds-files-collection-header__static",
                                attr:data-name-col = is_name_col.then(|| "true".to_owned()),
                                attr:data-align = is_right_aligned.then(|| "end".to_owned()),
                            ) {
                                text(class = "gds-files-collection-header__label") {{label}}
                            }
                        }
                    })
                }
            })
            .collect();

        Some(AnyView::new(view! {
            row(class = "gds-files-collection-header") {
                {cells}
            }
        }))
    }

    pub(super) fn collection_cells(
        &self,
        row: &Row,
        selected: Binding<bool>,
    ) -> Option<Vec<AnyView>> {
        install_surfaces_stylesheet();
        let col_count = self.collection_column_ids()?.len();
        let sel_bool = Signal::derive_local(move || selected.get().unwrap_or(false));

        match row {
            Row::File(entry) => {
                let icon = FileIcon::for_entry(entry);
                let artwork = collection_artwork(
                    icon.base(),
                    icon.badge(),
                    FileIcon::is_directory(entry),
                    sel_bool,
                );
                let address = entry.path.display().to_string();
                let name_cell = name_cell_view(
                    artwork,
                    entry.display_name.clone(),
                    Some(address.clone()),
                    entry.is_hidden,
                    sel_bool,
                    collection_column_class(0),
                );
                Some(vec![
                    name_cell,
                    timestamp_cell(entry.modified, 1),
                    meta_text_cell(EntryPresentation::type_label(entry), false, false, None, 2),
                    meta_text_cell(EntryPresentation::size_label(entry), true, false, None, 3),
                    meta_text_cell(address.clone(), false, false, Some(address), 4),
                ])
            }
            Row::Place(location) => {
                let entry = self.place_entry(location);
                let address = location.display_address();
                let (name, artwork, modified, kind_label, size_label, hidden) = match &entry {
                    Some(entry) => {
                        let icon = FileIcon::for_entry(entry);
                        let label = if matches!(
                            location,
                            Location::Volume { relative, .. } if relative.is_empty()
                        ) {
                            self.location_title(location)
                        } else if entry.display_name.is_empty() {
                            location.label()
                        } else {
                            entry.display_name.clone()
                        };
                        let art = collection_artwork(
                            icon.base(),
                            icon.badge(),
                            FileIcon::is_directory(entry),
                            sel_bool,
                        );
                        (
                            label,
                            art,
                            entry.modified,
                            EntryPresentation::type_label(entry),
                            EntryPresentation::size_label(entry),
                            entry.is_hidden,
                        )
                    }
                    None => {
                        let is_dir =
                            matches!(location, Location::Directory(_) | Location::Volume { .. });
                        let art = collection_artwork(location.icon(), None, is_dir, sel_bool);
                        let label = self.location_title(location);
                        (
                            label,
                            art,
                            None,
                            fallback_place_type_label(location),
                            UNKNOWN_VALUE.to_owned(),
                            false,
                        )
                    }
                };

                let name_cell = name_cell_view(
                    artwork,
                    name,
                    Some(address.clone()),
                    hidden,
                    sel_bool,
                    collection_column_class(0),
                );
                Some(vec![
                    name_cell,
                    timestamp_cell(modified, 1),
                    meta_text_cell(kind_label, false, false, None, 2),
                    meta_text_cell(size_label, true, false, None, 3),
                    meta_text_cell(address.clone(), false, false, Some(address), 4),
                ])
            }
            Row::Trash(entry) => {
                let (base, badge, is_dir) = trash_entry_artwork(entry);
                let artwork = collection_artwork(base, badge, is_dir, sel_bool);
                let name = entry.name.to_string_lossy().into_owned();
                let name_cell = name_cell_view(
                    artwork,
                    name.clone(),
                    Some(wrap_tokens(&name)),
                    false,
                    sel_bool,
                    collection_column_class(0),
                );

                let full_original = entry.original_path.display().to_string();
                let size_label = if let Some(items) = entry.items {
                    i18n::count("item-count", items)
                } else if let Some(bytes) = entry.size {
                    EntryPresentation::format_size(bytes)
                } else {
                    UNKNOWN_VALUE.to_owned()
                };

                Some(vec![
                    name_cell,
                    meta_text_cell(full_original.clone(), false, false, Some(full_original), 1),
                    timestamp_cell(entry.deleted_at, 2),
                    meta_text_cell(size_label, true, false, None, 3),
                ])
            }
            Row::Tag(tag) => {
                let artwork = collection_artwork(IconName::Tag, None, false, sel_bool);
                let leaf = row.label();
                let tip = if tag.contains('/') {
                    tag.clone()
                } else {
                    wrap_tokens(&leaf)
                };
                let name_cell = name_cell_view(
                    artwork,
                    leaf,
                    Some(tip),
                    false,
                    sel_bool,
                    collection_column_class(0),
                );

                if col_count <= 1 {
                    Some(vec![name_cell])
                } else {
                    let tag_address = TagAddress::new(tag).map_or_else(
                        |_| format!("tag://{tag}"),
                        |addr| Location::Tags(addr).display_address(),
                    );
                    Some(vec![
                        name_cell,
                        timestamp_cell(None, 1),
                        meta_text_cell(i18n::text("tags-title"), false, false, None, 2),
                        meta_text_cell(UNKNOWN_VALUE.to_owned(), true, false, None, 3),
                        meta_text_cell(tag_address.clone(), false, false, Some(tag_address), 4),
                    ])
                }
            }
            Row::Remote(entry) => {
                let (base, badge, is_dir) = remote_entry_artwork(entry);
                let artwork = collection_artwork(base, badge, is_dir, sel_bool);
                let name_cell = name_cell_view(
                    artwork,
                    entry.name.clone(),
                    Some(wrap_tokens(&entry.name)),
                    entry.name.starts_with('.'),
                    sel_bool,
                    collection_column_class(0),
                );

                let size_text = if entry.kind == remote::EntryKind::Directory {
                    UNKNOWN_VALUE.to_owned()
                } else {
                    entry
                        .size
                        .map_or_else(|| UNKNOWN_VALUE.to_owned(), EntryPresentation::format_size)
                };

                Some(vec![
                    name_cell,
                    timestamp_cell(entry.modified, 1),
                    meta_text_cell(remote_entry_type_label(entry), false, false, None, 2),
                    meta_text_cell(size_text, true, false, None, 3),
                ])
            }
            Row::Volume(volume) => {
                let snapshot = self.0.volumes.snapshot_untracked();
                let base_icon = volume_device_icon(volume, &snapshot);
                let artwork = collection_artwork(base_icon, None, false, sel_bool);
                let name_label = collection_name_label(volume.name.clone(), sel_bool);
                let busy = self.device_is_busy(&volume.id);
                let busy_spinner = busy.then(|| {
                    view! {
                        Spinner(label = volume.name.clone())
                    }
                    .into_view()
                });
                let system_badge = volume.system_mount.then(|| {
                    view! {
                        text(class = "gds-files-collection-cell__badge") {{i18n::text("device-system-volume")}}
                    }
                    .into_view()
                });

                let name_cell = AnyView::new(view! {
                    row(
                        class = format!("gds-files-collection-cell {}", collection_column_class(0)),
                    ) {
                        {artwork}
                        {name_label}
                        {busy_spinner}
                        {system_badge}
                    }
                });

                let (format_text, format_emphasis) = if volume.filesystem.is_empty() {
                    (UNKNOWN_VALUE.to_owned(), false)
                } else {
                    (volume.filesystem.clone(), true)
                };

                Some(vec![
                    name_cell,
                    meta_text_cell(format_text, false, format_emphasis, None, 1),
                    volume_capacity_cell(self, volume, sel_bool, 2),
                    volume_mount_point_cell(self, volume, 3),
                ])
            }
        }
    }
}
