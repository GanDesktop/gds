use zgui::prelude::*;
use zgui::view::CustomPropertyName;
use zgui::{component, view};

use super::NavItemContext;

#[component]
pub fn NavItemHeader(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    let reserve_attrs = NavItemContext::current()
        .filter(|c| c.trailing_reserve > 0.0)
        .map_or_else(Attrs::new, |c| {
            Attrs::new().custom_property(
                CustomPropertyName::new("gds-nav-header-reserve"),
                format!("{:.2}px", c.trailing_reserve),
            )
        });
    view! {
        row(
            class = "gds-nav-item__header",
            {..reserve_attrs},
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}
