//! Shared content page layout containers and history stack.

mod content;
mod header;
mod notice;
pub mod stack;
mod table;

pub use content::{PageContent, PageContentProps};
pub use header::{
    PageActions, PageActionsProps, PageHeader, PageHeaderProps, PageTitle, PageTitleProps,
};
pub use notice::{PageNotice, PageNoticeProps};
pub use table::{PageTableArea, PageTableAreaProps};

use zgui::prelude::*;
use zgui::{component, css, view};

pub const HEADER_PADDING_X: f32 = 24.0;
pub const HEADER_PADDING_Y: f32 = 16.0;
pub const HEADER_GAP: f32 = 16.0;
pub const TITLE_SIZE: f32 = 22.0;
pub const ACTION_GAP: f32 = 8.0;
pub const TABLE_INSET: f32 = 18.0;

const PAGE_SHEET: &str = "gds-ui-page";

const PAGE_CSS: &str = css!(
    ".gds-content-page {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
    }
    .gds-page-header {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        gap: 8px;
        padding: 16px 24px;
        min-width: 0px;
    }
    .gds-page-title {
        flex: 1 1 0px;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-weight: 600;
        font-size: calc(22px * var(--gds-font-scale, 1));
    }
    .gds-page-actions {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        gap: 8px;
    }
    .gds-page-notice {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        min-width: 0px;
        gap: 8px;
        padding: 0px 24px 8px 24px;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-font-size-xs, 12px);
    }
    .gds-page-content {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }
    .gds-page-table-area {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
        padding: 0px 18px 4px 18px;
    }"
);

pub(super) fn install_page_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(PAGE_SHEET, PAGE_CSS);
}

/// Top-level workspace content page container.
#[component]
pub fn ContentPage(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_page_stylesheets();
    view! {
        column(class = "gds-content-page", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
