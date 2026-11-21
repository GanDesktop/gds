use zgui::prelude::*;
use zgui::{component, view};

use super::install_drag_stylesheet;

/// Header row displayed at the top of a floating [`super::DragCard`].
#[component]
pub fn DragCardHeader(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_drag_stylesheet();
    view! {
        row(class = "gds-drag-card__header", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
