use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn NavItemDetails(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    view! {
        text(
            class = "gds-nav-item__details",
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}
