use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn PageTableArea(
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_page_stylesheets();
    view! {
        box(
            class = "gds-page-table-area",
            node_ref = node_ref.unwrap_or_default(),
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}
