use zgui::prelude::*;
use zgui::{component, css, view};

const SHEET: &str = css!(
    r#"
    .gds-picker.gds-picker {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-width: 0px;
        min-height: 0px;
        width: 100%;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
    }

    .gds-picker__toolbar.gds-picker__toolbar {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0px;
        flex-shrink: 0;
    }

    .gds-picker__location.gds-picker__location {
        flex: 1 1 0%;
        min-width: 0px;
        display: flex;
        flex-direction: row;
        align-items: center;
        overflow: hidden;
    }

    .gds-picker__content.gds-picker__content {
        flex: 1 1 0%;
        min-width: 0px;
        min-height: 200px;
        max-height: 440px;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        border: none;
        border-radius: 0px;
        background: transparent;
    }

    .gds-picker__status.gds-picker__status {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: flex-start;
        gap: 8px;
        min-width: 0px;
        flex-shrink: 0;
        height: var(--gds-icon-button-size, 32px);
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }
    "#
);

fn install_picker_stylesheets() {
    if zgui::view::current_host().is_some() {
        crate::ui::styles::install_ui_stylesheets();
        zgui::view::install_stylesheet("gds-ui-picker-anatomy", SHEET);
    }
}

/// A picker column; its host owns the title and decision footer.
#[component]
pub fn Picker(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_picker_stylesheets();
    view! {
        column(class = "gds-picker", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}

/// Compact navigation or filter controls with fixed, shared spacing.
#[component]
pub fn PickerToolbar(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_picker_stylesheets();
    view! {
        row(class = "gds-picker__toolbar", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}

/// The flexible location slot, shared by breadcrumbs and path editing.
#[component]
pub fn PickerLocation(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_picker_stylesheets();
    view! {
        row(class = "gds-picker__location", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}

/// Scrollable picker result body; navigation and decisions remain outside it.
#[component]
pub fn PickerContent(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_picker_stylesheets();
    view! {
        box(class = "gds-picker__content", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}

/// Status line below a picker listing for loading progress and current choice.
#[component]
pub fn PickerStatus(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_picker_stylesheets();
    view! {
        row(class = "gds-picker__status", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
