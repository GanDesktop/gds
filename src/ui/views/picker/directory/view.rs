use std::cell::Cell;
use std::rc::Rc;

use zgui::geom::{Css, CssPx, Point, Rect, Size};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, Signal, UnsyncCallback};
use zgui::view::{ListenerOptions, events, focus_is_text_entry};
use zgui::vocab::{Key, NamedKey, PointerButton, Role};
use zgui::{component, css, view};
use zgui_ui::{
    BreadcrumbItemProps, BreadcrumbLinkProps, BreadcrumbListProps, BreadcrumbPageProps,
    BreadcrumbProps, BreadcrumbSeparatorProps, ButtonSize, ButtonVariant, EmptyContentProps,
    EmptyDescriptionProps, EmptyHeaderProps, EmptyProps, EmptyTitleProps, InputGroupAddonAlign,
    InputGroupAddonProps, InputGroupButtonProps, InputGroupButtonSize, InputGroupInputProps,
    InputGroupProps, InputProps, SpinnerProps, VirtualListProps,
};
use zgui_ui_primitives::Binding;

use super::commands::PickerCommand;
use super::table::DirectoryRows;
use super::{
    DirectoryPicker, Location, MAX_PICKER_HEIGHT, MIN_PICKER_HEIGHT, ROW_HEIGHT,
    TABLE_LOAD_MORE_THRESHOLD,
};
use crate::app::Gds;
use crate::appearance::tokens::{DataListStyle, SIDEBAR_ICON_SIZE};
use crate::ui::button::{ButtonProps, IconButtonProps};
use crate::ui::dialog::{
    DialogActionsProps, DialogAnchor, DialogDecision, DialogFooterProps, DialogHeaderProps,
    DialogSize, DialogTitleProps, ModalDialogProps,
};
use crate::ui::icon::{IconName, IconProps};
use crate::ui::list::{ListGeometry, ListSurfaceProps};
use crate::ui::picker::{
    PickerContentProps, PickerLocationProps, PickerProps, PickerStatusProps, PickerToolbarProps,
};
use crate::ui::tooltip::{HintTooltipProps, wrap_tokens};

const SHEET: &str = css!(
    r#"
    .gds-directory-picker {
        width: 100%;
    }

    .gds-picker-address-input.gds-picker-address-input {
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        height: 32px;
        border-radius: 6px;
        background-color: var(--gds-content);
        border: 1px solid var(--gds-outline);
        box-shadow: none;
    }

    .gds-picker-address-input.gds-picker-address-input:focus,
    .gds-picker-address-input.gds-picker-address-input:focus-visible {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }

    .gds-picker-location-scroll {
        width: 100%;
        min-width: 0px;
        overflow-x: auto;
        overflow-y: hidden;
        --zgui-scrollbar-thumb: transparent;
        --zgui-scrollbar-track: transparent;
    }

    .gds-picker-breadcrumbs {
        width: 100%;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-picker-current-crumb {
        max-width: 180px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-picker-search-wrap {
        flex: 1 1 0%;
        min-width: 0px;
    }

    .gds-picker-search-group.gds-picker-search-group {
        width: 100%;
        height: 32px;
        border-radius: 6px;
        background-color: var(--gds-content);
        border: 1px solid var(--gds-outline);
        box-shadow: none;
    }

    .gds-picker-search-group.gds-picker-search-group:focus-within {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }

    .gds-picker-table-host {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-picker-list-surface.gds-picker-list-surface {
        border: none;
        background: transparent;
    }

    .gds-picker-header {
        display: flex;
        flex-direction: row;
        align-items: center;
        height: var(--gds-list-header-height, 30px);
        flex-shrink: 0;
        padding: 0px 12px;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        border-bottom: 1px solid var(--gds-border);
        user-select: none;
    }

    .gds-picker-list {
        flex: 1 1 0%;
        width: 100%;
        min-height: 0px;
        overflow-x: hidden;
    }

    .gds-picker-list .zui-virtual-list__row {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
        padding: var(--gds-list-selection-inset, 2px) 0px;
        box-sizing: border-box;
    }

    .gds-picker-list .zui-virtual-list__row > .zui-tooltip__trigger {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
    }

    .gds-picker-row {
        display: flex;
        flex-direction: row;
        flex: 1 1 0%;
        align-items: center;
        gap: 10px;
        width: 100%;
        height: 100%;
        min-height: var(--gds-list-selection-height, 36px);
        min-width: 0px;
        padding: 0px 12px;
        box-sizing: border-box;
        cursor: pointer;
        border-radius: 6px;
        color: var(--gds-foreground);
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-picker-row__icon {
        flex-shrink: 0;
        color: var(--gds-primary);
    }

    .gds-picker-row[data-selected="true"] .gds-picker-row__icon {
        color: var(--gds-selection-foreground);
    }

    .gds-picker-row:hover {
        background-color: var(--gds-hover);
    }

    .gds-picker-row[data-hidden="true"]:not([data-selected="true"]) .gds-picker-row__icon,
    .gds-picker-row[data-hidden="true"]:not([data-selected="true"]) .gds-picker-row__label {
        opacity: 0.65;
    }

    .gds-picker-row[data-selected="true"],
    .gds-picker-row[data-selected="true"]:hover {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
    }

    .gds-picker-row:not([data-selected="true"]):focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-picker-row[data-selected="true"]:focus-visible {
        outline: none;
    }

    .gds-picker-row__label {
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-picker-center {
        flex: 1 1 0%;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 20px;
        min-height: 200px;
    }

    .gds-picker-chosen-path {
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }
    "#
);

fn install_directory_picker_stylesheets() {
    if zgui::view::current_host().is_some() {
        crate::ui::styles::install_ui_stylesheets();
        zgui::view::install_stylesheet("gds-ui-directory-picker", SHEET);
    }
}

fn text_or(key: &'static str, fallback: &'static str) -> String {
    Gds::try_current().map_or_else(|| fallback.to_owned(), |gds| gds.i18n().text(key))
}

fn text_or_untracked(key: &'static str, fallback: &'static str) -> String {
    Gds::try_current().map_or_else(|| fallback.to_owned(), |gds| gds.i18n().text_untracked(key))
}

fn picker_body_height() -> f32 {
    if let Some(window) = zgui::prelude::try_use_window() {
        let win_h = window.size().get().height.0;
        if win_h > 0.0 {
            return (win_h - 190.0).clamp(MIN_PICKER_HEIGHT, MAX_PICKER_HEIGHT);
        }
    }
    360.0
}

fn handle_picker_keys(
    picker: &DirectoryPicker,
    address_ref: &NodeRef,
    filter_ref: &NodeRef,
    ev: &mut EventCx<'_, events::KeyDown>,
) {
    if PickerCommand::Back.matches_event(ev, ev.modifiers) {
        picker.back();
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    if PickerCommand::Forward.matches_event(ev, ev.modifiers) {
        picker.forward();
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    if PickerCommand::Up.matches_event(ev, ev.modifiers) {
        picker.parent();
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    if PickerCommand::Refresh.matches_event(ev, ev.modifiers) {
        picker.refresh();
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    if PickerCommand::EditLocation.matches_event(ev, ev.modifiers) {
        picker.begin_edit_location();
        address_ref.focus();
        address_ref.select_all();
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    if PickerCommand::Filter.matches_event(ev, ev.modifiers) {
        filter_ref.focus();
        filter_ref.select_all();
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    if PickerCommand::Hidden.matches_event(ev, ev.modifiers) {
        picker.toggle_hidden();
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }

    let mods = ev.modifiers.held();
    if focus_is_text_entry() || !mods.is_empty() {
        return;
    }

    match &ev.key {
        Key::Named(NamedKey::ArrowDown) => {
            picker.move_selection(1);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::ArrowUp) => {
            picker.move_selection(-1);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::Home) => {
            picker.select_first();
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::End) => {
            picker.select_last();
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::Enter) => {
            picker.open_selected();
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::Backspace) if !ev.repeat => {
            picker.parent();
            ev.prevent_default();
            ev.stop_propagation();
        }
        _ => {}
    }
}

fn device_to_css_bounds(node: NodeRef) -> Option<Rect<CssPx, Css>> {
    let dev = node.window_bounds()?;
    let scale = node.scale().max(0.01);
    Some(Rect::new(
        Point::new(CssPx(dev.origin.x.0 / scale), CssPx(dev.origin.y.0 / scale)),
        Size::new(
            CssPx(dev.size.width.0 / scale),
            CssPx(dev.size.height.0 / scale),
        ),
    ))
}

/// Directory chooser view component driven by [`DirectoryPicker`].
#[component]
pub fn DirectoryPickerView(
    picker: DirectoryPicker,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    install_directory_picker_stylesheets();

    picker.start();

    let address_ref = NodeRef::new();
    let filter_ref = NodeRef::new();
    let list_ref = NodeRef::new();
    let surface_ref = NodeRef::new();

    let page = picker.page();
    let rows = picker.rows();
    let selected_index = picker.selected_index();
    let is_loading = picker.is_loading();
    let pending_request = picker.pending_request();
    let loading_more = picker.is_loading_more();
    let error = picker.error();
    let show_hidden = picker.show_hidden();
    let filter_query = picker.filter_query();
    let editing = picker.is_editing_location();
    let address_text = picker.address_text();
    let can_back = picker.can_go_back();
    let can_forward = picker.can_go_forward();
    let can_parent = picker.can_go_parent();
    let chosen_path = picker.chosen_path();

    let back_label = Signal::derive_local(|| text_or(PickerCommand::Back.label_id(), "Back"));
    let forward_label =
        Signal::derive_local(|| text_or(PickerCommand::Forward.label_id(), "Forward"));
    let parent_label =
        Signal::derive_local(|| text_or(PickerCommand::Up.label_id(), "Parent folder"));
    let location_label =
        Signal::derive_local(|| text_or(PickerCommand::EditLocation.label_id(), "Edit location"));
    let stop_label = Signal::derive_local(|| text_or("picker-stop", "Stop loading"));
    let refresh_label =
        Signal::derive_local(|| text_or(PickerCommand::Refresh.label_id(), "Refresh"));
    let filter_placeholder =
        Signal::derive_local(|| text_or(PickerCommand::Filter.label_id(), "Filter folders…"));
    let clear_filter_label = Signal::derive_local(|| text_or("close", "Clear"));
    let hidden_label =
        Signal::derive_local(|| text_or(PickerCommand::Hidden.label_id(), "Show hidden folders"));
    let loading_label = Signal::derive_local(|| text_or("picker-loading", "Loading folders…"));
    let failed_title = Signal::derive_local(|| text_or("picker-failed", "Cannot open this folder"));
    let retry_label = Signal::derive_local(|| text_or("picker-retry", "Retry"));
    let empty_title = Signal::derive_local(|| text_or("picker-empty", "No visible folders"));
    let no_matches_title =
        Signal::derive_local(|| text_or("picker-no-matches", "No matching folders"));
    let name_header_label = Signal::derive_local(|| text_or("picker-name", "Name"));

    let picker_keys = picker.clone();
    let addr_key_ref = address_ref;
    let filter_key_ref = filter_ref;

    let picker_back = picker.clone();
    let picker_fwd = picker.clone();
    let picker_up = picker.clone();
    let picker_edit = picker.clone();
    let addr_focus_ref = address_ref;
    let picker_stop_or_refresh = picker.clone();
    let picker_hidden = picker.clone();
    let picker_filter = picker.clone();
    let picker_filter_clear = picker.clone();
    let picker_addr_input = picker.clone();
    let picker_addr_submit = picker.clone();
    let picker_addr_cancel = picker.clone();
    let picker_crumb = picker.clone();
    let picker_cancel_load = picker.clone();
    let picker_retry = picker.clone();
    let picker_row = picker.clone();
    let picker_scroll = picker.clone();
    let picker_blank = picker.clone();

    let row_count = Signal::derive_local(move || rows.with(DirectoryRows::len));
    let any_pending = Signal::derive_local(move || is_loading.get() || loading_more.get());
    let scaled_row_height = Signal::derive_local(|| {
        Gds::try_current().map_or(ROW_HEIGHT, |gds| {
            gds.appearance().typography().scaled(ROW_HEIGHT)
        })
    });
    let scaled_header_height = Signal::derive_local(|| {
        Gds::try_current().map_or(DataListStyle::HEADER_HEIGHT, |gds| {
            gds.appearance()
                .typography()
                .scaled(DataListStyle::HEADER_HEIGHT)
        })
    });

    let blank_press_attrs = Attrs::new().listener(
        events::POINTER_DOWN,
        ListenerOptions::CAPTURE,
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button != Some(PointerButton::Primary) {
                return;
            }
            let Some(bounds) = device_to_css_bounds(surface_ref) else {
                return;
            };
            let scale = list_ref.scale().max(1.0);
            let scroll_y = CssPx(list_ref.scroll_position().offset.y.0 / scale);
            let geometry = ListGeometry {
                rows: row_count.get_untracked(),
                row_height: scaled_row_height.get_untracked(),
                header_height: scaled_header_height.get_untracked(),
                inset_x: 0.0,
                bottom_inset: 0.0,
                scrollbar_width: DataListStyle::SCROLLBAR_LANE,
            };
            if geometry.is_blank(bounds, ev.position, scroll_y) {
                picker_blank.clear_selection();
            }
        },
    );

    view! {
        Picker(
            class = "gds-directory-picker",
            class = class,
            attr:data-testid = "directory-picker",
            style:height = move || Some(format!("{}px", picker_body_height())),
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                handle_picker_keys(&picker_keys, &addr_key_ref, &filter_key_ref, ev);
            }
        ) {
            PickerToolbar {
                IconButton(
                    icon = IconName::ArrowLeft,
                    label = back_label,
                    shortcut = PickerCommand::Back.default_stroke(),
                    disabled = Signal::derive_local(move || !can_back.get()),
                    attr:data-testid = "picker-back",
                    on:click = move |_| picker_back.back(),
                )
                IconButton(
                    icon = IconName::ArrowRight,
                    label = forward_label,
                    shortcut = PickerCommand::Forward.default_stroke(),
                    disabled = Signal::derive_local(move || !can_forward.get()),
                    attr:data-testid = "picker-forward",
                    on:click = move |_| picker_fwd.forward(),
                )
                IconButton(
                    icon = IconName::ArrowUp,
                    label = parent_label,
                    shortcut = PickerCommand::Up.default_stroke(),
                    disabled = Signal::derive_local(move || !can_parent.get()),
                    attr:data-testid = "picker-parent",
                    on:click = move |_| picker_up.parent(),
                )

                PickerLocation {
                    {move || {
                        if editing.get() || page.with(Option::is_none) {
                            let p_input = picker_addr_input.clone();
                            let p_key_enter = picker_addr_submit.clone();
                            let p_key_esc = picker_addr_cancel.clone();
                            AnyView::new(view! {
                                Input(
                                    node_ref = address_ref,
                                    class = "gds-picker-address-input",
                                    value = Binding::controlled(
                                        address_text,
                                        move |next| p_input.set_address(next),
                                    ),
                                    label = text_or_untracked(PickerCommand::EditLocation.label_id(), "Edit location"),
                                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                        if ev.key == Key::Named(NamedKey::Enter) && !ev.repeat {
                                            p_key_enter.apply_address(address_text.get_untracked());
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                        } else if ev.key == Key::Named(NamedKey::Escape) && !ev.repeat {
                                            p_key_esc.cancel_edit_location();
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                        }
                                    }
                                )
                            })
                        } else {
                            let p_crumb = picker_crumb.clone();
                            let full_path = Signal::derive_local(move || {
                                page.with(|p| p.as_ref().map(|loc| loc.location.path.clone()))
                                    .unwrap_or_default()
                            });
                            AnyView::new(view! {
                                HintTooltip(text = full_path) {
                                    box(
                                        class = "gds-picker-location-scroll",
                                        attr:data-testid = "picker-location",
                                    ) {
                                        Breadcrumb(
                                            class = "gds-picker-breadcrumbs",
                                            label = text_or_untracked(PickerCommand::EditLocation.label_id(), "Edit location"),
                                        ) {
                                            BreadcrumbList {
                                                {move || {
                                                    let Some(current_page) = page.get() else {
                                                        return AnyView::new(());
                                                    };
                                                    let current_path = current_page.location.path.clone();
                                                    let current_label = current_page.location.label;
                                                    let ancestors: Vec<(usize, Location)> = current_page
                                                        .ancestors
                                                        .into_iter()
                                                        .filter(|loc| loc.path != current_path)
                                                        .enumerate()
                                                        .collect();
                                                    let p_nav = Signal::stored_local(p_crumb.clone());
                                                    AnyView::new(view! {
                                                        for item in move || ancestors.clone(), key = |(idx, loc)| (*idx, loc.path.clone()) {
                                                            BreadcrumbItem {
                                                                BreadcrumbLink(
                                                                    on:click = {
                                                                        let nav_path = item.1.path.clone();
                                                                        let nav_picker = p_nav.get_untracked();
                                                                        move |_| nav_picker.navigate(nav_path.clone())
                                                                    },
                                                                ) {{item.1.label}}
                                                            }
                                                            BreadcrumbSeparator()
                                                        }
                                                        BreadcrumbItem {
                                                            BreadcrumbPage(class = "gds-picker-current-crumb") {
                                                                {current_label}
                                                            }
                                                        }
                                                    })
                                                }}
                                            }
                                        }
                                    }
                                }
                            })
                        }
                    }}
                }

                IconButton(
                    icon = IconName::Pencil,
                    label = location_label,
                    shortcut = PickerCommand::EditLocation.default_stroke(),
                    attr:data-testid = "picker-location-edit",
                    on:click = move |_| {
                        picker_edit.begin_edit_location();
                        addr_focus_ref.focus();
                        addr_focus_ref.select_all();
                    },
                ) {}

                {move || {
                    let p_act = picker_stop_or_refresh.clone();
                    if any_pending.get() {
                        AnyView::new(view! {
                            IconButton(
                                icon = IconName::X,
                                label = stop_label,
                                attr:data-testid = "picker-stop",
                                on:click = move |_| p_act.cancel_loading(),
                            )
                        })
                    } else {
                        AnyView::new(view! {
                            IconButton(
                                icon = IconName::RefreshCw,
                                label = refresh_label,
                                shortcut = PickerCommand::Refresh.default_stroke(),
                                attr:data-testid = "picker-refresh",
                                on:click = move |_| p_act.refresh(),
                            )
                        })
                    }
                }}
            }

            PickerToolbar {
                box(class = "gds-picker-search-wrap") {
                    InputGroup(class = "gds-picker-search-group") {
                        InputGroupAddon {
                            Icon(name = IconName::Search, size = 16.0)
                        }
                        InputGroupInput(
                            node_ref = filter_ref,
                            placeholder = text_or_untracked(PickerCommand::Filter.label_id(), "Filter folders…"),
                            a11y:label = move || filter_placeholder.get(),
                            value = Binding::controlled(
                                filter_query,
                                move |next| picker_filter.set_filter(next),
                            ),
                        ) {}
                        {move || {
                            if filter_query.get().is_empty() {
                                return None;
                            }
                            let p_clear = picker_filter_clear.clone();
                            let f_ref = filter_ref;
                            Some(
                                view! {
                                    InputGroupAddon(align = InputGroupAddonAlign::InlineEnd) {
                                        InputGroupButton(
                                            size = InputGroupButtonSize::IconXs,
                                            a11y:label = move || clear_filter_label.get(),
                                            on:click = move |_| {
                                                p_clear.set_filter(String::new());
                                                f_ref.focus();
                                            },
                                        ) {
                                            Icon(name = IconName::X, size = 14.0)
                                        }
                                    }
                                }
                                .into_view(),
                            )
                        }}
                    }
                }
                IconButton(
                    icon = IconName::Eye,
                    label = hidden_label,
                    toggled = show_hidden,
                    shortcut = PickerCommand::Hidden.default_stroke(),
                    attr:data-testid = "picker-hidden",
                    on:click = move |_| picker_hidden.toggle_hidden(),
                )
            }

            PickerContent {
                {move || {
                    if is_loading.get() {
                        let target_path = pending_request
                            .get()
                            .and_then(|req| req.path)
                            .unwrap_or_default();
                        let wrapped_target = wrap_tokens(&target_path);
                        let p_cancel = picker_cancel_load.clone();
                        return AnyView::new(view! {
                            box(class = "gds-picker-center", attr:data-testid = "picker-loading") {
                                Empty {
                                    Spinner(label = text_or_untracked("picker-loading", "Loading folders…"))
                                    EmptyHeader {
                                        EmptyTitle {{loading_label.get()}}
                                        {(!wrapped_target.is_empty()).then(|| {
                                            view! {
                                                EmptyDescription {{wrapped_target}}
                                            }
                                            .into_view()
                                        })}
                                    }
                                    EmptyContent {
                                        Button(
                                            variant = ButtonVariant::Outline,
                                            size = ButtonSize::Sm,
                                            on:click = move |_| p_cancel.cancel_loading(),
                                        ) {
                                            Icon(name = IconName::X, size = 16.0)
                                            text {{stop_label.get()}}
                                        }
                                    }
                                }
                            }
                        });
                    }

                    if let Some(err_msg) = error.get() {
                        let p_retry = picker_retry.clone();
                        let wrapped_err = wrap_tokens(&err_msg);
                        return AnyView::new(view! {
                            box(class = "gds-picker-center", attr:data-testid = "picker-failed") {
                                Empty {
                                    EmptyHeader {
                                        EmptyTitle {{failed_title.get()}}
                                        EmptyDescription {{wrapped_err}}
                                    }
                                    EmptyContent {
                                        Button(
                                            variant = ButtonVariant::Outline,
                                            size = ButtonSize::Sm,
                                            attr:data-testid = "picker-retry",
                                            on:click = move |_| p_retry.refresh(),
                                        ) {
                                            Icon(name = IconName::RefreshCw, size = 16.0)
                                            text {{retry_label.get()}}
                                        }
                                    }
                                }
                            }
                        });
                    }

                    if rows.with(DirectoryRows::is_empty) {
                        let msg = if filter_query.get().is_empty() {
                            empty_title.get()
                        } else {
                            no_matches_title.get()
                        };
                        return AnyView::new(view! {
                            box(class = "gds-picker-center", attr:data-testid = "picker-empty") {
                                Empty {
                                    Icon(name = IconName::Folder, size = 24.0)
                                    EmptyHeader {
                                        EmptyTitle {{msg}}
                                    }
                                }
                            }
                        });
                    }

                    let p_row = picker_row.clone();
                    let p_scroll = picker_scroll.clone();
                    let l_ref = list_ref;
                    let blank_attrs = blank_press_attrs.clone();
                    AnyView::new(view! {
                        box(class = "gds-picker-table-host") {
                            ListSurface(
                                node_ref = surface_ref,
                                row_height = scaled_row_height,
                                header_height = scaled_header_height,
                                class = "gds-picker-list-surface",
                                attr:data-style = "plain",
                                {..blank_attrs}
                            ) {
                                box(class = "gds-picker-header") {
                                    text {{move || name_header_label.get()}}
                                }
                                VirtualList(
                                    node_ref = list_ref,
                                    class = "gds-picker-list",
                                    count = row_count,
                                    row_size = scaled_row_height,
                                    overscan = 4,
                                    label = text_or_untracked("picker-folders", "Folders"),
                                    on:scroll = move |_| {
                                        let pos = l_ref.scroll_position();
                                        if pos.content_size.height.0 > 0.0
                                            && pos.remaining().height.0
                                                <= scaled_row_height.get_untracked()
                                                    * (TABLE_LOAD_MORE_THRESHOLD as f32)
                                                    * l_ref.scale()
                                        {
                                            p_scroll.load_more();
                                        }
                                    },
                                    row = move |index: usize| {
                                        let entry = rows.with(|r| r.get(index).cloned());
                                        let Some(entry) = entry else {
                                            return AnyView::new(view! { box() });
                                        };
                                        let path = entry.location.path;
                                        let label = entry.location.label;
                                        let is_hidden = entry.hidden;
                                        let p_click = p_row.clone();
                                        let p_dbl = p_row.clone();
                                        let open_path = path.clone();
                                        let tip_path = Signal::stored_local(path);
                                        AnyView::new(view! {
                                            HintTooltip(text = tip_path) {
                                                control(
                                                    class = "gds-picker-row",
                                                    tabindex = Focus::Sequential,
                                                    a11y:role = Role::ListItem,
                                                    a11y:label = label.clone(),
                                                    attr:data-hidden = is_hidden.then(|| "true".to_owned()),
                                                    attr:data-selected = move || {
                                                        (selected_index.get() == Some(index)).then(|| "true".to_owned())
                                                    },
                                                    on:click = move |_| p_click.select_row(index),
                                                    on:double_click = move |_| p_dbl.navigate(open_path.clone()),
                                                ) {
                                                    Icon(
                                                        name = IconName::Folder,
                                                        size = SIDEBAR_ICON_SIZE,
                                                        class = "gds-picker-row__icon",
                                                    )
                                                    text(class = "gds-picker-row__label") {{label}}
                                                }
                                            }
                                        })
                                    }
                                )
                            }
                        }
                    })
                }}
            }

            PickerStatus {
                {move || {
                    any_pending.get().then(|| {
                        view! {
                            Spinner(label = text_or_untracked("picker-loading", "Loading folders…"))
                        }
                        .into_view()
                    })
                }}
                HintTooltip(text = chosen_path) {
                    text(
                        class = "gds-picker-chosen-path",
                        attr:data-testid = "picker-chosen-path",
                    ) {{move || chosen_path.get()}}
                }
            }
        }
    }
}

/// Modal directory chooser dialog built over [`ModalDialog`] (`DialogSize::Lg`) and [`DirectoryPickerView`].
#[component]
pub fn DirectoryPickerDialog(
    #[prop(into, optional)] open: Binding<bool>,
    picker: DirectoryPicker,
    #[prop(into, optional)] title: Option<Signal<String, LocalStorage>>,
    #[prop(into, optional)] confirm_label: Option<Signal<String, LocalStorage>>,
    #[prop(optional)] anchor: Option<DialogAnchor>,
    on_select: UnsyncCallback<Location>,
    #[prop(optional)] on_cancel: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    install_directory_picker_stylesheets();

    let title =
        title.unwrap_or_else(|| Signal::derive_local(|| text_or("picker-title", "Select folder")));
    let confirm_label = confirm_label
        .unwrap_or_else(|| Signal::derive_local(|| text_or("picker-select", "Select folder")));
    let cancel_label = Signal::derive_local(|| text_or("dialog-cancel", "Cancel"));

    let settled = Rc::new(Cell::new(false));
    let reset_settled = {
        let settled = Rc::clone(&settled);
        RenderEffect::new(move |_| {
            if open.get() == Some(true) {
                settled.set(false);
            }
        })
    };
    on_cleanup_local(move || drop(reset_settled));
    let picker_confirm = picker.clone();
    let picker_cancel_close = picker.clone();
    let can_accept = picker.can_accept();
    let disabled = Signal::derive_local(move || !can_accept.get());
    let editing_loc = picker.is_editing_location();
    let dismiss_on_escape = Signal::derive_local(move || !editing_loc.get());

    let on_decision = {
        let settled = Rc::clone(&settled);
        UnsyncCallback::new(move |decision: DialogDecision| {
            match decision {
                DialogDecision::Confirm => {
                    let Some(location) = picker_confirm.selected_location() else {
                        return;
                    };
                    if settled.replace(true) {
                        return;
                    }
                    open.write(false);
                    on_select.run(location);
                }
                DialogDecision::Cancel | DialogDecision::Dismiss => {
                    if settled.replace(true) {
                        return;
                    }
                    picker_cancel_close.cancel_loading();
                    open.write(false);
                    if let Some(on_cancel) = on_cancel {
                        on_cancel.run(());
                    }
                }
            }
            if let Some(anchor) = anchor {
                anchor.restore();
            }
        })
    };

    view! {
        ModalDialog(
            open = open,
            size = DialogSize::Lg,
            disabled = disabled,
            dismiss_on_escape = dismiss_on_escape,
            close_on_confirm = false,
            anchor = anchor,
            on_decision = on_decision,
            class = "gds-picker-dialog",
        ) {
            {{
                let picker = picker.clone();
                view! {
                    DialogHeader {
                        DialogTitle {{move || title.get()}}
                    }
                    DirectoryPickerView(picker = picker)
                    DialogFooter {
                        DialogActions(
                            confirm_label = confirm_label,
                            cancel_label = cancel_label,
                            disabled = disabled,
                            on_decision = on_decision,
                        )
                    }
                }
            }}
        }
    }
}
