//! Standard padded content container.

use zgui::prelude::*;
use zgui::{component, view};

/// Padded flex column filling remaining workspace space.
#[component]
pub fn ContentContainer(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    view! {
        column(class = "gds-content-container", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
