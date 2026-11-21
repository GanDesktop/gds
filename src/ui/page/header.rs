mod actions;
mod title;

pub use actions::{PageActions, PageActionsProps};
pub use title::{PageTitle, PageTitleProps};

use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn PageHeader(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_page_stylesheets();
    view! {
        row(class = "gds-page-header", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
