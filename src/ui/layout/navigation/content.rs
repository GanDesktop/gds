use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn NavItemContent(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    view! {
        column(class = "gds-nav-item__content", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
