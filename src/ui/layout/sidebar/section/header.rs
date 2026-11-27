use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn SideBarSectionHeader(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_sidebar_stylesheets();
    view! {
        row(class = "gds-sidebar__section-header", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
