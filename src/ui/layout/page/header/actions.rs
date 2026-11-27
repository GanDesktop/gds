use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn PageActions(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_page_stylesheets();
    view! {
        row(class = "gds-page-actions", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
