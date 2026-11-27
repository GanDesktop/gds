use zgui::prelude::*;
use zgui::{component, view};

use super::install_drag_stylesheet;

/// Body container displayed inside a floating [`super::DragCard`].
#[component]
pub fn DragCardContent(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_drag_stylesheet();
    view! {
        column(class = "gds-drag-card__content", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
