use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn SideBarSectionTitle(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_sidebar_stylesheets();
    view! {
        text(
            class = "gds-sidebar__section-title",
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}
