use zgui::prelude::*;
use zgui::{component, view};

/// Fills remaining sidebar height and scrolls vertically by default; pass `scrollable = false`
/// when child sections manage independent scrollports.
#[component]
pub fn SideBarContent(
    #[prop(default = true)] scrollable: bool,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_sidebar_stylesheets();
    if scrollable {
        AnyView::new(view! {
            scroll(class = "gds-sidebar__content", class = class, {..attrs}) {
                column(class = "gds-sidebar__content-inner") {{children.into_view_once()}}
            }
        })
    } else {
        AnyView::new(view! {
            column(class = "gds-sidebar__content--static", class = class, {..attrs}) {
                {children.into_view_once()}
            }
        })
    }
}
