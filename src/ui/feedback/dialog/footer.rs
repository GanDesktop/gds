use zgui::prelude::*;
use zgui::{component, css, view};
use zgui_ui::DialogFooterProps as ZuiDialogFooterProps;

const SHEET: &str = css!(
    r#"
    .gds-dialog-footer {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
        width: 100%;
    }
    "#
);

/// Footer band aligned to the trailing edge of a GDS modal dialog.
#[component]
pub fn DialogFooter(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-dialog-footer", SHEET);
    }
    view! {
        ZuiDialogFooter(class = "gds-dialog-footer", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
