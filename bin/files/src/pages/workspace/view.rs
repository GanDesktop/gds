use std::{
    cell::{Cell, OnceCell, RefCell},
    ffi::OsString,
    rc::Rc,
};

use gds::app::Gds;
use gds::appearance::motion::{WORKSPACE_DURATION_MS, ease_out_cubic, use_transition};
use gds::i18n;
use gds::ui::about::{AboutDialogProps, Application};
use gds::ui::button::{ButtonSize, IconButtonProps};
use gds::ui::feedback::dialog::{ConfirmDialogProps, DialogDecision, DialogSize, ModalDialogProps};
use gds::ui::feedback::notifications::{NotificationHostProps, error as notify_error};
use gds::ui::feedback::tooltip::{HintTooltipProps, wrap_tokens};
use gds::ui::frame::{
    FooterProps, FrameProps, ToolbarProps, WindowDragAreaProps, WorkspaceFrameProps,
};
use gds::ui::interaction::{
    keytips::KeytipHostProps,
    region::{is_focus_visible, surface_with_shortcut, use_keyboard_modality},
};
use gds::ui::list::ListSurfaceProps;
use gds::ui::page::{
    ContentPageProps, PageActionsProps, PageContentProps, PageHeaderProps, PageTableAreaProps,
    PageTitleProps, TABLE_INSET,
};
use gds::ui::time::TimestampViewProps;
use gds::ui::window::{HeightTier, WindowControlsProps};
use gds::ui::{IconName, IconProps};
use zgui::geom::{CssPx, Point};
use zgui::prelude::*;
use zgui::reactive::{
    Owner, RenderEffect, RwSignal, Signal, UnsyncCallback, enter_non_reactive_zone,
    on_cleanup_local,
};
use zgui::view::time::{FrameHandle, Timers};
use zgui::view::{CustomPropertyName, Scoped, current_host, events, install_stylesheet};
use zgui::vocab::{PointerButton, Role, SharedString, Timestamp};
use zgui::{css, view};
use zgui_ui::{ButtonProps, ButtonVariant, SpinnerProps, VirtualListProps, Virtualize};
use zgui_ui_primitives::Binding;

use super::geometry::{GRID_GAP, GRID_PADDING, GridMotion};
use super::marquee;
use super::menus::Surface;
use super::model::{Contents, Pane, Prompt, Row};
use super::name_form::NameForm;
use super::ui::Browser;
use crate::entities::file::ViewMode;
use crate::entities::file::columns::Column;
use crate::entities::file::sorting::Direction;
use crate::entities::file::ui::EntryPresentation;
use crate::entities::location::Location;
use crate::shared::lib::clipboard::Clipboard;
use crate::shared::ui::actions::Command;
use crate::widgets::help::FilesHelpProps;

const DETAILS_WIDTH: f32 = 280.0;
const TAB_ENTER_INITIAL_OPACITY: f32 = 0.88;
const CONFIRM_PREVIEW_LIMIT: usize = 5;
const NAME_MIN_WIDTH: f32 = 180.0;
const NAME_DEFAULT_MAX_WIDTH: f32 = 500.0;
const NAME_WIDTH_FRACTION: f32 = 0.45;
const MODIFIED_WIDTH: f32 = 160.0;
const MODIFIED_MIN_WIDTH: f32 = 144.0;
const KIND_WIDTH: f32 = 110.0;
const KIND_MIN_WIDTH: f32 = 96.0;
const SIZE_WIDTH: f32 = 100.0;
const SIZE_MIN_WIDTH: f32 = 88.0;
const OWNER_WIDTH: f32 = 130.0;
const OWNER_MIN_WIDTH: f32 = 90.0;
const GROUP_WIDTH: f32 = 130.0;
const GROUP_MIN_WIDTH: f32 = 90.0;
const PERMISSIONS_WIDTH: f32 = 96.0;
const PERMISSIONS_MIN_WIDTH: f32 = 84.0;
const ACCESSED_WIDTH: f32 = 160.0;
const ACCESSED_MIN_WIDTH: f32 = 144.0;
const DETAILED_TYPE_WIDTH: f32 = 130.0;
const DETAILED_TYPE_MIN_WIDTH: f32 = 90.0;
const WORKSPACE_SHEET: &str = "gds-files-workspace";

const WORKSPACE_CSS: &str = css!(
    r#"
    .gds-files-root {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-root:focus {
        outline: none;
    }

    .gds-files-frame {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-frame-content {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-toolbar-title {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 0px;
        height: 100%;
        padding: 0px 8px;
        box-sizing: border-box;
    }

    .gds-files-toolbar-title-text {
        font-size: var(--gds-text-sm, 14px);
        font-weight: 400;
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-workspace-outer {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-workspace-column {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-workspace-frame {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-settings-host {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-content-shell {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-title-text {
        min-width: 0px;
        width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-page-actions {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-page-action-strip {
        display: flex;
        align-items: center;
        gap: 4px;
    }

    .gds-files-column-controls {
        display: flex;
        align-items: center;
        gap: 4px;
    }

    .gds-files-content-focus,
    .gds-files-content-focus.gds-region-surface,
    .gds-files-content-focus.gds-region-surface[data-focus-visible="true"],
    .gds-files-content-focus.gds-region-surface[data-focus-visible="true"]:focus,
    .gds-files-content-focus.gds-region-surface[data-focus-visible="true"]:focus-visible {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
        outline: none;
    }

    .gds-files-details-host {
        display: flex;
        flex-direction: column;
        flex: none;
        height: 100%;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-details-pane {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 280px;
        min-width: 280px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-state-center {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        min-width: 0px;
        min-height: 0px;
        align-items: center;
        justify-content: center;
        gap: 12px;
        padding: 24px;
        text-align: center;
        color: var(--gds-muted-foreground);
    }

    .gds-files-list-surface {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        border-width: 0px;
    }

    .gds-files-table-area {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        border-width: 0px;
    }

    .gds-files-state-center[data-focus-visible="true"],
    .gds-files-table-x[data-focus-visible="true"],
    .gds-files-grid-scroll[data-focus-visible="true"] {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-table-x {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow-x: hidden;
        overflow-y: hidden;
    }

    .gds-files-table-x[data-scroll-x="true"] {
        overflow-x: auto;
    }

    .gds-files-table-inner {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        min-height: 0px;
    }

    .gds-files-header-row {
        display: flex;
        flex: none;
        align-items: center;
        width: 100%;
        height: var(--gds-list-header-height, 30px);
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-files-header-cell {
        position: relative;
        display: flex;
        flex: none;
        align-items: center;
        align-self: stretch;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-files-header-button {
        display: flex;
        flex: 1 1 0px;
        align-items: center;
        align-self: stretch;
        justify-content: flex-start;
        gap: 4px;
        min-width: 0px;
        padding: 0px 12px;
        border-width: 0px;
        background-color: transparent;
        font-weight: 400;
        cursor: pointer;
    }

    .gds-files-header-button[data-name-col="true"] {
        padding-left: 40px;
    }

    .gds-files-header-button[data-align-end="true"] {
        justify-content: flex-end;
        text-align: right;
    }

    .gds-files-header-button[data-sortable="false"] {
        cursor: default;
    }

    .gds-files-header-button[data-sortable="true"]:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-header-label {
        flex: 0 1 auto;
        min-width: 0px;
        font-size: var(--gds-text-xs, 12px);
        font-weight: 400;
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-header-button:not([data-align-end="true"]) .gds-files-header-label {
        flex: 1 1 0px;
    }

    .gds-files-header-button[data-active="true"] .gds-files-header-label {
        color: var(--gds-foreground);
    }

    .gds-files-header-sort {
        display: inline-flex;
        flex-direction: column;
        flex: none;
        align-items: center;
        justify-content: center;
        width: 12px;
        height: 18px;
        color: var(--gds-muted-foreground);
        opacity: 0;
        transition: opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-header-button:hover .gds-files-header-sort,
    .gds-files-header-button:focus-visible .gds-files-header-sort {
        opacity: 1;
    }

    .gds-files-header-sort[data-active="true"] {
        opacity: 1;
        color: var(--gds-foreground);
    }

    .gds-files-header-resize {
        position: absolute;
        top: 0px;
        right: 0px;
        bottom: 0px;
        width: 5px;
        cursor: ew-resize;
        z-index: 2;
    }

    .gds-files-header-resize:hover {
        background-color: var(--gds-primary);
    }

    .gds-files-list-host {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-virtual-list {
        flex: 1 1 0px;
        min-height: 0px;
    }

    .gds-files-virtual-list .zui-virtual-list__row {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-files-row-wrap:not([data-grid="true"]) {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        padding: var(--gds-list-selection-inset, 2px) 0px;
        box-sizing: border-box;
    }

    .gds-files-row-wrap[data-grid="true"] {
        width: auto;
        height: auto;
        padding: 0px;
        flex: none;
    }

    .gds-files-row-wrap:not([data-grid="true"]) > .gds-files-menu-wrap,
    .gds-files-row-wrap:not([data-grid="true"]) .gds-files-drag-row-host {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-row-wrap[data-grid="true"] > .gds-files-menu-wrap,
    .gds-files-row-wrap[data-grid="true"] .gds-files-drag-row-host {
        width: auto;
        height: auto;
        flex: none;
    }

    .gds-files-row-content {
        display: flex;
        align-items: center;
        width: 100%;
        height: 100%;
        min-width: 0px;
        border-width: 0px;
        border-radius: 6px;
        user-select: none;
        cursor: pointer;
        transition:
            background-color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-row-wrap:not([data-grid="true"]) .gds-files-row-content {
        flex: 1 1 0px;
        width: auto;
        height: auto;
        min-height: var(--gds-list-selection-height, 36px);
        align-items: center;
    }

    .gds-files-row-content > .zui-tooltip__trigger {
        display: flex;
        align-self: stretch;
        flex: none;
        min-width: 0px;
    }

    .gds-files-row-content:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-row-content[data-selected="true"],
    .gds-files-row-content[data-selected="true"]:hover {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
    }

    .gds-files-row-content[data-cursor="true"]:not([data-selected="true"]) {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-row-content[data-hidden-entry="true"] .gds-files-name-cell,
    .gds-files-row-content[data-hidden-entry="true"] .gds-files-meta-cell,
    .gds-files-row-content[data-hidden-entry="true"] .gds-files-collection-cell {
        opacity: 0.65;
    }

    .gds-files-row-content[data-ghosted="true"] .gds-files-name-cell,
    .gds-files-row-content[data-ghosted="true"] .gds-files-meta-cell,
    .gds-files-row-content[data-ghosted="true"] .gds-files-collection-cell {
        opacity: 0.35;
    }

    .gds-files-name-cell {
        display: flex;
        flex: none;
        align-items: center;
        align-self: stretch;
        gap: 10px;
        padding: 0px 12px;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-files-row-name-group {
        display: flex;
        flex: 1 1 0px;
        align-items: center;
        gap: 6px;
        min-width: 0px;
    }

    .gds-files-row-name-text {
        flex: 0 1 auto;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-cut-icon {
        flex: none;
        color: var(--gds-muted-foreground);
    }

    .gds-files-row-content[data-selected="true"] .gds-files-cut-icon {
        color: var(--gds-selection-foreground);
    }

    .gds-files-meta-cell {
        display: flex;
        flex: none;
        align-items: center;
        align-self: stretch;
        justify-content: flex-start;
        min-width: 0px;
        padding: 0px 12px;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        box-sizing: border-box;
    }

    .gds-files-meta-cell > *,
    .gds-files-meta-cell .zui-tooltip__trigger {
        display: flex;
        min-width: 0px;
        max-width: 100%;
        overflow: hidden;
    }

    .gds-files-meta-cell[data-align-end="true"] {
        justify-content: flex-end;
        text-align: right;
    }

    .gds-files-meta-cell[data-align-end="true"] .gds-files-meta-text {
        text-align: right;
    }

    .gds-files-row-content[data-selected="true"] .gds-files-meta-cell {
        color: var(--gds-selection-foreground);
    }

    .gds-files-meta-cell[data-current-owner="true"] {
        font-weight: 600;
    }

    .gds-files-meta-text {
        width: 100%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-col-name.gds-files-col-name {
        flex: none;
        width: var(--gds-files-col-name);
        min-width: var(--gds-files-col-name);
        max-width: var(--gds-files-col-name);
        box-sizing: border-box;
    }

    .gds-files-col-modified.gds-files-col-modified {
        flex: none;
        width: var(--gds-files-col-modified);
        min-width: var(--gds-files-col-modified);
        max-width: var(--gds-files-col-modified);
        box-sizing: border-box;
    }

    .gds-files-col-accessed.gds-files-col-accessed {
        flex: none;
        width: var(--gds-files-col-accessed);
        min-width: var(--gds-files-col-accessed);
        max-width: var(--gds-files-col-accessed);
        box-sizing: border-box;
    }

    .gds-files-col-type.gds-files-col-type {
        flex: none;
        width: var(--gds-files-col-type);
        min-width: var(--gds-files-col-type);
        max-width: var(--gds-files-col-type);
        box-sizing: border-box;
    }

    .gds-files-col-size.gds-files-col-size {
        flex: none;
        width: var(--gds-files-col-size);
        min-width: var(--gds-files-col-size);
        max-width: var(--gds-files-col-size);
        box-sizing: border-box;
    }

    .gds-files-col-owner.gds-files-col-owner {
        flex: none;
        width: var(--gds-files-col-owner);
        min-width: var(--gds-files-col-owner);
        max-width: var(--gds-files-col-owner);
        box-sizing: border-box;
    }

    .gds-files-col-group.gds-files-col-group {
        flex: none;
        width: var(--gds-files-col-group);
        min-width: var(--gds-files-col-group);
        max-width: var(--gds-files-col-group);
        box-sizing: border-box;
    }

    .gds-files-col-detailed-type.gds-files-col-detailed-type {
        flex: none;
        width: var(--gds-files-col-detailed-type);
        min-width: var(--gds-files-col-detailed-type);
        max-width: var(--gds-files-col-detailed-type);
        box-sizing: border-box;
    }

    .gds-files-col-permissions.gds-files-col-permissions {
        flex: none;
        width: var(--gds-files-col-permissions);
        min-width: var(--gds-files-col-permissions);
        max-width: var(--gds-files-col-permissions);
        box-sizing: border-box;
    }

    .gds-files-grid-scroll {
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
        overflow-y: auto;
        overflow-x: hidden;
    }

    .gds-files-grid-pane {
        display: flex;
        flex-direction: column;
    }

    .gds-files-grid-row {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .gds-files-tile {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: center;
        padding: 8px;
        gap: 8px;
        border-radius: 8px;
        border: 1px solid transparent;
        box-sizing: border-box;
        user-select: none;
        cursor: pointer;
        transition:
            background-color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            border-color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-tile:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-tile[data-selected="true"],
    .gds-files-tile[data-selected="true"]:hover {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
        border-color: var(--gds-primary);
    }

    .gds-files-tile[data-cursor="true"] {
        border-color: var(--gds-primary);
    }

    .gds-files-tile[data-hidden-entry="true"] .gds-files-tile-artwork,
    .gds-files-tile[data-hidden-entry="true"] .gds-files-tile-name-wrap {
        opacity: 0.65;
    }

    .gds-files-tile[data-ghosted="true"] .gds-files-tile-artwork,
    .gds-files-tile[data-ghosted="true"] .gds-files-tile-name-wrap {
        opacity: 0.35;
    }

    .gds-files-tile-cut {
        position: absolute;
        top: 4px;
        right: 4px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        pointer-events: none;
        color: var(--gds-muted-foreground);
    }

    .gds-files-tile[data-selected="true"] .gds-files-tile-cut {
        color: var(--gds-selection-foreground);
    }

    .gds-files-tile-artwork {
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        width: 100%;
    }

    .gds-files-tile-name-wrap {
        display: flex;
        align-items: flex-start;
        justify-content: center;
        width: 100%;
        min-width: 0px;
        overflow: hidden;
        text-align: center;
    }

    .gds-files-tile-name {
        display: block;
        width: 100%;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
        white-space: normal;
        overflow-wrap: anywhere;
        overflow: hidden;
        text-overflow: ellipsis;
        text-align: center;
    }

    .gds-files-directory-surface {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-drop-background {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-menu-wrap {
        display: flex;
        min-width: 0px;
    }

    .gds-files-menu-anchor {
        position: relative;
        display: flex;
        align-items: center;
    }

    .gds-files-menu-item-row {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-menu-item-label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-menu-content,
    .gds-menu-content.gds-menu-content.gds-files-menu-content {
        min-width: 160px;
        max-width: 340px;
        max-height: min(calc(100vh - 18px), var(--zui-popper-available-height, calc(100vh - 18px)));
        overflow-y: auto;
    }

    .gds-files-context-portal-anchor {
        position: fixed;
        inset: 0px;
        pointer-events: none;
    }

    .gds-files-context-point {
        position: fixed;
        width: 1px;
        height: 1px;
        pointer-events: none;
    }

    .gds-files-hidden {
        display: none;
    }

    .gds-files-empty-overlay {
        position: absolute;
        inset: 0px;
        display: flex;
        align-items: center;
        justify-content: center;
        pointer-events: none;
        color: var(--gds-muted-foreground);
    }

    .gds-files-footer-scope {
        width: 100%;
        height: 100%;
    }

    .gds-files-footer-status {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-footer-status-text {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        white-space: nowrap;
    }

    .gds-files-footer-controls {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
    }
    "#
);

pub(super) fn local_memo<T: PartialEq + Clone + 'static>(
    compute: impl Fn() -> T + 'static,
) -> Signal<T, LocalStorage> {
    let owner = Owner::current();
    let owner_fx = owner.clone();
    let compute = Rc::new(compute);
    let compute_fx = Rc::clone(&compute);
    let slot: Rc<OnceCell<RwSignal<T, LocalStorage>>> = Rc::new(OnceCell::new());
    let slot_fx = Rc::clone(&slot);
    let fx = RenderEffect::new(move |_| {
        let next = compute_fx();
        if let Some(signal) = slot_fx.get() {
            if signal.with_untracked(|current| *current != next) {
                signal.set(next);
            }
        } else {
            let signal = match &owner_fx {
                Some(parent) => parent.with(|| RwSignal::new_local(next)),
                None => RwSignal::new_local(next),
            };
            let _ = slot_fx.set(signal);
        }
    });
    on_cleanup_local(move || drop(fx));
    let signal = *slot.get_or_init(|| match &owner {
        Some(parent) => parent.with(|| RwSignal::new_local(compute())),
        None => RwSignal::new_local(compute()),
    });
    Signal::from(signal.read_only())
}

pub(super) fn install_workspace_stylesheets() {
    gds::ui::styles::install_ui_stylesheets();
    if zgui::view::current_host().is_some() {
        install_stylesheet(WORKSPACE_SHEET, WORKSPACE_CSS);
    }
}

/// Animates content opacity `0.88 -> 1.0` over `160ms` (`motion.md`) when switching tabs, while
/// keeping initial mount and reduced-motion switches at `1.0`.
fn use_tab_enter_opacity(active_tab_id: Signal<u64, LocalStorage>) -> Signal<f32, LocalStorage> {
    #[derive(Default)]
    struct TabFadeState {
        started_at: Option<Timestamp>,
        last_frame_at: Option<Timestamp>,
        frame_handle: Option<FrameHandle>,
    }

    let opacity = RwSignal::new_local(1.0_f32);
    let timers = zgui::reactive::is_ui_thread()
        .then(Timers::current)
        .flatten();
    let state = Rc::new(RefCell::new(TabFadeState::default()));

    fn schedule_frame(
        timers: &Timers,
        state: &Rc<RefCell<TabFadeState>>,
        opacity: RwSignal<f32, LocalStorage>,
    ) {
        let timers_next = timers.clone();
        let state_next = Rc::clone(state);
        let handle = timers.request_frame(move |at| {
            let reduced =
                Gds::try_current().is_some_and(|gds| gds.appearance().motion().reduced_untracked());
            if reduced {
                let mut anim = state_next.borrow_mut();
                anim.started_at = None;
                anim.last_frame_at = None;
                anim.frame_handle = None;
                drop(anim);
                opacity.set(1.0);
                return;
            }

            let (value, done) = {
                let mut anim = state_next.borrow_mut();
                let start = match anim.started_at {
                    Some(start) => start,
                    None => {
                        let base = anim.last_frame_at.unwrap_or(at);
                        anim.started_at = Some(base);
                        base
                    }
                };
                anim.last_frame_at = Some(at);
                let elapsed = (at - start).as_secs_f32();
                let duration = (WORKSPACE_DURATION_MS as f32) / 1000.0;
                let progress = (elapsed / duration).clamp(0.0, 1.0);
                if progress >= 1.0 {
                    anim.started_at = None;
                    anim.last_frame_at = None;
                    anim.frame_handle = None;
                    (1.0, true)
                } else {
                    let eased = ease_out_cubic(progress);
                    let current =
                        TAB_ENTER_INITIAL_OPACITY + (1.0 - TAB_ENTER_INITIAL_OPACITY) * eased;
                    (current, false)
                }
            };
            opacity.set(value);
            if !done {
                schedule_frame(&timers_next, &state_next, opacity);
            }
        });
        state.borrow_mut().frame_handle = Some(handle);
    }

    let watch_state = Rc::clone(&state);
    let tab_fx = RenderEffect::new(move |prev_tab: Option<u64>| {
        let current = active_tab_id.get();
        let Some(prev) = prev_tab else {
            return current;
        };
        if prev == current {
            return current;
        }
        let reduced =
            Gds::try_current().is_some_and(|gds| gds.appearance().motion().reduced_untracked());
        if reduced || timers.is_none() {
            let old = {
                let mut anim = watch_state.borrow_mut();
                anim.started_at = None;
                anim.last_frame_at = None;
                anim.frame_handle.take()
            };
            drop(old);
            opacity.set(1.0);
            return current;
        }
        if let Some(ref timers) = timers {
            let old = {
                let mut anim = watch_state.borrow_mut();
                anim.started_at = None;
                anim.last_frame_at = None;
                anim.frame_handle.take()
            };
            drop(old);
            opacity.set(TAB_ENTER_INITIAL_OPACITY);
            schedule_frame(timers, &watch_state, opacity);
        }
        current
    });

    on_cleanup_local(move || {
        drop(tab_fx);
        let old = state.borrow_mut().frame_handle.take();
        drop(old);
    });

    opacity.into()
}

#[derive(Clone, PartialEq)]
enum FileBody {
    Loading(Location),
    Empty(&'static str),
    Network,
    Collection(super::surfaces::CollectionBodyState),
    List,
    Grid,
}

#[derive(Clone, PartialEq)]
enum HeaderStructure {
    Collection {
        columns: Vec<&'static str>,
        sortable: Vec<Column>,
        sort: crate::entities::file::sorting::Sort,
        lang: Option<gds::i18n::Language>,
    },
    Directory {
        columns: crate::entities::file::columns::Layout,
        sortable: Vec<Column>,
        sort: crate::entities::file::sorting::Sort,
        lang: Option<gds::i18n::Language>,
    },
}

impl Browser {
    pub fn view(&self) -> impl IntoView + use<> {
        let browser = self.clone();
        Scoped::new(move || browser.view_scoped())
    }

    fn view_scoped(&self) -> impl IntoView + use<> {
        install_workspace_stylesheets();

        let keytip_active = RwSignal::new_local(false);
        let hint_context = self.clone();
        let keytip_guard_fx = RenderEffect::new(move |_| {
            let menu_open = !matches!(hint_context.0.menus.surface.get(), Surface::Closed);
            if menu_open && keytip_active.get_untracked() {
                keytip_active.set(false);
            }
        });
        on_cleanup_local(move || drop(keytip_guard_fx));

        // `WindowHandle::size()` is already in `CssPx` and starts `(0.0, 0.0)` before Wayland
        // configure; ignore non-positive dimensions and do not divide by scale (#114).
        if let Some(window) = self.0.window.clone() {
            let width_sig = self.0.window_width;
            let height_sig = self.0.window_height;
            let size = window.size();
            let win_fx = RenderEffect::new(move |_| {
                let sz = size.get();
                let width = sz.width.0;
                let height = sz.height.0;
                if width.is_finite() && width > 0.0 {
                    width_sig.set(width);
                }
                if height.is_finite() && height > 0.0 {
                    height_sig.set(height);
                }
            });
            on_cleanup_local(move || drop(win_fx));
        } else {
            let root = self.0.root_focus;
            let width_sig = self.0.window_width;
            let height_sig = self.0.window_height;
            let root_size = root.observe_border_size();
            let root_fx = RenderEffect::new(move |_| {
                if let Some(sz) = root_size.get() {
                    let s = root.scale().max(1.0);
                    let width = sz.width.0 / s;
                    let height = sz.height.0 / s;
                    if width.is_finite() && width > 0.0 {
                        width_sig.set(width);
                    }
                    if height.is_finite() && height > 0.0 {
                        height_sig.set(height);
                    }
                }
            });
            on_cleanup_local(move || drop(root_fx));
        }

        let help = local_memo({
            let browser = self.clone();
            move || browser.pane() == Pane::Help
        });

        let root_focus = self.0.root_focus;
        let notice_sig = self.0.notice;

        let keyboard = self.clone();
        let init_focus_browser = self.clone();
        let mouse = self.clone();
        let main_browser = self.clone();
        let prompt_browser = self.clone();
        let connector_view = self.0.connector.view();
        let menu_overlay = self.menu_overlay();

        view! {
            KeytipHost(active = Some(keytip_active)) {
                NotificationHost {
                    {{
                        let notice_fx = RenderEffect::new(move |_| {
                            if let Some(message) = notice_sig.get() {
                                let _z = enter_non_reactive_zone();
                                notify_error(message);
                            }
                        });
                        on_cleanup_local(move || drop(notice_fx));

                        let prompt_dialog = prompt_browser.prompt_dialogs();

                        let shortcut_guard: Rc<RefCell<Option<zgui::view::WindowShortcut>>> =
                            Rc::new(RefCell::new(None));
                        let shortcut_guard_init = Rc::clone(&shortcut_guard);
                        let shortcut_guard_fx_slot = Rc::clone(&shortcut_guard);
                        let shortcut_guard_cleanup = Rc::clone(&shortcut_guard);
                        let shortcut_fx = RenderEffect::new(move |_| {
                            if root_focus.get().is_some()
                                && shortcut_guard_fx_slot.borrow().is_none()
                            {
                                *shortcut_guard_fx_slot.borrow_mut() =
                                    root_focus.window_shortcut();
                            }
                        });
                        on_cleanup_local(move || {
                            drop(shortcut_fx);
                            drop(shortcut_guard_cleanup.borrow_mut().take());
                        });

                        let content_focus = init_focus_browser.0.content_focus;
                        let init_focus_fx = RenderEffect::new(move |_| {
                            if content_focus.get().is_some()
                                && init_focus_browser.pane_untracked() == Pane::Files
                                && matches!(
                                    init_focus_browser.0.address_presentation.get_untracked(),
                                    super::model::AddressPresentation::Breadcrumbs
                                )
                                && !gds::ui::keytips::has_active_modal_scope()
                                && current_host()
                                    .is_some_and(|host| host.focused().get_untracked().is_none())
                            {
                                content_focus.focus();
                            }
                        });
                        on_cleanup_local(move || drop(init_focus_fx));

                        let sync_shortcut = Scoped::new(move || {
                            if shortcut_guard_init.borrow().is_none() {
                                *shortcut_guard_init.borrow_mut() = root_focus.window_shortcut();
                            }
                        });

                        view! {
                            column(
                                node_ref = root_focus,
                                class = "gds-files-root",
                                tabindex = Focus::Sequential,
                                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                    keyboard.key_down(ev);
                                },
                                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                                    mouse.mouse_navigation(ev);
                                }
                            ) {
                                {{move || {
                                    if help.get() {
                                        main_browser.help_view()
                                    } else {
                                        main_browser.workspace_view()
                                    }
                                }}}
                                {connector_view}
                                {menu_overlay}
                                {prompt_dialog}
                            }
                            {sync_shortcut}
                        }
                    }}
                }
            }
        }
    }

    pub(super) fn subpage_toolbar_title(label_key: &'static str) -> AnyView {
        super::chrome::install_chrome_stylesheets();

        AnyView::new(view! {
            WindowDragArea(class = "gds-files-toolbar-title") {
                text(class = "gds-files-toolbar-title-text") {
                    {move || i18n::text(label_key)}
                }
            }
        })
    }

    fn help_view(&self) -> AnyView {
        let root_ref = self.0.settings_node;
        let back_click = self.clone();

        let back_button = AnyView::new(view! {
            row(class = "gds-files-menu-controls") {
                IconButton(
                    icon = IconName::ArrowLeft,
                    label = Signal::derive_local(|| i18n::text("page-return")),
                    shortcut = Some(Command::Cancel.shortcut()),
                    on:click = move |_| back_click.back_page()
                )
            }
        });
        let title_controls = AnyView::new(view! {
            row(class = "gds-files-toolbar") {
                {Self::subpage_toolbar_title("help")}
            }
        });
        let help_body = AnyView::new(view! {
            box(
                node_ref = root_ref,
                tabindex = Focus::Sequential,
                class = "gds-files-settings-host"
            ) {
                FilesHelp()
            }
        });
        let empty_status = AnyView::new(view! { box() });

        AnyView::new(view! {
            Frame(without_sidebar = true, class = "gds-files-frame") {
                Toolbar(
                    menu = back_button,
                    controls = title_controls,
                    window_controls = AnyView::new(view! { WindowControls() })
                ) {}
                WorkspaceFrame(
                    content = help_body,
                    paint_content_surface = true,
                    class = "gds-files-workspace-frame"
                ) {}
                Footer(status = empty_status) {}
            }
        })
    }

    fn workspace_view(&self) -> AnyView {
        let browser = self.clone();
        AnyView::new(Scoped::new(move || browser.workspace_view_scoped()))
    }

    fn workspace_view_scoped(&self) -> AnyView {
        let pane_memo = local_memo({
            let browser = self.clone();
            move || browser.pane()
        });
        let sidebar_width = self.sidebar_width();
        let sidebar_pres = self.sidebar_presentation();

        let workspace_col = NodeRef::new();
        let workspace_size = workspace_col.observe_border_size();
        let window_scale = self.0.window.as_ref().map(|window| window.scale());
        let workspace_width = Signal::derive_local(move || {
            let size = workspace_size.get()?;
            let scale = window_scale.map_or_else(|| workspace_col.scale(), |scale| scale.get());
            Some(size.width.0 / scale)
        });
        let resize_browser = self.clone();
        let term_size_fx = RenderEffect::new(move |_| {
            if let Some(sz) = workspace_size.get() {
                let s = workspace_col.scale().max(1.0);
                let w = sz.width.0 / s;
                let h = sz.height.0 / s;
                if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
                    resize_browser.terminal_workspace_resized(w, h);
                }
            }
        });
        on_cleanup_local(move || drop(term_size_fx));

        let picker_is_none = Signal::derive_local({
            let browser = self.clone();
            move || browser.read(|state| state.picker.is_none())
        });

        let sidebar_browser = self.clone();
        let content_browser = self.clone();
        let terminal_browser = self.clone();
        let status_browser = self.clone();
        let footer_ctrl_browser = self.clone();

        let toolbar_left = self.toolbar_leading();
        let toolbar_controls = self.toolbar_controls();
        let sidebar_scope = AnyView::new(view! { box(class = "gds-files-footer-scope") });
        let footer_status = AnyView::new(view! {
            {{move || {
                if pane_memo.get() == Pane::Files {
                    let sb = status_browser.clone();
                    if let Some(picker_name) = sb.render_picker_name() {
                        picker_name
                    } else {
                        let status_controls = sb.status_controls();
                        let filter_b = sb.clone();
                        let show_micro_filter = Signal::derive_local(move || {
                            filter_b.height_tier() == HeightTier::Micro
                                && filter_b.filter_available()
                        });
                        let micro_b = sb.clone();
                        AnyView::new(view! {
                            row(class = "gds-files-footer-status") {
                                text(class = "gds-files-footer-status-text") {{move || sb.status()}}
                                Show(when = move || show_micro_filter.get()) {
                                    {{micro_b.micro_filter(false)}}
                                }
                                {status_controls}
                            }
                        })
                    }
                } else {
                    status_browser.0.settings.footer_text()
                }
            }}}
        });
        let footer_controls = AnyView::new(view! {
            Show(when = move || pane_memo.get() == Pane::Files) {
                {{
                    footer_ctrl_browser
                        .render_picker_controls()
                        .unwrap_or_else(|| footer_ctrl_browser.footer_controls())
                }}
            }
        });

        AnyView::new(view! {
            Frame(sidebar_width = sidebar_width, class = "gds-files-frame") {
                Toolbar(
                    left = toolbar_left,
                    controls = toolbar_controls,
                    window_controls = AnyView::new(view! { WindowControls() })
                ) {}
                {{move || match pane_memo.get() {
                    Pane::Settings => {
                        let sb = sidebar_browser.clone();
                        let cb = content_browser.clone();
                        let pres_initial = sidebar_pres.get_untracked();
                        let settings_sidebar = sb.0.settings.sidebar(pres_initial);
                        let settings_content = AnyView::new(view! {
                            box(
                                node_ref = cb.0.settings_node,
                                tabindex = Focus::Sequential,
                                class = "gds-files-settings-host"
                            ) {
                                {{cb.0.settings.content()}}
                            }
                        });
                        AnyView::new(view! {
                            WorkspaceFrame(
                                sidebar = settings_sidebar,
                                content = settings_content,
                                paint_content_surface = true,
                                class = "gds-files-workspace-frame"
                            ) {}
                        })
                    }
                    Pane::Files => {
                        let pres_initial = sidebar_pres.get_untracked();
                        let files_sidebar = sidebar_browser.sidebar(pres_initial);
                        let files_content = content_browser.file_content(workspace_width);
                        let drag_overlay = content_browser.drag_overlay();
                        let term_b = terminal_browser.clone();
                        AnyView::new(view! {
                            WorkspaceFrame(
                                paint_content_surface = false,
                                class = "gds-files-workspace-outer"
                            ) {
                                column(node_ref = workspace_col, class = "gds-files-workspace-column") {
                                    WorkspaceFrame(
                                        sidebar = files_sidebar,
                                        content = files_content,
                                        paint_content_surface = true,
                                        class = "gds-files-workspace-frame"
                                    ) {}
                                    Show(when = move || picker_is_none.get()) {
                                        {{term_b.terminal_view(pres_initial)}}
                                    }
                                    {drag_overlay}
                                }
                            }
                        })
                    }
                    Pane::Help => AnyView::new(view! { box(class = "gds-files-hidden") }),
                }}}
                Footer(
                    sidebar_status = sidebar_scope,
                    status = footer_status,
                    controls = footer_controls
                ) {}
            }
        })
    }

    fn footer_controls(&self) -> AnyView {
        let terminal_sig = self.0.terminal;
        let click_browser = self.clone();
        let terminal_toggled = Signal::derive_local(move || terminal_sig.get());
        let terminal_label = Signal::derive_local(move || {
            i18n::text(if terminal_sig.get() {
                "files-terminal-hide"
            } else {
                "files-terminal-show"
            })
        });
        let terminal_enabled = local_memo({
            let browser = self.clone();
            move || browser.0.preferences.state().with(|p| p.terminal_enabled())
        });
        let visible = Signal::derive_local({
            let browser = self.clone();
            move || browser.read(|state| state.picker.is_none()) && terminal_enabled.get()
        });

        AnyView::new(view! {
            row(class = "gds-files-footer-controls") {
                Show(when = move || visible.get()) {
                    {{
                        let clk_b = click_browser.clone();
                        let term_btn_ref = NodeRef::new();
                        #[cfg(test)]
                        tests::record_footer_terminal_node(term_btn_ref);
                        view! {
                            IconButton(
                                node_ref = term_btn_ref,
                                icon = IconName::Terminal,
                                size = ButtonSize::IconXs,
                                label = terminal_label,
                                toggled = Some(terminal_toggled),
                                shortcut = Some(Command::Terminal.shortcut()),
                                a11y:role = Role::Button,
                                on:click = move |_| clk_b.toggle_terminal()
                            )
                        }
                    }}
                }
            }
        })
    }

    fn control(
        &self,
        icon: IconName,
        label: &'static str,
        action: fn(&Browser),
        command: Option<Command>,
        active: Option<Signal<bool, LocalStorage>>,
    ) -> AnyView {
        let click_browser = self.clone();
        let shortcut = command.map(Command::shortcut);
        let active_sig = active.unwrap_or_else(|| Signal::stored_local(false));

        AnyView::new(view! {
            IconButton(
                icon = icon,
                label = Signal::derive_local(move || i18n::text(label)),
                active = active_sig,
                shortcut = shortcut,
                on:click = move |_| action(&click_browser)
            )
        })
    }

    fn page_title(&self) -> String {
        self.read(|state| {
            let location = state.tab().navigation.location();
            if *location == Location::from(state.home.as_path()) {
                return i18n::text("home");
            }
            if let Location::Volume { identity, relative } = location
                && relative.is_empty()
                && let Some(volume) = self.volume_at_location(identity)
            {
                return volume.name;
            }
            self.location_title(location)
        })
    }

    fn details_available(&self) -> bool {
        self.details_available_structure()
    }

    pub(super) fn details_available_structure(&self) -> bool {
        self.read_structure(|state| {
            let tab = state.tab();
            match tab.contents {
                Contents::Directory | Contents::Places(_) | Contents::Recent(_) => true,
                Contents::Tags { .. } => !tab.is_pure_tag_catalogue(),
                Contents::LoadingCollection
                | Contents::Unavailable(_)
                | Contents::Network { .. }
                | Contents::Trash(_)
                | Contents::Remote { .. }
                | Contents::Volumes => false,
            }
        })
    }

    fn file_content(&self, workspace_width: Signal<Option<f32>, LocalStorage>) -> AnyView {
        let browser = self.clone();
        AnyView::new(Scoped::new(move || {
            browser.file_content_scoped(workspace_width)
        }))
    }

    fn file_content_scoped(&self, workspace_width: Signal<Option<f32>, LocalStorage>) -> AnyView {
        let title_browser = self.clone();
        let content_browser = self.clone();
        let details_browser = self.clone();
        let actions_browser = self.clone();
        let notice_browser = self.clone();
        let focus_browser = self.clone();
        let search_text_browser = self.clone();
        let search_ime_browser = self.clone();

        let content_ref = self.0.content_focus;

        let region_attrs =
            surface_with_shortcut(content_ref, Some(Command::FocusContent.shortcut()));
        let keyboard_modality = use_keyboard_modality(content_ref);
        let visible_sig = self.0.list_focus_visible;
        {
            let sync_browser = focus_browser.clone();
            let list_ref = self.0.list;
            let focus_sync_fx = RenderEffect::new(move |_| {
                let direct_ring = is_focus_visible(content_ref);
                let is_kbd = keyboard_modality.get();
                if let Some(host) = current_host() {
                    let active = host.focused().get();
                    let self_id = content_ref.get();
                    let direct = self_id.is_some() && active == self_id;
                    let inside =
                        active.is_some_and(|id| self_id == Some(id) || content_ref.contains(id));
                    sync_browser.0.list_focused.set(inside);
                    let on_surface = direct
                        || active.is_some_and(|id| {
                            list_ref.get_untracked() == Some(id) || list_ref.contains(id)
                        });
                    if !on_surface {
                        sync_browser.reset_typeahead();
                    }
                    if direct_ring {
                        if !visible_sig.get_untracked() {
                            visible_sig.set(true);
                        }
                    } else if (!direct || !is_kbd) && visible_sig.get_untracked() {
                        visible_sig.set(false);
                    }
                } else if visible_sig.get_untracked() != direct_ring {
                    visible_sig.set(direct_ring);
                }
            });
            on_cleanup_local(move || drop(focus_sync_fx));
        }

        let active_tab_id = local_memo({
            let browser = self.clone();
            move || browser.read_structure(|state| state.tab().id)
        });
        let tab_opacity = use_tab_enter_opacity(active_tab_id);

        let body = local_memo({
            let browser = self.clone();
            move || browser.file_body()
        });
        let is_network = local_memo(move || matches!(body.get(), FileBody::Network));

        let details_goal = Signal::derive_local({
            let browser = self.clone();
            move || {
                if browser.details_permitted()
                    && browser.0.details.get()
                    && browser.details_available()
                {
                    1.0_f32
                } else {
                    0.0_f32
                }
            }
        });
        let details_progress = use_transition(details_goal, WORKSPACE_DURATION_MS);
        let details_mounted = Signal::derive_local({
            let browser = self.clone();
            move || browser.details_permitted() && details_progress.get() > 0.001
        });

        let details_width = Signal::derive_local(move || {
            if details_mounted.get() {
                DETAILS_WIDTH * details_progress.get()
            } else {
                0.0
            }
        });

        let content_width = self.0.content_width;
        let sidebar_width = self.sidebar_width();
        // Use the same pane widths as layout so column and grid bindings settle before layout.
        let size_fx = RenderEffect::new(move |_| {
            if let Some(total) = workspace_width
                .get()
                .filter(|width| width.is_finite() && *width > 0.0)
            {
                let width = (total - sidebar_width.get() - details_width.get()).max(1.0);
                if width != content_width.get_untracked() {
                    content_width.set(width);
                }
            }
        });
        on_cleanup_local(move || drop(size_fx));

        let float_check_b = self.clone();
        let show_floating_filter = Signal::derive_local(move || {
            float_check_b.is_save_picker()
                && float_check_b.height_tier() == HeightTier::Micro
                && float_check_b.filter_available()
        });
        let float_filter_b = self.clone();

        AnyView::new(view! {
            row(class = "gds-files-content-shell") {
                box(
                    node_ref = content_ref,
                    class = "gds-files-content-focus",
                    tabindex = Focus::Sequential,
                    style:opacity = move || {
                        let alpha = tab_opacity.get();
                        (alpha < 0.999).then(|| format!("{alpha:.3}"))
                    },
                    {..region_attrs},
                    on:text = move |ev: &mut EventCx<'_, events::Text>| {
                        if search_text_browser.handle_content_text_input(ev.text.as_str(), ev.timestamp) {
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    },
                    on:ime_commit = move |ev: &mut EventCx<'_, events::ImeCommit>| {
                        if let zgui::vocab::ImeEvent::Commit(committed) = &**ev
                            && search_ime_browser.handle_content_text_input(committed.as_str(), ev.timestamp)
                        {
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    }
                ) {
                    {{move || {
                        if is_network.get() {
                            // `surfaces/network.rs` renders its own `ContentPage` and `HeightTier::Normal` `PageHeader`.
                            let net_browser = content_browser.clone();
                            let picker_footer = net_browser.picker_footer();
                            AnyView::new(view! {
                                column(class = "gds-files-workspace-column") {
                                    {{net_browser.network_content()}}
                                    {picker_footer}
                                }
                            })
                        } else {
                            let tb = title_browser.clone();
                            let ab = actions_browser.clone();
                            let nb = notice_browser.clone();
                            let cb = content_browser.clone();
                            let fb = content_browser.clone();
                            let hb = content_browser.clone();
                            let normal_height =
                                Signal::derive_local(move || hb.height_tier() == HeightTier::Normal);
                            let picker_footer = cb.picker_footer();
                            AnyView::new(view! {
                                ContentPage {
                                    Show(when = move || normal_height.get()) {
                                        {{
                                            let tb_hdr = tb.clone();
                                            let ab_hdr = ab.clone();
                                            let fb_hdr = fb.clone();
                                            let filter_view = fb_hdr.folder_filter();
                                            view! {
                                                PageHeader {
                                                    PageTitle {
                                                        text(class = "gds-files-title-text") {{move || tb_hdr.page_title()}}
                                                    }
                                                    PageActions(class = "gds-files-page-actions") {
                                                        {filter_view}
                                                        {move || ab_hdr.page_actions()}
                                                    }
                                                }
                                            }
                                        }}
                                    }
                                    {move || nb.collection_notice()}
                                    PageContent {
                                        {{move || {
                                            let current = body.get();
                                            let _z = enter_non_reactive_zone();
                                            cb.render_file_body(current)
                                        }}}
                                        {picker_footer}
                                    }
                                }
                            })
                        }
                    }}}
                    Show(when = move || show_floating_filter.get()) {
                        {{float_filter_b.micro_filter(true)}}
                    }
                }
                Show(when = move || details_mounted.get()) {
                    {{
                        let d_browser = details_browser.clone();
                        view! {
                            box(
                                class = "gds-files-details-host",
                                style:width = move || {
                                    Some(format!("{:.1}px", details_width.get()))
                                }
                            ) {
                                box(class = "gds-files-details-pane") {
                                    {{d_browser.details_view()}}
                                }
                            }
                        }
                    }}
                }
            }
        })
    }

    fn file_body(&self) -> FileBody {
        if let Some(location) = self.read(|state| {
            let tab = state.tab();
            tab.navigation.is_loading().then(|| {
                tab.navigation
                    .pending_location()
                    .unwrap_or_else(|| tab.navigation.location())
                    .clone()
            })
        }) {
            return FileBody::Loading(location);
        }
        if let Some(collection) = self.collection_body_state() {
            return FileBody::Collection(collection);
        }
        self.read(|state| {
            let tab = state.tab();
            match &tab.contents {
                Contents::Remote { .. } if tab.rows.is_empty() && tab.navigation.is_loading() => {
                    FileBody::Loading(tab.navigation.location().clone())
                }
                Contents::Network { .. } => FileBody::Network,
                _ => {
                    if tab.rows.is_empty() {
                        FileBody::Empty(if self.0.filter.with(String::is_empty) {
                            "empty-directory"
                        } else {
                            "empty-filter"
                        })
                    } else if tab.mode == ViewMode::Grid {
                        FileBody::Grid
                    } else {
                        FileBody::List
                    }
                }
            }
        })
    }

    fn render_file_body(&self, body: FileBody) -> AnyView {
        match body {
            FileBody::Loading(location) => self.location_loading(&location),
            FileBody::Collection(state) => self.background_drop_target(state.view(self)),
            FileBody::Network => self.background_drop_target(self.network_content()),
            FileBody::Empty(message) => {
                let target = self.empty_view(message);
                self.directory_menu(self.background_drop_target(target))
            }
            FileBody::List => self.list_view(),
            FileBody::Grid => self.grid_view(),
        }
    }

    fn location_loading(&self, location: &Location) -> AnyView {
        let message = match location {
            Location::Volume { identity, .. } => {
                let label = self
                    .volume_at_location(identity)
                    .map_or_else(|| location.label(), |volume| volume.name);
                gds::t!("loading-volume", name = label)
            }
            _ => i18n::text("loading"),
        };
        let cancel = self.clone();
        AnyView::new(view! {
            column(class = "gds-files-state-center") {
                Spinner()
                text {{message}}
                Button(
                    variant = ButtonVariant::Outline,
                    on:click = move |_| cancel.cancel_location_load(),
                ) {{i18n::text("operation-cancel")}}
            }
        })
    }

    fn empty_view(&self, message: &'static str) -> AnyView {
        let focus_visible = self.0.list_focus_visible;
        AnyView::new(view! {
            PageTableArea(class = "gds-files-table-area") {
                column(
                    class = "gds-files-state-center",
                    attr:data-focus-visible = move || focus_visible.get().then(|| "true".to_owned())
                ) {
                    text {{i18n::text(message)}}
                }
            }
        })
    }

    const fn column_width_class(column: Column) -> &'static str {
        match column {
            Column::Name => "gds-files-col-name",
            Column::Modified => "gds-files-col-modified",
            Column::Accessed => "gds-files-col-accessed",
            Column::Type => "gds-files-col-type",
            Column::Size => "gds-files-col-size",
            Column::Owner => "gds-files-col-owner",
            Column::Group => "gds-files-col-group",
            Column::DetailedType => "gds-files-col-detailed-type",
            Column::Permissions => "gds-files-col-permissions",
        }
    }

    const fn column_custom_property(column: Column) -> &'static str {
        match column {
            Column::Name => "gds-files-col-name",
            Column::Modified => "gds-files-col-modified",
            Column::Accessed => "gds-files-col-accessed",
            Column::Type => "gds-files-col-type",
            Column::Size => "gds-files-col-size",
            Column::Owner => "gds-files-col-owner",
            Column::Group => "gds-files-col-group",
            Column::DetailedType => "gds-files-col-detailed-type",
            Column::Permissions => "gds-files-col-permissions",
        }
    }

    fn table_column_attrs(&self) -> Attrs {
        let mut attrs = Attrs::new();
        for column in Column::ALL {
            let browser = self.clone();
            let width = local_memo(move || browser.column_width(column));
            attrs = attrs.custom_property(
                CustomPropertyName::new(Self::column_custom_property(column)),
                move || Some(format!("{:.2}px", width.get())),
            );
        }
        let collection_browser = self.clone();
        let collection_columns = local_memo(move || collection_browser.collection_columns());
        for index in 0..super::surfaces::COLLECTION_MAX_COLUMNS {
            let width = local_memo(move || {
                collection_columns.with(|cols| {
                    cols.as_ref()
                        .and_then(|items| items.get(index).map(|(_, w)| *w))
                })
            });
            attrs = attrs.custom_property(
                CustomPropertyName::new(super::surfaces::collection_column_class(index)),
                move || width.get().map(|w| format!("{w:.2}px")),
            );
        }
        attrs
    }

    fn column_min_width(column: Column) -> f32 {
        match column {
            Column::Name => NAME_MIN_WIDTH,
            Column::Modified => MODIFIED_MIN_WIDTH,
            Column::Accessed => ACCESSED_MIN_WIDTH,
            Column::Type => KIND_MIN_WIDTH,
            Column::Size => SIZE_MIN_WIDTH,
            Column::Owner => OWNER_MIN_WIDTH,
            Column::Group => GROUP_MIN_WIDTH,
            Column::DetailedType => DETAILED_TYPE_MIN_WIDTH,
            Column::Permissions => PERMISSIONS_MIN_WIDTH,
        }
    }

    pub(super) fn table_viewport_width_untracked(&self) -> f32 {
        (self.0.content_width.get_untracked() - TABLE_INSET * 2.0)
            .floor()
            .max(1.0)
    }

    fn column_default_width(&self, column: Column) -> f32 {
        match column {
            Column::Name => (self.table_viewport_width() * NAME_WIDTH_FRACTION)
                .clamp(NAME_MIN_WIDTH, NAME_DEFAULT_MAX_WIDTH),
            Column::Modified => MODIFIED_WIDTH,
            Column::Accessed => ACCESSED_WIDTH,
            Column::Type => KIND_WIDTH,
            Column::Size => SIZE_WIDTH,
            Column::Owner => OWNER_WIDTH,
            Column::Group => GROUP_WIDTH,
            Column::DetailedType => DETAILED_TYPE_WIDTH,
            Column::Permissions => PERMISSIONS_WIDTH,
        }
    }

    fn column_default_width_untracked(&self, column: Column) -> f32 {
        match column {
            Column::Name => (self.table_viewport_width_untracked() * NAME_WIDTH_FRACTION)
                .clamp(NAME_MIN_WIDTH, NAME_DEFAULT_MAX_WIDTH),
            Column::Modified => MODIFIED_WIDTH,
            Column::Accessed => ACCESSED_WIDTH,
            Column::Type => KIND_WIDTH,
            Column::Size => SIZE_WIDTH,
            Column::Owner => OWNER_WIDTH,
            Column::Group => GROUP_WIDTH,
            Column::DetailedType => DETAILED_TYPE_WIDTH,
            Column::Permissions => PERMISSIONS_WIDTH,
        }
    }

    fn column_width(&self, column: Column) -> f32 {
        let min = Self::column_min_width(column);
        let explicit = self
            .0
            .column_widths
            .with(|widths| widths.get(&column).copied());
        if let Some(width) = explicit {
            return width.max(min);
        }
        self.column_default_width(column).max(min)
    }

    fn column_width_untracked(&self, column: Column) -> f32 {
        let min = Self::column_min_width(column);
        let explicit = self
            .0
            .column_widths
            .with_untracked(|widths| widths.get(&column).copied());
        if let Some(width) = explicit {
            return width.max(min);
        }
        self.column_default_width_untracked(column).max(min)
    }

    fn content_columns_width(&self) -> f32 {
        if let Some(columns) = self.collection_columns() {
            return columns.into_iter().map(|(_, width)| width).sum::<f32>();
        }
        self.read(|state| {
            state
                .tab()
                .columns
                .columns()
                .iter()
                .map(|column| self.column_width(*column))
                .sum::<f32>()
        })
    }

    fn table_width(&self) -> f32 {
        self.content_columns_width()
            .max(self.table_viewport_width())
    }

    fn column_header(&self, column: Column) -> AnyView {
        let sort_browser = self.clone();
        let resize_drag = self.clone();
        let resize_cancel = self.clone();
        let resize_end = self.clone();
        let sortable = self.sortable_columns_untracked().contains(&column);
        let sorting = self.read_untracked(|state| state.tab().sort);
        let active = sortable && sorting.key == column;
        let ascending = sorting.direction == Direction::Ascending;
        let align_end = column == Column::Size;

        let drag_state: RwSignal<Option<(f32, f32)>, LocalStorage> = RwSignal::new_local(None);
        let header_cell_ref = NodeRef::new();
        let resize_ref = NodeRef::new();
        #[cfg(test)]
        {
            tests::record_header_cell_node(column, header_cell_ref);
            tests::record_header_resize_node(column, resize_ref);
        }
        let window = try_use_window();

        let sort_indicator = sortable.then(|| {
            AnyView::new(view! {
                box(
                    class = "gds-files-header-sort",
                    attr:data-active = active.then(|| "true".to_owned()),
                    a11y:hidden = true
                ) {
                    {if active {
                        let icon = if ascending {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        };
                        AnyView::new(view! {
                            Icon(name = icon, size = 12.0)
                        })
                    } else {
                        AnyView::new(view! {
                            column(style:align-items = "center", style:justify-content = "center") {
                                Icon(name = IconName::ChevronUp, size = 8.0) {}
                                Icon(name = IconName::ChevronDown, size = 8.0) {}
                            }
                        })
                    }}
                }
            })
        });

        let label_text = i18n::text(column.label_id());
        let label_node = AnyView::new(view! {
            text(class = "gds-files-header-label") {{label_text.clone()}}
        });

        AnyView::new(view! {
            row(
                node_ref = header_cell_ref,
                class = format!("gds-files-header-cell {}", Self::column_width_class(column))
            ) {
                control(
                    class = "gds-files-header-button",
                    attr:data-name-col = (column == Column::Name).then(|| "true".to_owned()),
                    attr:data-align-end = align_end.then(|| "true".to_owned()),
                    attr:data-sortable = if sortable { "true" } else { "false" },
                    attr:data-active = active.then(|| "true".to_owned()),
                    tabindex = if sortable { Focus::Sequential } else { Focus::Programmatic },
                    a11y:role = Role::Button,
                    a11y:label = SharedString::from(label_text),
                    a11y:disabled = !sortable,
                    on:click = move |_| {
                        if sortable {
                            sort_browser.sort_by(column);
                        }
                    }
                ) {
                    {if align_end {
                        AnyView::new(view! {
                            {sort_indicator}
                            {label_node}
                        })
                    } else {
                        AnyView::new(view! {
                            {label_node}
                            {sort_indicator}
                        })
                    }}
                }
                box(
                    node_ref = resize_ref,
                    class = "gds-files-header-resize",
                    on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                        if ev.button != Some(PointerButton::Primary) || !ev.primary {
                            return;
                        }
                        let w = resize_drag.column_width_untracked(column);
                        drag_state.set(Some((ev.position.x.0, w)));
                        ev.capture_pointer();
                        ev.stop_propagation();
                    },
                    on:pointer_move = {
                        let b = self.clone();
                        move |ev: &mut EventCx<'_, events::PointerMove>| {
                            let Some((start_x, base_w)) = drag_state.get_untracked() else {
                                return;
                            };
                            let delta = ev.position.x.0 - start_x;
                            b.0.column_widths.update(|widths| {
                                widths.insert(
                                    column,
                                    (base_w + delta).clamp(Self::column_min_width(column), 900.0),
                                );
                            });
                        }
                    },
                    on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                        if drag_state.get_untracked().is_some() {
                            drag_state.set(None);
                            ev.release_pointer();
                            let widths = resize_end
                                .0
                                .column_widths
                                .get_untracked()
                                .into_iter()
                                .map(|(col, width)| (col.id().to_owned(), width))
                                .collect();
                            resize_end.update(|state| {
                                state
                                    .places
                                    .set_widths(state.tab().navigation.location().clone(), widths);
                            });
                            resize_end.save_places();
                        }
                    },
                    on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                        if let Some((_, original)) = drag_state.get_untracked() {
                            drag_state.set(None);
                            ev.release_pointer();
                            resize_cancel.0.column_widths.update(|widths| {
                                widths.insert(column, original);
                            });
                        }
                    },
                    on:pointer_enter = {
                        let win = window.clone();
                        move |_| {
                            if let Some(ref w) = win {
                                w.set_cursor(CursorStyle::ResizeEastWest);
                            }
                        }
                    },
                    on:pointer_leave = {
                        let win = window;
                        move |_| {
                            if drag_state.get_untracked().is_none()
                                && let Some(ref w) = win
                            {
                                w.set_cursor(CursorStyle::Default);
                            }
                        }
                    }
                )
            }
        })
    }

    fn attach_list_scroll_observer(&self) {
        let list_node = self.0.list;
        let scroll_obs = list_node.observe_scroll();
        let browser = self.clone();
        let scroll_fx = RenderEffect::new(move |_| {
            let pos = scroll_obs.get();
            if pos.scrollport.width.0 > 0.0 || pos.scrollport.height.0 > 0.0 {
                let s = list_node.scale().max(1.0);
                browser.viewport_changed(Point::new(
                    CssPx(pos.offset.x.0 / s),
                    CssPx(pos.offset.y.0 / s),
                ));
            }
        });
        on_cleanup_local(move || drop(scroll_fx));
    }

    fn clear_blank_selection(&self) {
        self.reset_typeahead();
        self.update_selection(|state| {
            let view = state.tab_mut().navigation.view_mut();
            view.selection.clear();
            view.anchor = None;
        });
    }

    fn list_view(&self) -> AnyView {
        let browser = self.clone();
        AnyView::new(Scoped::new(move || browser.list_view_scoped()))
    }

    fn list_view_scoped(&self) -> AnyView {
        self.attach_list_scroll_observer();

        let list_node = self.0.list;
        let viewport = NodeRef::new();
        let count_browser = self.clone();
        let rows_browser = self.clone();
        let header_browser = self.clone();
        let table_browser = self.clone();
        let overflow_browser = self.clone();

        let row_h_browser = self.clone();
        let hdr_h_browser = self.clone();
        let row_height_sig = Signal::derive_local(move || row_h_browser.list_row_height());
        let row_count = Signal::derive_local(move || {
            count_browser.read_structure(|state| state.tab().rows.len())
        });
        let header_h_sig = Signal::derive_local(move || {
            if row_count.get() == 0 {
                0.0
            } else {
                hdr_h_browser.list_header_height()
            }
        });
        let empty = local_memo({
            let browser = self.clone();
            move || browser.collection_empty_label()
        });
        let header_structure = local_memo({
            let browser = self.clone();
            move || {
                let lang = Gds::try_current().map(|gds| gds.i18n().language());
                let is_collection = browser.read_structure(|state| {
                    matches!(
                        state.tab().contents,
                        Contents::Places(_)
                            | Contents::Recent(_)
                            | Contents::Trash(_)
                            | Contents::Tags { .. }
                            | Contents::Remote { .. }
                            | Contents::Volumes
                    )
                });
                if is_collection && let Some(columns) = browser.collection_column_ids() {
                    let sortable = browser.sortable_columns_untracked();
                    let sort = browser.read_structure(|state| state.tab().sort);
                    HeaderStructure::Collection {
                        columns,
                        sortable,
                        sort,
                        lang,
                    }
                } else {
                    let (columns, sort) = browser
                        .read_structure(|state| (state.tab().columns.clone(), state.tab().sort));
                    let sortable = browser.sortable_columns_untracked();
                    HeaderStructure::Directory {
                        columns,
                        sortable,
                        sort,
                        lang,
                    }
                }
            }
        });

        let has_marquee =
            self.read_untracked(|state| state.picker.is_none() && state.allows_multiple());
        let blank_press = (!has_marquee).then(|| {
            let clear_browser = self.clone();
            UnsyncCallback::new(move |()| clear_browser.clear_blank_selection())
        });

        let focus_visible = self.0.list_focus_visible;
        let table_col_attrs = self.table_column_attrs();
        let target = AnyView::new(view! {
            ListSurface(
                scroll_ref = list_node,
                rows = row_count,
                row_height = row_height_sig,
                header_height = header_h_sig,
                inset_x = TABLE_INSET,
                on_blank_press = blank_press,
                class = "gds-files-list-surface"
            ) {
                PageTableArea(class = "gds-files-table-area", {..table_col_attrs}) {
                    scroll(
                        node_ref = viewport,
                        class = "gds-files-table-x",
                        attr:data-focus-visible = move || {
                            focus_visible.get().then(|| "true".to_owned())
                        },
                        attr:data-scroll-x = move || {
                            (overflow_browser.content_columns_width()
                                > overflow_browser.table_viewport_width() + 0.5)
                                .then(|| "true".to_owned())
                        }
                    ) {
                        column(
                            class = "gds-files-table-inner",
                            style:min-width = move || Some(format!("{:.2}px", table_browser.table_width()))
                        ) {
                            {{move || {
                                let structure = header_structure.get();
                                let _z = enter_non_reactive_zone();
                                match structure {
                                    HeaderStructure::Collection { .. } => {
                                        header_browser
                                            .collection_header()
                                            .unwrap_or_else(|| AnyView::new(view! { box() }))
                                    }
                                    HeaderStructure::Directory { columns, .. } => {
                                        let cells = columns
                                            .columns()
                                            .iter()
                                            .map(|column| header_browser.column_header(*column))
                                            .collect::<Vec<_>>();
                                        AnyView::new(view! {
                                            row(class = "gds-files-header-row") {{cells}}
                                        })
                                    }
                                }
                            }}}
                            box(class = "gds-files-list-host") {
                                VirtualList(
                                    node_ref = Some(list_node),
                                    count = row_count,
                                    row_size = row_height_sig,
                                    label = i18n::text("app-title"),
                                    class = "gds-files-virtual-list",
                                    row = move |index: usize| rows_browser.visible_row(index, None)
                                ) {}
                                {{move || empty.get().map(|label| AnyView::new(view! {
                                    box(class = "gds-files-empty-overlay") {
                                        text {{i18n::text(label)}}
                                    }
                                }))}}
                            }
                        }
                    }
                }
            }
        });

        let dropped = self.background_drop_target(target);
        let surface = if has_marquee {
            self.marquee_surface(dropped, marquee::Layout::List { viewport })
        } else {
            dropped
        };
        self.directory_menu(surface)
    }

    fn grid_view(&self) -> AnyView {
        let browser = self.clone();
        AnyView::new(Scoped::new(move || browser.grid_view_scoped()))
    }

    fn grid_view_scoped(&self) -> AnyView {
        self.attach_list_scroll_observer();

        let list_node = self.0.list;
        let motion = GridMotion::new(self);
        let motion_rows = motion.clone();
        let count_browser = self.clone();
        let rows_browser = self.clone();

        let grid_row_count = Signal::derive_local(move || {
            let total = count_browser.read_structure(|state| state.tab().rows.len());
            let columns = motion.target.get().columns.max(1);
            total.div_ceil(columns)
        });
        let row_height = Signal::derive_local(move || motion.target.get().row_height.max(1.0));
        let seen = Virtualize::new(list_node, grid_row_count, row_height, 4);
        let window = Signal::derive_local(move || seen.window());
        let has_marquee =
            self.read_untracked(|state| state.picker.is_none() && state.allows_multiple());
        let blank_browser = self.clone();

        let focus_visible = self.0.list_focus_visible;
        let target = AnyView::new(view! {
            ListSurface(
                scroll_ref = list_node,
                class = "gds-files-list-surface"
            ) {
                PageTableArea(class = "gds-files-table-area") {
                    scroll(
                        node_ref = list_node,
                        class = "gds-files-grid-scroll",
                        attr:data-focus-visible = move || {
                            focus_visible.get().then(|| "true".to_owned())
                        },
                        on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                            if !has_marquee
                                && ev.button == Some(PointerButton::Primary)
                                && !ev.modifiers.shift()
                                && !ev.modifiers.primary()
                                && !super::drag::take_row_pointer_down(&blank_browser)
                            {
                                blank_browser.clear_blank_selection();
                            }
                        }
                    ) {
                        column(
                            class = "gds-files-grid-pane",
                            style:padding-left = format!("{GRID_PADDING}px"),
                            style:padding-right = format!("{GRID_PADDING}px"),
                            style:padding-top = move || {
                                Some(format!("{:.1}px", window.try_get().map_or(0.0, |w| w.lead)))
                            },
                            style:padding-bottom = move || {
                                Some(format!("{:.1}px", window.try_get().map_or(0.0, |w| w.trail)))
                            }
                        ) {
                            for grid_row in move || window.try_get().map(|w| w.indices()).unwrap_or_default(),
                                key = |grid_row: &usize| *grid_row
                            {
                                {{
                                    let browser = rows_browser.clone();
                                    let row_motion = motion_rows.clone();
                                    move || {
                                        let total = browser.read_structure(|state| state.tab().rows.len());
                                        let grid = row_motion.target.get();
                                        let columns = grid.columns.max(1);
                                        let start = grid_row * columns;
                                        let end = ((grid_row + 1) * columns).min(total);
                                        let tiles = (start..end)
                                            .map(|index| browser.visible_row(index, Some(row_motion.clone())))
                                            .collect::<Vec<_>>();
                                        view! {
                                            row(
                                                class = "gds-files-grid-row",
                                                style:height = format!("{:.1}px", grid.row_height),
                                                style:gap = format!("{GRID_GAP}px")
                                            ) {{tiles}}
                                        }
                                    }
                                }}
                            }
                        }
                    }
                }
            }
        });

        let dropped = self.background_drop_target(target);
        let surface = if has_marquee {
            self.marquee_surface(dropped, marquee::Layout::Grid)
        } else {
            dropped
        };
        self.directory_menu(surface)
    }

    fn visible_row(&self, index: usize, motion: Option<GridMotion>) -> AnyView {
        let is_grid = motion.is_some();
        let browser = self.clone();
        let row_key = local_memo(move || {
            let revision = browser.0.revision.get();
            let state = browser.0.state.borrow();
            let tab = state.tab();
            let entry = tab.rows.get(index).map(Row::key);
            (tab.id, entry, tab.columns.clone(), revision)
        });
        let render_browser = self.clone();
        AnyView::new(view! {
            box(
                class = "gds-files-row-wrap",
                attr:data-grid = is_grid.then(|| "true".to_owned())
            ) {
                {{move || {
                    row_key.with(|_| ());
                    let _z = enter_non_reactive_zone();
                    render_browser.row_view(index, motion.clone())
                }}}
            }
        })
    }

    fn row_view(&self, index: usize, motion: Option<GridMotion>) -> AnyView {
        let (row, columns, grid, cut) = self.read_structure(|state| {
            let tab = state.tab();
            let Some(row) = tab.rows.get(index).cloned() else {
                return (None, tab.columns.clone(), tab.mode == ViewMode::Grid, None);
            };
            let cut = row.path().map(|path| path.to_owned());
            (
                Some(row),
                tab.columns.clone(),
                tab.mode == ViewMode::Grid,
                cut,
            )
        });
        let Some(row) = row else {
            return AnyView::new(view! { box() });
        };
        let hidden = match &row {
            Row::File(entry) => entry.is_hidden,
            Row::Remote(entry) => entry.name.starts_with('.'),
            Row::Place(location) => self
                .place_entry(location)
                .is_some_and(|entry| entry.is_hidden),
            _ => false,
        };

        let key = row.key();
        let selected_sig = Signal::derive_local({
            let browser = self.clone();
            let key = key.clone();
            move || {
                let _ = browser.0.selection.get();
                let _ = browser.0.revision.get();
                browser
                    .0
                    .state
                    .borrow()
                    .tab()
                    .navigation
                    .view()
                    .selection
                    .contains(&key)
            }
        });
        let cursor_sig = Signal::derive_local({
            let browser = self.clone();
            let key = key.clone();
            move || {
                if !browser.0.list_focus_visible.get() {
                    return false;
                }
                let _ = browser.0.selection.get();
                let _ = browser.0.revision.get();
                browser
                    .0
                    .state
                    .borrow()
                    .tab()
                    .navigation
                    .view()
                    .cursor
                    .as_ref()
                    == Some(&key)
            }
        });
        let selection_goal =
            Signal::derive_local(move || if selected_sig.get() { 1.0_f32 } else { 0.0_f32 });
        let selection_progress = use_transition(selection_goal, WORKSPACE_DURATION_MS);

        let label = row.label();
        let a11y_path = row.path().map(|path| path.display().to_string());
        let desc_attrs = a11y_path.as_ref().map_or_else(Attrs::new, |path| {
            Attrs::new()
                .a11y_from(A11yBinding::unspecified().description(SharedString::from(path.clone())))
        });

        let cut_for_badge = cut.clone();
        let is_cut = Signal::derive_local(move || {
            let _ = Clipboard::current().snapshot();
            cut_for_badge
                .as_ref()
                .is_some_and(|path| Clipboard::current().is_cut(path))
        });
        let drag_browser = self.clone();
        let ghosted = Signal::derive_local(move || {
            let _ = Clipboard::current().snapshot();
            cut.as_ref().is_some_and(|path| {
                Clipboard::current().is_cut(path)
                    || drag_browser.is_drag_source(path)
                    || super::drag::FileDrag::is_source(path)
            })
        });

        let select_row = self.clone();
        let open_row = self.clone();
        let context_row = self.clone();

        if grid {
            let geometry = self.grid_geometry_untracked();
            let artwork = self.tile_artwork(&row, geometry.icon_size, selection_progress);
            let a11y_label = SharedString::from(label.clone());
            let tile_tooltip = wrap_tokens(&label);
            let tile_pres = motion
                .as_ref()
                .map(|grid_motion| grid_motion.present_tile(key, index));
            let tile_width =
                tile_pres.map_or_else(|| Signal::stored_local(geometry.width), |p| p.width);
            let tile_height =
                tile_pres.map_or_else(|| Signal::stored_local(geometry.tile_height), |p| p.height);
            let tile_offset = tile_pres.map(|p| p.offset);
            let line_height = geometry.line_height;
            let artwork_height = geometry.artwork_height;
            let name_height = geometry.name_height;

            let tile = AnyView::new(view! {
                column(
                    class = "gds-files-tile",
                    style:width = move || Some(format!("{:.1}px", tile_width.get())),
                    style:height = move || Some(format!("{:.1}px", tile_height.get())),
                    style:left = move || {
                        tile_offset.and_then(|off| {
                            let dx = off.get().0;
                            (dx.abs() > 0.05).then(|| format!("{dx:.1}px"))
                        })
                    },
                    style:top = move || {
                        tile_offset.and_then(|off| {
                            let dy = off.get().1;
                            (dy.abs() > 0.05).then(|| format!("{dy:.1}px"))
                        })
                    },
                    style:padding = format!("{GRID_PADDING}px"),
                    style:gap = format!("{GRID_GAP}px"),
                    attr:data-selected = move || selected_sig.get().then(|| "true".to_owned()),
                    attr:data-cursor = move || cursor_sig.get().then(|| "true".to_owned()),
                    attr:data-ghosted = move || ghosted.get().then(|| "true".to_owned()),
                    attr:data-hidden-entry = hidden.then(|| "true".to_owned()),
                    a11y:role = Role::ListItem,
                    a11y:label = a11y_label,
                    a11y:selected = move || selected_sig.get(),
                    {..desc_attrs},
                    on:click = move |ev: &mut EventCx<'_, events::Click>| {
                        select_row.select_row(index, ev.modifiers);
                        select_row.focus_content();
                    },
                    on:double_click = move |_: &mut EventCx<'_, events::DoubleClick>| {
                        open_row.open_row(index, false);
                    },
                    on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                        if ev.button == Some(PointerButton::Secondary) {
                            context_row.select_context(index);
                        }
                    }
                ) {
                    Show(when = move || is_cut.get()) {
                        box(class = "gds-files-tile-cut", a11y:hidden = true) {
                            Icon(name = IconName::Scissors, size = 12.0)
                        }
                    }
                    box(
                        class = "gds-files-tile-artwork",
                        style:height = format!("{artwork_height:.1}px")
                    ) {
                        {artwork}
                    }
                    box(
                        class = "gds-files-tile-name-wrap",
                        style:height = format!("{name_height:.1}px")
                    ) {
                        text(
                            class = "gds-files-tile-name",
                            style:line-height = format!("{line_height:.1}px"),
                            style:max-height = format!("{:.1}px", line_height * 2.0)
                        ) {{label}}
                    }
                }
            });

            let tipped = AnyView::new(view! {
                HintTooltip(text = tile_tooltip) { {tile} }
            });
            let dragged = self.file_row_drag(tipped, &row, Some(geometry));
            return self.row_menu(dragged, index);
        }

        // Table row (`ViewMode::List` and collection tables)
        let selected_binding = Binding::controlled(selected_sig, |_| {});
        let cells: Vec<AnyView> =
            if let Some(collection_cells) = self.collection_cells(&row, selected_binding) {
                collection_cells
            } else {
                let label_tooltip = wrap_tokens(&label);
                let a11y_value = SharedString::from(label.clone());
                let select_label = self.clone();
                let open_label = self.clone();
                let label_view = AnyView::new(view! {
                    row(class = "gds-files-row-name-group") {
                        text(
                            class = "gds-files-row-name-text",
                            a11y:role = Role::Label,
                            a11y:value = a11y_value,
                            a11y:selected = move || selected_sig.get(),
                            {..desc_attrs},
                            on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                ev.stop_propagation();
                                select_label.select_row(index, ev.modifiers);
                                select_label.focus_content();
                            },
                            on:double_click = move |ev: &mut EventCx<'_, events::DoubleClick>| {
                                ev.stop_propagation();
                                open_label.open_row(index, false);
                            }
                        ) {{label}}
                        Show(when = move || is_cut.get()) {
                            Icon(
                                name = IconName::Scissors,
                                size = 12.0,
                                class = "gds-files-cut-icon"
                            )
                        }
                    }
                });

                let row_artwork = self.row_artwork_with_selection(&row, 18.0, selection_progress);
                let name_cell_ref = NodeRef::new();
                #[cfg(test)]
                tests::record_row_cell_node(index, Column::Name, name_cell_ref);
                let mut name = Some(AnyView::new(view! {
                    HintTooltip(
                        text = label_tooltip,
                        trigger_class = Self::column_width_class(Column::Name),
                    ) {
                        row(
                            node_ref = name_cell_ref,
                            class = "gds-files-name-cell gds-files-col-name"
                        ) {
                            {row_artwork}
                            {label_view}
                        }
                    }
                }));
                columns
                    .columns()
                    .iter()
                    .map(|column| {
                        if *column == Column::Name {
                            name.take().unwrap_or_else(|| AnyView::new(view! { box() }))
                        } else {
                            self.cell(index, &row, *column)
                        }
                    })
                    .collect()
            };

        let target = AnyView::new(view! {
            row(
                class = "gds-files-row-content",
                attr:data-selected = move || selected_sig.get().then(|| "true".to_owned()),
                attr:data-cursor = move || cursor_sig.get().then(|| "true".to_owned()),
                attr:data-ghosted = move || ghosted.get().then(|| "true".to_owned()),
                attr:data-hidden-entry = hidden.then(|| "true".to_owned()),
                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                    select_row.select_row(index, ev.modifiers);
                    select_row.focus_content();
                },
                on:double_click = move |_: &mut EventCx<'_, events::DoubleClick>| {
                    open_row.open_row(index, false);
                },
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    if ev.button == Some(PointerButton::Secondary) {
                        context_row.select_context(index);
                    }
                }
            ) {{cells}}
        });

        let dragged = self.file_row_drag(target, &row, None);
        self.row_menu(dragged, index)
    }

    fn cell(&self, index: usize, row: &Row, column: Column) -> AnyView {
        #[cfg(not(test))]
        let _ = index;
        let current_owner = matches!(
            (row, column),
            (Row::File(entry), Column::Owner) if EntryPresentation::owner(entry).current
        );
        let align_end = column == Column::Size;

        let text_cell = |value: String| {
            let hint = wrap_tokens(&value);
            AnyView::new(view! {
                HintTooltip(text = hint) {
                    text(class = "gds-files-meta-text") {{value}}
                }
            })
        };

        let content = match (row, column) {
            (Row::File(entry), Column::Modified) => AnyView::new(view! {
                TimestampView(value = Signal::stored_local(entry.modified))
            }),
            (Row::File(entry), Column::Accessed) => AnyView::new(view! {
                TimestampView(value = Signal::stored_local(entry.details.accessed))
            }),
            (Row::Trash(entry), Column::Modified) => AnyView::new(view! {
                TimestampView(value = Signal::stored_local(entry.deleted_at))
            }),
            (Row::File(entry), Column::Name) => text_cell(entry.display_name.clone()),
            (Row::File(entry), Column::Type) => text_cell(EntryPresentation::type_label(entry)),
            (Row::File(entry), Column::DetailedType) => {
                text_cell(EntryPresentation::detailed_type(entry))
            }
            (Row::File(entry), Column::Size) => text_cell(EntryPresentation::size_label(entry)),
            (Row::File(entry), Column::Owner) => text_cell(EntryPresentation::owner(entry).text),
            (Row::File(entry), Column::Group) => text_cell(EntryPresentation::group(entry)),
            (Row::File(entry), Column::Permissions) => {
                entry.details.permissions.as_ref().map_or_else(
                    || text_cell("—".to_owned()),
                    |permissions| {
                        let hint = permissions.octal();
                        let sym = permissions.symbols();
                        AnyView::new(view! {
                            HintTooltip(text = hint) {
                                text(class = "gds-files-meta-text") {{sym}}
                            }
                        })
                    },
                )
            }
            _ => text_cell(String::new()),
        };

        let cell_ref = NodeRef::new();
        #[cfg(test)]
        tests::record_row_cell_node(index, column, cell_ref);

        AnyView::new(view! {
            box(
                node_ref = cell_ref,
                class = format!("gds-files-meta-cell {}", Self::column_width_class(column)),
                attr:data-current-owner = current_owner.then(|| "true".to_owned()),
                attr:data-align-end = align_end.then(|| "true".to_owned())
            ) {{content}}
        })
    }

    fn page_actions(&self) -> AnyView {
        if self.actions_collapsed() {
            return self.page_menu();
        }
        if let Some(actions) = self.special_page_actions() {
            return actions;
        }
        let browser = self.clone();
        let layout = local_memo({
            let browser = self.clone();
            move || {
                let grid = browser.read_structure(|state| state.tab().mode == ViewMode::Grid);
                let details_avail =
                    browser.details_permitted() && browser.details_available_structure();
                (grid, details_avail)
            }
        });
        AnyView::new(view! {
            {{move || {
                let (grid, details_avail) = layout.get();
                let _z = enter_non_reactive_zone();
                let fav_b = browser.clone();
                let favorite = Signal::derive_local(move || {
                    fav_b.read_structure(|state| {
                        state.places.is_favorite(state.tab().navigation.location())
                    })
                });
                let prefs = browser.0.preferences.state();
                let hidden = Signal::derive_local(move || prefs.with(|p| p.files.show_hidden));
                let details: Signal<bool, LocalStorage> = browser.0.details.into();
                let details_button = details_avail.then(|| {
                    browser.control(
                        IconName::Info,
                        "details-toggle",
                        Browser::toggle_details,
                        Some(Command::Details),
                        Some(details),
                    )
                });

                view! {
                    row(class = "gds-files-page-action-strip") {
                        {{browser.control(
                            if grid { IconName::List } else { IconName::LayoutGrid },
                            "view-toggle",
                            Browser::toggle_view,
                            Some(Command::ViewMode),
                            None,
                        )}}
                        {{browser.control(
                            IconName::Star,
                            "favorite-toggle",
                            Browser::favorite,
                            Some(Command::Favorite),
                            Some(favorite),
                        )}}
                        {{browser.column_menu()}}
                        {{browser.control(
                            IconName::Eye,
                            "show-hidden",
                            |b| b.edit_preferences(|preferences| {
                                preferences.files.show_hidden = !preferences.files.show_hidden;
                            }),
                            Some(Command::Hidden),
                            Some(hidden),
                        )}}
                        {details_button}
                        {{browser.control(
                            IconName::RefreshCw,
                            "refresh",
                            Browser::refresh,
                            Some(Command::Refresh),
                            None,
                        )}}
                    }
                }
            }}}
        })
    }

    fn status(&self) -> String {
        let visible_volumes = self
            .0
            .volumes
            .snapshot()
            .volumes
            .iter()
            .filter(|volume| volume.visible)
            .count();
        let filter = self.0.filter.get();
        self.read(|state| {
            let tab = state.tab();
            if tab.navigation.is_loading() || matches!(tab.contents, Contents::LoadingCollection) {
                return i18n::text("loading");
            }
            if matches!(tab.navigation.location(), Location::Volumes)
                || matches!(tab.contents, Contents::Volumes)
            {
                let count = if matches!(tab.contents, Contents::Volumes) && !filter.is_empty() {
                    tab.rows.len()
                } else {
                    visible_volumes
                };
                return i18n::count("volume-count", count);
            }
            if !filter.is_empty() {
                let total = match &tab.contents {
                    Contents::Directory => tab.navigation.directory().map(|dir| dir.entries.len()),
                    Contents::Places(items) => Some(items.len()),
                    Contents::Recent(entries) => Some(entries.len()),
                    Contents::Remote { listing, .. } => Some(listing.entries.len()),
                    Contents::Tags { entries, .. } if !tab.is_pure_tag_catalogue() => {
                        Some(entries.len())
                    }
                    _ => None,
                };
                if let Some(total) = total {
                    return gds::t!(
                        "filter-count",
                        visible = tab.rows.len() as i64,
                        total = total as i64
                    );
                }
            }
            i18n::count("item-count", tab.rows.len())
        })
    }

    fn prompt_dialogs(&self) -> AnyView {
        let browser = self.clone();
        AnyView::new(Scoped::new(move || {
            let prompt_sig = browser.0.prompt;
            let gen_sig = browser.0.prompt_generation;

            // 1. `Prompt::About` -> `AboutDialog` (`DialogSize::Sm = 420px`)
            let about_open =
                Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::About));
            let about_cancel = browser.clone();

            // 2. `Prompt::Confirm` -> `ConfirmDialog` (`DialogSize::Sm = 420px`)
            let confirm_open =
                Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::Confirm { .. }));
            let confirm_title = RwSignal::new_local(String::new());
            let confirm_message = RwSignal::new_local(String::new());
            let confirm_primary = RwSignal::new_local(String::new());
            let confirm_cancel_lbl = Signal::derive_local(|| i18n::text("operation-cancel"));
            let confirm_icon = RwSignal::new_local(Option::<IconName>::None);
            let confirm_primary_icon = RwSignal::new_local(IconName::Check);
            let confirm_items = RwSignal::new_local(Vec::<String>::new());
            let confirm_overflow = RwSignal::new_local(Option::<String>::None);
            let confirm_destructive = RwSignal::new_local(false);
            let confirm_cb_slot: Rc<RefCell<Option<UnsyncCallback<DialogDecision>>>> =
                Rc::new(RefCell::new(None));
            {
                let cb_browser = browser.clone();
                let slot = Rc::clone(&confirm_cb_slot);
                let sync_confirm_fx = RenderEffect::new(move |_| {
                    let generation = gen_sig.get();
                    let _z = enter_non_reactive_zone();
                    match cb_browser.0.prompt.get_untracked() {
                        Prompt::Confirm {
                            title,
                            message,
                            primary,
                            accept,
                        } => {
                            let options = cb_browser.0.confirm_options.get_untracked();
                            let shown_items: Vec<String> = options
                                .items
                                .iter()
                                .take(CONFIRM_PREVIEW_LIMIT)
                                .cloned()
                                .collect();
                            let extra_from_slice =
                                options.items.len().saturating_sub(CONFIRM_PREVIEW_LIMIT);
                            let remaining = options.overflow_count.saturating_add(extra_from_slice);
                            let overflow_line = (remaining > 0)
                                .then(|| i18n::count("operation-more-items", remaining));

                            confirm_title.set(title);
                            confirm_message.set(message);
                            confirm_primary.set(primary);
                            confirm_icon.set(options.icon);
                            confirm_primary_icon
                                .set(options.primary_icon.unwrap_or(IconName::Check));
                            confirm_items.set(shown_items);
                            confirm_overflow.set(overflow_line);
                            confirm_destructive.set(options.destructive);

                            let weak = cb_browser.downgrade();
                            let settled = Rc::new(Cell::new(false));
                            let decision_cb =
                                UnsyncCallback::new(move |decision: DialogDecision| {
                                    if settled.get() {
                                        return;
                                    }
                                    let Some(b) = weak.upgrade() else {
                                        return;
                                    };
                                    if b.0.prompt_generation.get_untracked() != generation
                                        || !matches!(
                                            b.0.prompt.get_untracked(),
                                            Prompt::Confirm { .. }
                                        )
                                    {
                                        return;
                                    }
                                    settled.set(true);
                                    match decision {
                                        DialogDecision::Confirm => {
                                            b.0.prompt_decisions.clear();
                                            b.0.prompt.set(Prompt::Closed);
                                            accept();
                                        }
                                        DialogDecision::Cancel | DialogDecision::Dismiss => {
                                            b.cancel_prompt();
                                        }
                                    }
                                });
                            *slot.borrow_mut() = Some(decision_cb);
                            cb_browser
                                .0
                                .prompt_decisions
                                .register(false, None, decision_cb);
                        }
                        Prompt::Closed => {
                            cb_browser.0.prompt_decisions.clear();
                        }
                        _ => {}
                    }
                });
                on_cleanup_local(move || drop(sync_confirm_fx));
            }
            let cb_slot_run = confirm_cb_slot;

            // 3. `Prompt::Name` -> `ModalDialog` (`DialogSize::Sm`)
            let name_open =
                Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::Name { .. }));
            let name_open_cancel = browser.clone();
            let name_decision_cancel = browser.clone();
            let name_decisions = browser.0.prompt_decisions.clone();
            let name_body_browser = browser.clone();
            let retained_name: Rc<RefCell<Option<(NameForm, Rc<dyn Fn(OsString)>, u64)>>> =
                Rc::new(RefCell::new(None));

            // 4. `Prompt::Conflict` -> `ModalDialog` (`DialogSize::Md`)
            let conflict_open =
                Signal::derive_local(move || matches!(prompt_sig.get(), Prompt::Conflict { .. }));
            let conflict_open_cancel = browser.clone();
            let conflict_decision_cancel = browser.clone();
            let conflict_decisions = browser.0.prompt_decisions.clone();
            let conflict_body_browser = browser;
            type RetainedConflict = (
                crate::features::file_operations::transfer::Mode,
                crate::features::file_operations::paste::PasteConflict,
                Rc<dyn Fn(crate::features::file_operations::paste::ConflictResolution)>,
                u64,
            );
            let retained_conflict: Rc<RefCell<Option<RetainedConflict>>> =
                Rc::new(RefCell::new(None));

            view! {
                AboutDialog(
                    open = Binding::controlled(
                        about_open,
                        move |next: bool| {
                            if !next && matches!(about_cancel.0.prompt.get_untracked(), Prompt::About) {
                                about_cancel.cancel_prompt();
                            }
                        },
                    ),
                    application = Application::cargo("gds-files"),
                )
                ConfirmDialog(
                    open = Binding::controlled(confirm_open, |_| {}),
                    size = DialogSize::Sm,
                    title = confirm_title,
                    description = confirm_message,
                    icon = confirm_icon,
                    primary_icon = confirm_primary_icon,
                    items = confirm_items,
                    overflow_text = confirm_overflow,
                    confirm_label = confirm_primary,
                    cancel_label = confirm_cancel_lbl,
                    destructive = confirm_destructive,
                    generation = gen_sig,
                    on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                        let cb = *cb_slot_run.borrow();
                        if let Some(cb) = cb {
                            cb.run(decision);
                        }
                    }),
                )
                ModalDialog(
                    open = Binding::controlled(
                        name_open,
                        move |next: bool| {
                            if !next
                                && matches!(
                                    name_open_cancel.0.prompt.get_untracked(),
                                    Prompt::Name { .. }
                                )
                            {
                                name_open_cancel.cancel_prompt();
                            }
                        },
                    ),
                    size = DialogSize::Sm,
                    close_on_confirm = false,
                    on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                        match decision {
                            DialogDecision::Confirm => {
                                let _ = name_decisions.confirm();
                            }
                            DialogDecision::Cancel | DialogDecision::Dismiss => {
                                if matches!(
                                    name_decision_cancel.0.prompt.get_untracked(),
                                    Prompt::Name { .. }
                                ) {
                                    name_decision_cancel.cancel_prompt();
                                }
                            }
                        }
                    }),
                ) {
                    {{
                        let pb = name_body_browser.clone();
                        let retained = Rc::clone(&retained_name);
                        move || {
                            let generation = gen_sig.get();
                            let _z = enter_non_reactive_zone();
                            if let Prompt::Name { form, submit } = pb.0.prompt.get_untracked() {
                                *retained.borrow_mut() =
                                    Some((form.clone(), Rc::clone(&submit), generation));
                                form.view(&pb, generation, submit, &pb.0.prompt_decisions)
                            } else if let Some((form, submit, retained_gen)) = retained.borrow().clone() {
                                form.view(&pb, retained_gen, submit, &pb.0.prompt_decisions)
                            } else {
                                AnyView::new(())
                            }
                        }
                    }}
                }
                ModalDialog(
                    open = Binding::controlled(
                        conflict_open,
                        move |next: bool| {
                            if !next
                                && matches!(
                                    conflict_open_cancel.0.prompt.get_untracked(),
                                    Prompt::Conflict { .. }
                                )
                            {
                                conflict_open_cancel.cancel_prompt();
                            }
                        },
                    ),
                    size = DialogSize::Md,
                    class = "gds-files-conflict-modal",
                    dismiss_on_outside_press = false,
                    dismiss_on_escape = false,
                    close_on_confirm = false,
                    on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                        match decision {
                            DialogDecision::Confirm => {
                                let _ = conflict_decisions.confirm();
                            }
                            DialogDecision::Cancel | DialogDecision::Dismiss => {
                                if matches!(
                                    conflict_decision_cancel.0.prompt.get_untracked(),
                                    Prompt::Conflict { .. }
                                ) {
                                    conflict_decision_cancel.cancel_prompt();
                                }
                            }
                        }
                    }),
                ) {
                    {{
                        let pb = conflict_body_browser.clone();
                        let retained = Rc::clone(&retained_conflict);
                        move || {
                            let generation = gen_sig.get();
                            let _z = enter_non_reactive_zone();
                            if let Prompt::Conflict {
                                mode,
                                conflict,
                                resolve,
                                ..
                            } = pb.0.prompt.get_untracked()
                            {
                                *retained.borrow_mut() = Some((
                                    mode,
                                    conflict.clone(),
                                    Rc::clone(&resolve),
                                    generation,
                                ));
                                super::conflict_dialog::render_conflict_dialog(
                                    &pb,
                                    generation,
                                    mode,
                                    conflict,
                                    resolve,
                                    &pb.0.prompt_decisions,
                                )
                            } else if let Some((mode, conflict, resolve, retained_gen)) =
                                retained.borrow().clone()
                            {
                                super::conflict_dialog::render_conflict_dialog(
                                    &pb,
                                    retained_gen,
                                    mode,
                                    conflict,
                                    resolve,
                                    &pb.0.prompt_decisions,
                                )
                            } else {
                                AnyView::new(())
                            }
                        }
                    }}
                }
            }
        }))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use gds::app::Gds;
    use gds::appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH};
    use gds::i18n::Language;
    use gds::service::task_manager::{Scope, Supervisor};
    use zgui::app::Fonts;
    use zgui::atlas::{MemorySink, TextureSink};
    use zgui::bits::DamageSet;
    use zgui::geom::{CssPx, DevicePx, Point, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::{GetUntracked, NodeRef};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::scene::Scene;
    use zgui::vocab::{
        KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction,
        PointerButton, PointerEvent, Role, Timestamp,
    };
    use zgui_platform_headless::Harness;

    use super::{Browser, Column, Location};
    use crate::entities::location::{places::Places, volumes};
    use crate::entities::preferences;
    use crate::pages::workspace::ui::LaunchOptions;
    use crate::shared::i18n as files_i18n;

    thread_local! {
        static HEADER_CELL_NODES: RefCell<HashMap<Column, NodeRef>> = RefCell::new(HashMap::new());
        static ROW_CELL_NODES: RefCell<HashMap<(usize, Column), NodeRef>> = RefCell::new(HashMap::new());
        static HEADER_RESIZE_NODES: RefCell<HashMap<Column, NodeRef>> = RefCell::new(HashMap::new());
        static FOOTER_TERMINAL_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
        static LAST_COLOR_SPRITES: RefCell<Vec<(f32, f32)>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn record_header_cell_node(column: Column, node: NodeRef) {
        HEADER_CELL_NODES.with(|slot| {
            slot.borrow_mut().insert(column, node);
        });
    }

    pub(super) fn record_row_cell_node(index: usize, column: Column, node: NodeRef) {
        ROW_CELL_NODES.with(|slot| {
            slot.borrow_mut().insert((index, column), node);
        });
    }

    pub(super) fn record_header_resize_node(column: Column, node: NodeRef) {
        HEADER_RESIZE_NODES.with(|slot| {
            slot.borrow_mut().insert(column, node);
        });
    }

    pub(super) fn record_footer_terminal_node(node: NodeRef) {
        FOOTER_TERMINAL_NODE.with(|slot| {
            *slot.borrow_mut() = Some(node);
        });
    }

    fn clear_view_probes() {
        HEADER_CELL_NODES.with(|slot| slot.borrow_mut().clear());
        ROW_CELL_NODES.with(|slot| slot.borrow_mut().clear());
        HEADER_RESIZE_NODES.with(|slot| slot.borrow_mut().clear());
        FOOTER_TERMINAL_NODE.with(|slot| *slot.borrow_mut() = None);
        LAST_COLOR_SPRITES.with(|slot| slot.borrow_mut().clear());
    }

    fn footer_terminal_node() -> Option<NodeRef> {
        FOOTER_TERMINAL_NODE.with(|slot| *slot.borrow())
    }

    fn assert_content_width_matches_layout(browser: &Browser) {
        let node = browser.0.content_focus;
        let bounds = node.bounds().expect("file content has completed layout");
        let measured = bounds.size.width.0;
        let expected = browser.0.content_width.get_untracked() * node.scale();
        assert!(
            (measured - expected).abs() <= 1.0,
            "file content width was {measured} device pixels, expected {expected}"
        );
    }

    fn last_color_sprites() -> Vec<(f32, f32)> {
        LAST_COLOR_SPRITES.with(|slot| slot.borrow().clone())
    }

    fn header_cell_node(column: Column) -> Option<NodeRef> {
        HEADER_CELL_NODES.with(|slot| slot.borrow().get(&column).copied())
    }

    fn row_cell_node(index: usize, column: Column) -> Option<NodeRef> {
        ROW_CELL_NODES.with(|slot| slot.borrow().get(&(index, column)).copied())
    }

    fn header_resize_node(column: Column) -> Option<NodeRef> {
        HEADER_RESIZE_NODES.with(|slot| slot.borrow().get(&column).copied())
    }

    fn assert_directory_header_and_row_cell_parity(browser: &Browser, row_indices: &[usize]) {
        let visible_columns =
            browser.read_untracked(|state| state.tab().columns.columns().to_vec());
        for column in visible_columns {
            let expected_w = browser.column_width_untracked(column);
            let header_node = header_cell_node(column)
                .unwrap_or_else(|| panic!("recorded header cell node for {column:?}"));
            let scale = header_node.scale().max(1.0);
            let header_bounds = header_node
                .window_bounds()
                .unwrap_or_else(|| panic!("measured header cell bounds for {column:?}"));
            let header_x = header_bounds.origin.x.0 / scale;
            let header_w = header_bounds.size.width.0 / scale;
            assert!(
                (header_bounds.size.width.0 - expected_w * scale).abs() <= 1.0,
                "expected {column:?} header cell width ~= {expected_w:.2}px, got {header_w:.2}px"
            );
            for &row_idx in row_indices {
                let cell_node = row_cell_node(row_idx, column)
                    .unwrap_or_else(|| panic!("recorded row {row_idx} cell node for {column:?}"));
                let cell_bounds = cell_node
                    .window_bounds()
                    .unwrap_or_else(|| panic!("measured row {row_idx} cell bounds for {column:?}"));
                let cell_x = cell_bounds.origin.x.0 / scale;
                let cell_w = cell_bounds.size.width.0 / scale;
                assert!(
                    (header_w - cell_w).abs() == 0.0 && (header_x - cell_x).abs() == 0.0,
                    "expected exact 0.0px header/row parity for {column:?} on row {row_idx}: header=({header_x:.2}, {header_w:.2}) vs row=({cell_x:.2}, {cell_w:.2})"
                );
            }
        }
    }

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

        fn draw(&mut self, scene: &Scene, _: &DamageSet) -> FrameOutcome {
            if !scene.primitives.color_sprites.is_empty() {
                LAST_COLOR_SPRITES.with(|slot| {
                    let mut out = slot.borrow_mut();
                    out.clear();
                    for sprite in &scene.primitives.color_sprites {
                        let [_, _, w, h] = sprite.bounds;
                        if w > 0.0 && h > 0.0 {
                            out.push((w, h));
                        }
                    }
                });
            }
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

    #[test]
    fn column_header_resize_drag_tracks_continuous_pointer_moves_without_losing_capture() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("Desktop")).expect("create Desktop");
        std::fs::write(home.join("notes.txt"), b"hello\n").expect("create notes.txt");
        clear_view_probes();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home;

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            let deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.is_empty()) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for Home directory rows to load"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            harness.settle(8);

            let initial = browser.column_width_untracked(Column::Name);
            let initial_name_cell_0 = row_cell_node(0, Column::Name)
                .and_then(|n| n.get_untracked())
                .expect("bound row 0 Column::Name cell NodeId");
            assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);
            let resize_node = header_resize_node(Column::Name)
                .expect("recorded .gds-files-header-resize for Column::Name");
            let initial_node_id = resize_node
                .get_untracked()
                .expect("bound resize handle NodeId");
            let scale = resize_node.scale().max(1.0);
            let bounds = resize_node
                .window_bounds()
                .expect("measured bounds for .gds-files-header-resize");
            let start_x = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
            let start_y = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;

            // 1. PointerDown on `.gds-files-header-resize`, then multiple PointerMove steps (+40, +110, +180)
            // settled across frames to prove DOM nodes & pointer capture survive reactive width updates!
            let down_pos = Point::new(CssPx(start_x), CssPx(start_y));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(down_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);

            for delta in [40.0_f32, 110.0, 180.0] {
                let move_pos = Point::new(CssPx(start_x + delta), CssPx(start_y + 12.0));
                harness.deliver_to_first(SurfaceEvent::Pointer {
                    action: PointerAction::Moved,
                    event: PointerEvent::mouse(move_pos),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
                harness.settle(6);

                let current = browser.column_width(Column::Name);
                assert!(
                    (current - (initial + delta)).abs() < 0.5,
                    "expected Column::Name width == {:.1} after +{delta}px drag, got {current:.1}",
                    initial + delta
                );
                let latest_resize = header_resize_node(Column::Name)
                    .and_then(|n| n.get_untracked())
                    .expect("resize handle still live");
                assert_eq!(
                    latest_resize, initial_node_id,
                    "header resize DOM node must not be destroyed/recreated during column drag"
                );
                assert_eq!(
                    row_cell_node(0, Column::Name).and_then(|n| n.get_untracked()),
                    Some(initial_name_cell_0),
                    "row cell DOM node must remain stable during column resize drag"
                );
                assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);
            }

            let up_pos = Point::new(CssPx(start_x + 180.0), CssPx(start_y + 12.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(up_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(6);
            assert!(
                (browser.column_width(Column::Name) - (initial + 180.0)).abs() < 0.5,
                "expected final Column::Name width == {:.1} on PointerUp",
                initial + 180.0
            );

            // Moving after PointerUp must NOT continue resizing.
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(Point::new(CssPx(start_x + 250.0), CssPx(start_y))),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert!(
                (browser.column_width(Column::Name) - (initial + 180.0)).abs() < 0.5,
                "pointer capture must release on PointerUp"
            );

            // Starting a second drag and firing PointerCancel must release capture and restore width.
            let resized_bounds = resize_node
                .window_bounds()
                .expect("updated bounds for .gds-files-header-resize");
            let second_x = (resized_bounds.origin.x.0 + resized_bounds.size.width.0 * 0.5) / scale;
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(Point::new(CssPx(second_x), CssPx(start_y)))
                    .with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(Point::new(CssPx(second_x + 50.0), CssPx(start_y))),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert!(
                (browser.column_width(Column::Name) - (initial + 230.0)).abs() < 0.5,
                "expected second drag to reach {:.1} before cancel",
                initial + 230.0
            );
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Cancelled,
                event: PointerEvent::mouse(Point::new(CssPx(second_x + 50.0), CssPx(start_y))),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert!(
                (browser.column_width(Column::Name) - (initial + 180.0)).abs() < 0.5,
                "PointerCancel must restore pre-drag width ({:.1})",
                initial + 180.0
            );
            assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope: Scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::Chinese).expect("Chinese catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("column header resize harness");
    }

    #[test]
    fn grid_tile_thumbnails_fill_full_tile_artwork_slot_and_details_media_spans_full_width_by_168px()
     {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("Pictures")).expect("create Pictures");
        std::fs::write(home.join("Pictures/nested.bin"), vec![0xabu8; 4096])
            .expect("create nested file inside Pictures");
        std::fs::write(home.join("notes.txt"), b"hello\n").expect("create notes.txt");
        let two_line_name = "eb72d0c7423da74ca3d41fb9e570ce33470697977.txt";
        let very_long_name =
            "release_artifact_2026_11_21_x86_64_unknown_linux_gnu_symbols_bundle.tar.zst";
        std::fs::write(home.join(two_line_name), b"two lines\n").expect("create 2-line fixture");
        std::fs::write(home.join(very_long_name), b"long\n").expect("create 3-line fixture");
        let png_path = home.join("screenshot.png");
        let img: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(320, 180, |x, y| {
                image::Rgba([(x & 0xff) as u8, (y & 0xff) as u8, 200, 255])
            });
        img.save(&png_path).expect("write 16:9 PNG fixture");
        let png_len = std::fs::metadata(&png_path).expect("png metadata").len();
        let expected_multi_size =
            crate::entities::file::ui::EntryPresentation::format_size(png_len + 5 + 4096);

        clear_view_probes();
        super::super::inspection::tests::clear_recorded_artwork_nodes();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home;

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            let offscreen = harness
                .platform()
                .offscreens()
                .first()
                .cloned()
                .expect("primary offscreen");
            offscreen.set_a11y_listener(true);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            let deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.len() < 5) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for Home directory rows to load"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }

            // Switch to Grid view and open the Details pane.
            browser.toggle_view();
            browser.toggle_details();
            zgui::reactive::flush();
            harness.settle(16);

            let (screenshot_idx, notes_idx, pictures_idx, two_line_idx, very_long_idx) = browser
                .read_untracked(|state| {
                    let tab = state.tab();
                    let mut s_idx = None;
                    let mut n_idx = None;
                    let mut p_idx = None;
                    let mut t_idx = None;
                    let mut l_idx = None;
                    for (idx, row) in tab.rows.iter().enumerate() {
                        match row.label().as_str() {
                            "screenshot.png" => s_idx = Some(idx),
                            "notes.txt" => n_idx = Some(idx),
                            "Pictures" => p_idx = Some(idx),
                            name if name == two_line_name => t_idx = Some(idx),
                            name if name == very_long_name => l_idx = Some(idx),
                            _ => {}
                        }
                    }
                    (
                        s_idx.expect("screenshot.png row index"),
                        n_idx.expect("notes.txt row index"),
                        p_idx.expect("Pictures row index"),
                        t_idx.expect("two-line row index"),
                        l_idx.expect("very-long row index"),
                    )
                });

            // Select screenshot.png and wait for background inspection worker to decode its PNG thumbnail.
            browser.select_row(screenshot_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(8);

            let cache = browser.0.inspections.get_untracked();
            let thumb_deadline = Instant::now() + Duration::from_secs(5);
            while cache.pending_count() > 0
                || cache.running_count() > 0
                || last_color_sprites().len() < 2
            {
                assert!(
                    Instant::now() < thumb_deadline,
                    "timed out waiting for screenshot.png thumbnail decode"
                );
                harness.advance(Duration::from_millis(20));
                harness.pump();
                zgui::reactive::flush();
                harness.settle(4);
                std::thread::sleep(Duration::from_millis(5));
            }
            zgui::reactive::flush();
            harness.settle(16);

            // 1. Grid tile thumbnail wrapper fills the full tile content width (> 120px) x 100px height!
            let tile_thumb = super::super::inspection::tests::tile_thumb_node("screenshot.png")
                .expect("recorded .gds-files-tile-thumb-wrap for screenshot.png");
            let scale = tile_thumb.scale().max(1.0);
            let tile_thumb_bounds = tile_thumb
                .bounds()
                .expect("measured bounds for .gds-files-tile-thumb-wrap");
            let tile_thumb_w = tile_thumb_bounds.size.width.0 / scale;
            let tile_thumb_h = tile_thumb_bounds.size.height.0 / scale;
            assert!(
                tile_thumb_w > 120.0 && (tile_thumb_h - 100.0).abs() < 0.5,
                "expected Grid thumbnail wrapper > 120px wide x 100px tall (never 48px), got {tile_thumb_w:.1}x{tile_thumb_h:.1}"
            );

            // Verify actual painted ColorSprites for our 16:9 PNG fill full Grid (> 120px wide)
            // and Details (> 240px wide) slots via object-fit: contain (never 48x27px or 144x81px)!
            let sprites: Vec<(f32, f32)> = last_color_sprites()
                .into_iter()
                .map(|(w, h)| (w / scale, h / scale))
                .collect();
            assert!(
                sprites.iter().any(|&(w, h)| w > 120.0 && h > 65.0),
                "expected painted Grid 16:9 thumbnail ColorSprite > 120px wide (never 48x27px), got {sprites:?}"
            );
            assert!(
                sprites.iter().any(|&(w, h)| w > 240.0 && h > 130.0),
                "expected painted Details 16:9 thumbnail ColorSprite > 240px wide (never 144x81px), got {sprites:?}"
            );

            // 2. Fallback icons in Grid view remain 48x48px!
            for fallback_name in ["Pictures", "notes.txt"] {
                let fb_node = super::super::inspection::tests::tile_fallback_node(fallback_name)
                    .unwrap_or_else(|| panic!("recorded fallback slot for {fallback_name}"));
                let fb_bounds = fb_node
                    .bounds()
                    .unwrap_or_else(|| panic!("measured bounds for {fallback_name} fallback"));
                let fb_w = fb_bounds.size.width.0 / scale;
                let fb_h = fb_bounds.size.height.0 / scale;
                assert!(
                    (fb_w - 48.0).abs() < 0.5 && (fb_h - 48.0).abs() < 0.5,
                    "expected Grid fallback icon for {fallback_name} to remain 48x48px, got {fb_w:.1}x{fb_h:.1}"
                );
            }

            // 3. No Shock contract: Details pane stage (168px), 2-line title slot (44px), and
            //    scrollable body origin Y stay at identical pixel coordinates across images,
            //    plain files, folders, 2-line filenames, 3+ line clamped filenames, and multi-select!
            let measure_details_anchors = |label: &str| -> (f32, f32, f32, f32, f32) {
                let stage =
                    super::super::inspection::tests::details_thumb_node().unwrap_or_else(|| {
                        panic!("recorded .gds-files-details__thumb-wrap for {label}")
                    });
                let title = super::super::inspection::tests::details_title_slot_node()
                    .unwrap_or_else(|| {
                        panic!("recorded .gds-files-details__title-slot for {label}")
                    });
                let body = super::super::inspection::tests::details_body_node()
                    .unwrap_or_else(|| panic!("recorded .gds-files-details__body for {label}"));
                let sb = stage.window_bounds().expect("stage window bounds");
                let tb = title.window_bounds().expect("title slot window bounds");
                let bb = body.window_bounds().expect("body window bounds");
                (
                    sb.size.width.0 / scale,
                    sb.size.height.0 / scale,
                    sb.origin.y.0 / scale,
                    tb.origin.y.0 / scale,
                    bb.origin.y.0 / scale,
                )
            };

            let (details_w, details_h, base_stage_y, base_title_y, base_body_y) =
                measure_details_anchors("screenshot.png");
            let base_title_h = super::super::inspection::tests::details_title_slot_node()
                .expect("title slot")
                .bounds()
                .expect("title bounds")
                .size
                .height
                .0
                / scale;
            assert_content_width_matches_layout(&browser);
            assert!(
                details_w > 240.0 && (details_h - 168.0).abs() < 0.5,
                "expected Details media box to span full width (> 240px) x 168px, got {details_w:.1}x{details_h:.1}"
            );
            assert!(
                (base_title_h - 44.0).abs() < 0.5,
                "expected fixed 2-line Details title slot height == 44px, got {base_title_h:.1}"
            );

            // Selecting non-thumbnail `notes.txt` keeps the exact 168px stage + 44px title slot +
            // body Y anchor while centering a 64x64px icon inside the stage.
            browser.select_row(notes_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(16);
            let (nw, nh, n_stage_y, n_title_y, n_body_y) = measure_details_anchors("notes.txt");
            assert!(
                nw > 240.0
                    && (nh - 168.0).abs() < 0.5
                    && (n_stage_y - base_stage_y).abs() < 0.5
                    && (n_title_y - base_title_y).abs() < 0.5
                    && (n_body_y - base_body_y).abs() < 0.5,
                "No Shock violation on notes.txt: stage=({nw:.1}x{nh:.1} @ {n_stage_y:.1}), title_y={n_title_y:.1}, body_y={n_body_y:.1} vs base ({base_stage_y:.1}, {base_title_y:.1}, {base_body_y:.1})"
            );
            let details_fb = super::super::inspection::tests::details_fallback_node()
                .expect("recorded Details 64px fallback artwork");
            let details_fb_bounds = details_fb
                .bounds()
                .expect("measured bounds for Details 64px fallback");
            let dfb_w = details_fb_bounds.size.width.0 / scale;
            let dfb_h = details_fb_bounds.size.height.0 / scale;
            assert!(
                (dfb_w - 64.0).abs() < 0.5 && (dfb_h - 64.0).abs() < 0.5,
                "expected Details fallback icon inside 168px stage to measure 64x64px, got {dfb_w:.1}x{dfb_h:.1}"
            );

            // Check folder, 2-line hash filename, and 3+ line clamped filename: 0px anchor shift!
            for (idx, label) in [
                (pictures_idx, "Pictures"),
                (two_line_idx, two_line_name),
                (very_long_idx, very_long_name),
            ] {
                browser.select_row(idx, Modifiers::NONE);
                zgui::reactive::flush();
                harness.settle(16);
                let (sw, sh, sy, ty, by) = measure_details_anchors(label);
                assert!(
                    sw > 240.0
                        && (sh - 168.0).abs() < 0.5
                        && (sy - base_stage_y).abs() < 0.5
                        && (ty - base_title_y).abs() < 0.5
                        && (by - base_body_y).abs() < 0.5,
                    "No Shock violation on {label}: stage=({sw:.1}x{sh:.1} @ {sy:.1}), title_y={ty:.1}, body_y={by:.1} vs base ({base_stage_y:.1}, {base_title_y:.1}, {base_body_y:.1})"
                );
            }

            // Check multi-selection (`Ctrl` toggle across 2 files + 1 folder): 0px anchor shift
            // AND displays item count ("项目数量"), folder/file breakdown, and combined byte size!
            browser.select_row(screenshot_idx, Modifiers::CONTROL);
            browser.select_row(pictures_idx, Modifiers::CONTROL);
            let multi_deadline = Instant::now() + Duration::from_secs(3);
            let live_texts = loop {
                harness.advance(Duration::from_millis(15));
                harness.pump();
                zgui::reactive::flush();
                harness.settle(8);
                let mut nodes = std::collections::HashMap::new();
                for update in offscreen.a11y_log() {
                    for (id, node) in update.nodes {
                        nodes.insert(id, node);
                    }
                }
                let texts: Vec<String> = nodes
                    .values()
                    .filter(|n| !n.is_hidden())
                    .filter_map(|n| n.value().or_else(|| n.label()).map(str::to_owned))
                    .collect();
                if texts.iter().any(|t| t == &expected_multi_size) {
                    break texts;
                }
                assert!(
                    Instant::now() < multi_deadline,
                    "timed out waiting for multi-select combined size {expected_multi_size:?}; live_texts={texts:?}"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            let items_label = gds::i18n::text("details-items");
            let size_label = gds::i18n::text("size");
            let folders_label = gds::i18n::text("details-folders");
            let files_label = gds::i18n::text("details-files");
            for expected_token in [
                items_label.as_str(),
                "3",
                size_label.as_str(),
                expected_multi_size.as_str(),
                folders_label.as_str(),
                files_label.as_str(),
            ] {
                assert!(
                    live_texts.iter().any(|t| t == expected_token),
                    "expected multi-select details row token {expected_token:?} in {live_texts:?}"
                );
            }
            let (mw, mh, my, mty, mby) = measure_details_anchors("multi-select");
            assert!(
                mw > 240.0
                    && (mh - 168.0).abs() < 0.5
                    && (my - base_stage_y).abs() < 0.5
                    && (mty - base_title_y).abs() < 0.5
                    && (mby - base_body_y).abs() < 0.5,
                "No Shock violation on multi-select: stage=({mw:.1}x{mh:.1} @ {my:.1}), title_y={mty:.1}, body_y={mby:.1} vs base ({base_stage_y:.1}, {base_title_y:.1}, {base_body_y:.1})"
            );

            // Re-selecting `screenshot.png` after toggle cycle keeps the full-width (> 240px) x 168px
            // media box and identical No Shock anchors.
            browser.select_row(screenshot_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(16);
            assert!(
                !details_fb.is_bound(),
                "re-selecting image in Details pane must unmount the 64px fallback slot"
            );
            let (details_w2, details_h2, sy2, ty2, by2) =
                measure_details_anchors("screenshot.png re-select");
            assert!(
                details_w2 > 240.0
                    && (details_h2 - 168.0).abs() < 0.5
                    && (sy2 - base_stage_y).abs() < 0.5
                    && (ty2 - base_title_y).abs() < 0.5
                    && (by2 - base_body_y).abs() < 0.5,
                "expected re-selected Details media box to preserve full width x 168px and anchors, got {details_w2:.1}x{details_h2:.1}"
            );

            browser.toggle_details();
            zgui::reactive::flush();
            harness.settle(8);
            browser.toggle_details();
            zgui::reactive::flush();
            harness.settle(8);
            browser.select_row(screenshot_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(8);
            browser.select_row(notes_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(8);
            browser.select_row(screenshot_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(16);
            let (details_w3, details_h3, sy3, ty3, by3) =
                measure_details_anchors("screenshot.png post-toggle");
            assert!(
                details_w3 > 240.0
                    && (details_h3 - 168.0).abs() < 0.5
                    && (sy3 - base_stage_y).abs() < 0.5
                    && (ty3 - base_title_y).abs() < 0.5
                    && (by3 - base_body_y).abs() < 0.5,
                "expected Details media box after toggle + regular-file switch to stay > 240px x 168px at fixed anchors, got {details_w3:.1}x{details_h3:.1}"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope: Scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::English).expect("English catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("grid thumbnail and details media layout harness");
    }

    fn send_ctrl_b(harness: &mut Harness<Box<dyn AppHandler>>) {
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::character("b"),
            modifiers: Modifiers::CONTROL,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event: KeyEvent::character("b"),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.pump();
        zgui::reactive::flush();
    }

    fn click_node_center(harness: &mut Harness<Box<dyn AppHandler>>, node: NodeRef) {
        let scale = node.scale().max(1.0);
        let bounds = node.window_bounds().expect("measured node window_bounds");
        let cx = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
        let cy = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;
        let pos = Point::new(CssPx(cx), CssPx(cy));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(8);
        zgui::reactive::flush();
    }

    #[test]
    fn fresh_launch_ctrl_b_toggles_sidebar_repeatedly_across_frame_zero_transitions_and_sidebar_focus()
     {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("Desktop")).expect("create Desktop");
        std::fs::write(home.join("notes.txt"), b"hello\n").expect("create notes.txt");
        clear_view_probes();
        super::super::chrome::tests::clear_recorded_tab_nodes();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home;

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            let browser = browser_slot.borrow().clone().expect("browser constructed");
            assert!(
                browser.0.sidebar.get_untracked(),
                "sidebar starts expanded at desktop width"
            );

            // 1. Frame 0 immediately after mounting `browser.view()`, BEFORE `settle()` / frame callbacks!
            for (step, expected) in [false, true, false, true].into_iter().enumerate() {
                send_ctrl_b(&mut harness);
                assert_eq!(
                    browser.0.sidebar.get_untracked(),
                    expected,
                    "Frame-0 Ctrl+B press #{} expected sidebar == {expected}",
                    step + 1
                );
            }

            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(8);
            let load_deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.len() < 2) {
                assert!(
                    Instant::now() < load_deadline,
                    "timed out waiting for directory rows before Ctrl+B transition"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            harness.settle(8);

            // 2. Mid-transition (`48ms`) repeated `Ctrl+B` presses.
            for (step, expected) in [false, true, false, true].into_iter().enumerate() {
                send_ctrl_b(&mut harness);
                harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
                assert_content_width_matches_layout(&browser);
                assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);
                zgui::reactive::flush();
                assert_eq!(
                    browser.0.sidebar.get_untracked(),
                    expected,
                    "Mid-transition (48ms) Ctrl+B press #{} expected sidebar == {expected}",
                    step + 1
                );
            }
            harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            harness.settle(4);

            // 3. Full `200ms` settle repeated `Ctrl+B` presses.
            for (step, expected) in [false, true, false, true].into_iter().enumerate() {
                send_ctrl_b(&mut harness);
                harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
                zgui::reactive::flush();
                harness.settle(4);
                assert_eq!(
                    browser.0.sidebar.get_untracked(),
                    expected,
                    "Settled (200ms) Ctrl+B press #{} expected sidebar == {expected}",
                    step + 1
                );
                let expected_w = if expected {
                    SIDEBAR_WIDTH
                } else {
                    COLLAPSED_SIDEBAR_WIDTH
                };
                assert!(
                    (browser.sidebar_width().get_untracked() - expected_w).abs() < 0.5,
                    "expected settled sidebar_width == {expected_w}, got {}",
                    browser.sidebar_width().get_untracked()
                );
            }

            // 4. While a sidebar tab holds focus, and across unrelated revision / selection bumps!
            let tab_id = browser.read_untracked(|state| state.tab().id);
            let tab_node = super::super::chrome::tests::recorded_tab_node(tab_id)
                .expect("recorded active sidebar tab node");
            let initial_tab_id = tab_node.get_untracked().expect("bound sidebar tab NodeId");
            browser.changed();
            browser.update_selection(|_| {});
            zgui::reactive::flush();
            harness.settle(4);
            assert_eq!(
                tab_node.get_untracked(),
                Some(initial_tab_id),
                "unrelated revision/selection bumps must never unmount or recreate sidebar_tab"
            );

            tab_node.focus();
            harness.settle(4);
            for (step, expected) in [false, true, false, true].into_iter().enumerate() {
                send_ctrl_b(&mut harness);
                harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
                zgui::reactive::flush();
                harness.settle(4);
                assert_eq!(
                    browser.0.sidebar.get_untracked(),
                    expected,
                    "Tab-focused Ctrl+B press #{} expected sidebar == {expected}",
                    step + 1
                );
                assert_eq!(
                    tab_node.get_untracked(),
                    Some(initial_tab_id),
                    "sidebar_tab DOM node must remain stable across collapse/expand"
                );
            }

            // 5. While a sidebar heading (`Locations`) holds focus across `is_collapsed_rail`!
            let heading_node = super::super::chrome::tests::heading_node("locations")
                .expect("recorded Locations sidebar heading node");
            let initial_heading_id = heading_node
                .get_untracked()
                .expect("bound sidebar heading NodeId");
            heading_node.focus();
            harness.settle(4);
            for (step, expected) in [false, true, false, true].into_iter().enumerate() {
                send_ctrl_b(&mut harness);
                harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
                zgui::reactive::flush();
                harness.settle(4);
                assert_eq!(
                    browser.0.sidebar.get_untracked(),
                    expected,
                    "Heading-focused Ctrl+B press #{} expected sidebar == {expected}",
                    step + 1
                );
                assert_eq!(
                    heading_node.get_untracked(),
                    Some(initial_heading_id),
                    "sidebar_heading DOM node must remain stable across collapsed-rail tooltip gating"
                );
            }

            // 6. Micro window (< 600px) guards Ctrl+B so `sidebar` preference stays intact.
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(540.0),
                DevicePx(800.0),
            )));
            harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            assert_eq!(
                browser.width_tier_untracked(),
                gds::ui::window::WidthTier::Micro
            );
            assert_content_width_matches_layout(&browser);
            send_ctrl_b(&mut harness);
            assert!(
                browser.0.sidebar.get_untracked(),
                "Ctrl+B in micro mode (< 600px) must not flip underlying sidebar state"
            );

            harness.deliver_to_first(SurfaceEvent::ScaleFactorChanged {
                scale_factor: 1.25,
                size: Size::new(DevicePx(960.0), DevicePx(800.0)),
            });
            harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            assert_content_width_matches_layout(&browser);
            assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope: Scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(true);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::Chinese).expect("Chinese catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("fresh-launch repeated Ctrl+B harness");
    }

    #[test]
    fn footer_terminal_icon_button_pointer_clicks_open_on_first_click_and_close_on_second_click() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home");
        clear_view_probes();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home;

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            assert!(
                !browser.0.terminal.get_untracked(),
                "terminal dock starts closed"
            );

            let term_btn =
                footer_terminal_node().expect("recorded footer terminal IconButton NodeRef");

            // 1st click opens terminal dock; 2nd click closes it; 3rd opens; 4th closes!
            for (click_idx, expected_open) in [true, false, true, false].into_iter().enumerate() {
                click_node_center(&mut harness, term_btn);
                assert_eq!(
                    browser.0.terminal.get_untracked(),
                    expected_open,
                    "footer terminal IconButton click #{} expected terminal == {expected_open}",
                    click_idx + 1
                );
            }

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope: Scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::Chinese).expect("Chinese catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("footer terminal button click toggle harness");
    }

    pub(in crate::pages::workspace) fn send_key_chord(
        harness: &mut Harness<Box<dyn AppHandler>>,
        event: KeyEvent,
        modifiers: Modifiers,
    ) {
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: event.clone(),
            modifiers,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event,
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(6);
        zgui::reactive::flush();
    }

    #[test]
    fn content_focus_single_dispatch_and_open_menu_or_columns_portal_isolates_workspace_shortcuts()
    {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::write(home.join("alpha.txt"), b"a\n").expect("write alpha.txt");
        std::fs::write(home.join("beta.txt"), b"b\n").expect("write beta.txt");
        std::fs::write(home.join("gamma.txt"), b"c\n").expect("write gamma.txt");
        clear_view_probes();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home;

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            let deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.len() < 3) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for 3 directory rows to load"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            zgui::reactive::flush();
            harness.settle(8);

            // 1. Single-dispatch verification when `content_focus` holds focus:
            //    Selecting row 0 and pressing `ArrowDown` ONCE must advance to row 1 (never row 2!).
            browser.0.content_focus.focus();
            browser.select_row(0, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(4);

            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::ArrowDown, PhysicalKey::Code(KeyCode::ArrowDown)),
                Modifiers::NONE,
            );
            let selected_idx = browser.read_untracked(|state| {
                let tab = state.tab();
                let key = tab
                    .navigation
                    .view()
                    .selection
                    .as_slice()
                    .first()
                    .expect("single selected key");
                tab.backing.position(key).expect("selected row index")
            });
            assert_eq!(
                selected_idx, 1,
                "single ArrowDown on content_focus must advance selection by 1 row (to index 1, never 2)"
            );

            // Open a 2nd tab and record a recent location so `Ctrl+W` and `Alt+C` are observable.
            browser.new_tab(None);
            browser.update(|state| {
                state.places.record(Location::from(state.home.as_path()));
            });
            zgui::reactive::flush();
            harness.settle(8);
            assert_eq!(browser.read_untracked(|state| state.tabs.len()), 2);
            assert_eq!(browser.read_untracked(|state| state.places.recent.len()), 1);

            // 2. Open menu (`show_app_menu`) isolation — both when portaled menu holds focus
            //    and when trigger / `root_focus` holds focus!
            browser.show_app_menu();
            zgui::reactive::flush();
            harness.settle(8);
            assert!(browser.0.menus.is_open());

            for focus_root in [false, true] {
                if focus_root {
                    browser.0.root_focus.focus();
                    harness.settle(4);
                }
                send_key_chord(&mut harness, KeyEvent::character("b"), Modifiers::CONTROL);
                send_key_chord(&mut harness, KeyEvent::character("w"), Modifiers::CONTROL);
                send_key_chord(&mut harness, KeyEvent::character("c"), Modifiers::ALT);
                assert!(
                    browser.0.sidebar.get_untracked(),
                    "open menu must block Ctrl+B (focus_root={focus_root})"
                );
                assert_eq!(
                    browser.read_untracked(|state| state.tabs.len()),
                    2,
                    "open menu must block Ctrl+W (focus_root={focus_root})"
                );
                assert_eq!(
                    browser.read_untracked(|state| state.places.recent.len()),
                    1,
                    "open menu must block Alt+C ClearRecent (focus_root={focus_root})"
                );
                assert!(
                    browser.0.menus.is_open(),
                    "menu stays open across unhandled workspace chords (focus_root={focus_root})"
                );
            }

            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                Modifiers::NONE,
            );
            assert!(
                !browser.0.menus.is_open(),
                "Escape must close open application menu cleanly"
            );

            // 3. `Surface::Columns` portaled `Dialog` isolation — both when portaled dialog holds focus
            //    and when `content_focus` holds focus!
            browser.configure_columns();
            zgui::reactive::flush();
            harness.settle(8);
            assert!(matches!(
                browser.0.menus.surface.get_untracked(),
                super::Surface::Columns { .. }
            ));

            for focus_content in [false, true] {
                if focus_content {
                    browser.0.content_focus.focus();
                    harness.settle(4);
                }
                send_key_chord(&mut harness, KeyEvent::character("b"), Modifiers::CONTROL);
                send_key_chord(&mut harness, KeyEvent::character("w"), Modifiers::CONTROL);
                if focus_content {
                    // When `DialogContent` itself holds focus, `Alt+C` is `DialogActions`'s own Cancel mnemonic;
                    // when `content_focus` holds focus, `menus.is_open()` must block `Alt+C` (`ClearRecent`).
                    send_key_chord(&mut harness, KeyEvent::character("c"), Modifiers::ALT);
                }
                send_key_chord(&mut harness, KeyEvent::character("m"), Modifiers::ALT);
                assert!(
                    browser.0.sidebar.get_untracked(),
                    "Surface::Columns must block Ctrl+B (focus_content={focus_content})"
                );
                assert_eq!(
                    browser.read_untracked(|state| state.tabs.len()),
                    2,
                    "Surface::Columns must block Ctrl+W (focus_content={focus_content})"
                );
                assert_eq!(
                    browser.read_untracked(|state| state.places.recent.len()),
                    1,
                    "Surface::Columns must block Alt+C ClearRecent (focus_content={focus_content})"
                );
                assert!(
                    matches!(
                        browser.0.menus.surface.get_untracked(),
                        super::Surface::Columns { .. }
                    ),
                    "Surface::Columns must not be overwritten by Alt+M (focus_content={focus_content})"
                );
            }

            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                Modifiers::NONE,
            );
            assert!(
                !browser.0.menus.is_open(),
                "Escape must close Surface::Columns dialog cleanly"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope: Scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::English).expect("English catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("single-dispatch and portal isolation harness");
    }

    #[test]
    fn context_menu_dismissal_modal_scope_cleanup_and_shift_delete_alt_mnemonics() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::write(home.join("victim.txt"), b"bye\n").expect("write victim.txt");
        clear_view_probes();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home.clone();

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            let deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.is_empty()) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for victim.txt to load"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            zgui::reactive::flush();
            harness.settle(8);

            // 1. Right-click empty directory space -> click "New Folder" (first item) ->
            //    context menu must dismiss immediately and stay dismissed after creating the folder.
            let menu_origin = Point::new(CssPx(640.0), CssPx(420.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(menu_origin).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(menu_origin).with_button(PointerButton::Secondary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(8);
            zgui::reactive::flush();
            assert!(
                browser.0.menus.is_open(),
                "right-clicking empty directory space must open directory context menu"
            );

            // Opening the context menu via right-click must NOT pre-highlight/focus item 0:
            // pressing Enter before any pointer hover or ArrowDown must leave the menu open
            // without triggering item 0 ("New Folder").
            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
                Modifiers::NONE,
            );
            assert!(
                matches!(browser.0.prompt.get_untracked(), super::Prompt::Closed),
                "context menu must not pre-focus item 0 on open"
            );
            assert!(
                browser.0.menus.is_open(),
                "unfocused Enter on newly opened context menu must keep menu open"
            );

            // Click the first menu item ("New Folder") inside the anchored context menu.
            let first_item_pos = Point::new(CssPx(680.0), CssPx(438.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(first_item_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(first_item_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(8);
            zgui::reactive::flush();
            assert!(
                matches!(browser.0.prompt.get_untracked(), super::Prompt::Name { .. }),
                "clicking New Folder must open NameForm prompt"
            );
            assert!(
                !browser.0.menus.is_open(),
                "clicking New Folder in context menu must immediately dismiss the context menu"
            );

            // Submit the new folder with Enter and wait for creation.
            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
                Modifiers::NONE,
            );
            let folder_deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.len() < 2) {
                assert!(
                    Instant::now() < folder_deadline,
                    "timed out waiting for created folder to appear"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            zgui::reactive::flush();
            harness.settle(8);
            assert!(
                !browser.0.menus.is_open(),
                "context menu must remain closed after new folder creation completes"
            );

            // 2. Opening and closing a `DialogScope::enter()` modal (`Surface::Columns`) must NOT
            //    leak `modal_scopes` in `KeytipRegistry` (which would block subsequent shortcuts
            //    and suppress modal `Alt` keytips).
            browser.configure_columns();
            zgui::reactive::flush();
            harness.settle(8);
            assert!(gds::ui::keytips::has_active_modal_scope());
            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                Modifiers::NONE,
            );
            assert!(
                !gds::ui::keytips::has_active_modal_scope(),
                "closing Surface::Columns must clean up its modal scope from KeytipRegistry"
            );

            let collect_labels = |harness: &Harness<Box<dyn AppHandler>>| -> Vec<String> {
                let Some(surface) = harness.platform().offscreens().first().cloned() else {
                    return Vec::new();
                };
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
                let mut labels = Vec::new();
                while let Some(id) = stack.pop() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let Some(node) = nodes.get(&id) else {
                        continue;
                    };
                    if node.role() == Role::Label
                        && let Some(lbl) = node.label()
                    {
                        labels.push(lbl.to_owned());
                    }
                    for &child in node.children().iter().rev() {
                        stack.push(child);
                    }
                }
                labels
            };

            // Holding `Alt` in the workspace after closing `Surface::Columns` must still reveal
            // workspace keytip badges (e.g., `"M"` on the application menu button).
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(6);
            zgui::reactive::flush();
            harness.settle(4);
            let workspace_labels = collect_labels(&harness);
            assert!(
                workspace_labels.iter().any(|lbl| lbl == "M"),
                "workspace Alt keytip 'M' must appear after closing Surface::Columns, got {workspace_labels:?}"
            );
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Released,
                event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(6);
            zgui::reactive::flush();

            // 3. Select `victim.txt` and press `Shift+Delete` -> `Alt+C` cancels, then `Shift+Delete`
            //    again -> `Alt+A` confirms permanent deletion.
            let victim_idx = browser.read_untracked(|state| {
                state
                    .tab()
                    .rows
                    .iter()
                    .position(|row| row.label() == "victim.txt")
                    .expect("victim.txt row")
            });
            browser.0.content_focus.focus();
            browser.select_row(victim_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(6);

            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::Delete, PhysicalKey::Code(KeyCode::Delete)),
                Modifiers::SHIFT,
            );
            assert!(
                matches!(
                    browser.0.prompt.get_untracked(),
                    super::Prompt::Confirm { .. }
                ),
                "Shift+Delete must open permanent delete confirmation prompt"
            );
            assert!(
                gds::ui::keytips::has_active_modal_scope(),
                "Prompt::Confirm must activate modal DialogScope"
            );

            // Holding `Alt` inside `Prompt::Confirm` must mount the modal `"A"` and `"C"` keytip
            // badges while suppressing workspace `"M"`.
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(6);
            zgui::reactive::flush();
            harness.settle(4);
            let modal_labels = collect_labels(&harness);
            assert!(
                modal_labels.iter().any(|lbl| lbl == "A")
                    && modal_labels.iter().any(|lbl| lbl == "C")
                    && !modal_labels.iter().any(|lbl| lbl == "M"),
                "holding Alt inside Shift+Delete confirmation must show modal 'A'/'C' keytips and suppress workspace 'M', got {modal_labels:?}"
            );
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Released,
                event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(6);
            zgui::reactive::flush();

            // Cancel via `Alt+C`.
            send_key_chord(&mut harness, KeyEvent::character("c"), Modifiers::ALT);
            assert!(
                matches!(browser.0.prompt.get_untracked(), super::Prompt::Closed),
                "Alt+C must cancel the Shift+Delete confirmation dialog"
            );
            assert!(
                !gds::ui::keytips::has_active_modal_scope(),
                "cancelling Prompt::Confirm must release its modal DialogScope"
            );
            assert!(home.join("victim.txt").exists());

            // Re-open with `Shift+Delete` and confirm via `Alt+A`.
            browser.0.content_focus.focus();
            browser.select_row(victim_idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(6);
            send_key_chord(
                &mut harness,
                KeyEvent::named(NamedKey::Delete, PhysicalKey::Code(KeyCode::Delete)),
                Modifiers::SHIFT,
            );
            assert!(matches!(
                browser.0.prompt.get_untracked(),
                super::Prompt::Confirm { .. }
            ));

            send_key_chord(&mut harness, KeyEvent::character("a"), Modifiers::ALT);
            assert!(
                matches!(browser.0.prompt.get_untracked(), super::Prompt::Closed),
                "Alt+A must confirm the Shift+Delete permanent deletion dialog"
            );
            let delete_deadline = Instant::now() + Duration::from_secs(3);
            while home.join("victim.txt").exists() {
                assert!(
                    Instant::now() < delete_deadline,
                    "timed out waiting for Alt+A permanent delete to remove victim.txt"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope: Scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::English).expect("English catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("context menu, modal scope cleanup, and Shift+Delete Alt mnemonics harness");
    }

    #[test]
    fn typeahead_filename_matching_and_auto_focus_search_switch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::create_dir(home.join("My Documents")).expect("create My Documents");
        std::fs::create_dir(home.join("My Pictures")).expect("create My Pictures");
        for name in [
            "Cargo.toml",
            "config.toml",
            "const.rs",
            "Lemur.txt",
            "Leopard.txt",
            "Lion.txt",
            "Llama.txt",
            "notes.txt",
            "zeta.txt",
        ] {
            std::fs::write(home.join(name), b"x\n").expect("write fixture");
        }
        for i in 1..=25 {
            std::fs::write(home.join(format!("item-{i:02}.txt")), b"x\n").expect("write filler");
        }
        clear_view_probes();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home;

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            let deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.len() < 36) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for fixture directory to load"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            zgui::reactive::flush();
            harness.settle(8);

            let selected_label = |browser: &Browser| -> Option<String> {
                browser.read_untracked(|state| {
                    let tab = state.tab();
                    let key = tab.navigation.view().selection.as_slice().first()?;
                    let idx = tab.backing.position(key)?;
                    tab.rows.get(idx).map(super::Row::label)
                })
            };

            let send_key_at =
                |harness: &mut Harness<Box<dyn AppHandler>>, event: KeyEvent, at_ms: u64| {
                    let stamp = Timestamp::from_origin(Duration::from_millis(at_ms));
                    harness.deliver_to_first(SurfaceEvent::Key {
                        state: KeyState::Pressed,
                        event: event.clone(),
                        modifiers: Modifiers::NONE,
                        timestamp: stamp,
                    });
                    harness.deliver_to_first(SurfaceEvent::Key {
                        state: KeyState::Released,
                        event,
                        modifiers: Modifiers::NONE,
                        timestamp: stamp,
                    });
                    harness.settle(4);
                    zgui::reactive::flush();
                };

            assert!(!browser.0.preferences.snapshot().files.auto_focus_search);
            browser.0.content_focus.focus();
            zgui::reactive::flush();
            harness.settle(4);

            // 1. Continuous multi-key prefix matching ("c", "o", "n", "s").
            send_key_at(&mut harness, KeyEvent::character("c"), 100);
            assert_eq!(selected_label(&browser).as_deref(), Some("Cargo.toml"));
            assert!(browser.0.filter.get_untracked().is_empty());
            assert!(browser.0.list_focus_visible.get_untracked());

            send_key_at(&mut harness, KeyEvent::character("o"), 250);
            assert_eq!(selected_label(&browser).as_deref(), Some("config.toml"));

            send_key_at(&mut harness, KeyEvent::character("n"), 400);
            assert_eq!(selected_label(&browser).as_deref(), Some("config.toml"));

            send_key_at(&mut harness, KeyEvent::character("s"), 550);
            assert_eq!(selected_label(&browser).as_deref(), Some("const.rs"));

            // 2. Repeated same character cycles ("l", "l") and distinct key switches to prefix ("a" -> "lla").
            send_key_at(&mut harness, KeyEvent::character("l"), 2000);
            assert_eq!(selected_label(&browser).as_deref(), Some("Lemur.txt"));
            send_key_at(&mut harness, KeyEvent::character("l"), 2150);
            assert_eq!(selected_label(&browser).as_deref(), Some("Leopard.txt"));
            send_key_at(&mut harness, KeyEvent::character("a"), 2300);
            assert_eq!(selected_label(&browser).as_deref(), Some("Llama.txt"));

            // 3. Space mid-query matches filenames with spaces ("my p" -> "My Pictures").
            send_key_at(&mut harness, KeyEvent::character("m"), 4000);
            assert_eq!(selected_label(&browser).as_deref(), Some("My Documents"));
            send_key_at(&mut harness, KeyEvent::character("y"), 4150);
            assert_eq!(selected_label(&browser).as_deref(), Some("My Documents"));
            send_key_at(
                &mut harness,
                KeyEvent::named(NamedKey::Space, PhysicalKey::Code(KeyCode::Space)),
                4300,
            );
            assert_eq!(selected_label(&browser).as_deref(), Some("My Documents"));
            send_key_at(&mut harness, KeyEvent::character("p"), 4450);
            assert_eq!(selected_label(&browser).as_deref(), Some("My Pictures"));

            // 4. Backspace corrects a mistyped character mid-query.
            send_key_at(&mut harness, KeyEvent::character("c"), 6000);
            send_key_at(&mut harness, KeyEvent::character("o"), 6150);
            send_key_at(&mut harness, KeyEvent::character("x"), 6300);
            assert_eq!(selected_label(&browser).as_deref(), Some("config.toml"));
            send_key_at(
                &mut harness,
                KeyEvent::named(NamedKey::Backspace, PhysicalKey::Code(KeyCode::Backspace)),
                6450,
            );
            send_key_at(&mut harness, KeyEvent::character("n"), 6600);
            send_key_at(&mut harness, KeyEvent::character("s"), 6750);
            assert_eq!(selected_label(&browser).as_deref(), Some("const.rs"));

            // 5. Off-screen match scrolls into view.
            send_key_at(&mut harness, KeyEvent::character("z"), 8000);
            assert_eq!(selected_label(&browser).as_deref(), Some("zeta.txt"));
            assert!(
                browser.0.list.scroll_offset().y.0 > 0.0,
                "matching an off-screen row must scroll the list viewport"
            );

            // 6. Escape resets active type-ahead immediately (no wait for timeout).
            send_key_at(&mut harness, KeyEvent::character("c"), 10_000);
            assert_eq!(selected_label(&browser).as_deref(), Some("Cargo.toml"));
            send_key_at(
                &mut harness,
                KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                10_100,
            );
            send_key_at(&mut harness, KeyEvent::character("n"), 10_200);
            assert_eq!(selected_label(&browser).as_deref(), Some("notes.txt"));

            // 7. Open menu blocks type-ahead.
            browser.show_app_menu();
            zgui::reactive::flush();
            harness.settle(6);
            send_key_at(&mut harness, KeyEvent::character("c"), 12_000);
            assert_eq!(selected_label(&browser).as_deref(), Some("notes.txt"));
            send_key_at(
                &mut harness,
                KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                12_100,
            );

            // 8. Grid mode also supports type-ahead.
            browser.toggle_view();
            zgui::reactive::flush();
            harness.settle(8);
            browser.0.content_focus.focus();
            zgui::reactive::flush();
            harness.settle(4);
            send_key_at(&mut harness, KeyEvent::character("l"), 14_000);
            send_key_at(&mut harness, KeyEvent::character("i"), 14_150);
            assert_eq!(selected_label(&browser).as_deref(), Some("Lion.txt"));

            // 9. Enabling `auto_focus_search` switches typing to focus the filter field in order.
            browser.edit_preferences(|prefs| prefs.files.auto_focus_search = true);
            zgui::reactive::flush();
            harness.settle(6);
            browser.0.content_focus.focus();
            zgui::reactive::flush();
            harness.settle(4);
            send_key_at(&mut harness, KeyEvent::character("n"), 16_000);
            send_key_at(&mut harness, KeyEvent::character("o"), 16_150);
            assert_eq!(browser.0.filter.get_untracked(), "no");
            assert_eq!(browser.read_untracked(|state| state.tab().rows.len()), 1);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope: Scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::English).expect("English catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("typeahead and auto-focus search harness");
    }
}
