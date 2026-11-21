use zgui::prelude::*;
use zgui::{component, view};

/// Column for section items; scrolling is opt-in (`scrollable = true`).
#[component]
pub fn SideBarSectionContent(
    #[prop(default = false)] scrollable: bool,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_sidebar_stylesheets();
    if scrollable {
        AnyView::new(view! {
            scroll(class = "gds-sidebar__section-content--scrollable", class = class, {..attrs}) {
                column(class = "gds-sidebar__section-content-inner") {{children.into_view_once()}}
            }
        })
    } else {
        AnyView::new(view! {
            column(class = "gds-sidebar__section-content", class = class, {..attrs}) {
                {children.into_view_once()}
            }
        })
    }
}
