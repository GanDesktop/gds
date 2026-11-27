use zgui::prelude::*;
use zgui::{component, view};

/// Flexible page body; children own scrolling and selection surfaces.
#[component]
pub fn PageContent(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_page_stylesheets();
    view! {
        column(class = "gds-page-content", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
