mod content;
mod header;
mod title;

pub use content::{SideBarSectionContent, SideBarSectionContentProps};
pub use header::{SideBarSectionHeader, SideBarSectionHeaderProps};
pub use title::{SideBarSectionTitle, SideBarSectionTitleProps};

use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn SideBarSection(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_sidebar_stylesheets();
    view! {
        column(class = "gds-sidebar__section", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
