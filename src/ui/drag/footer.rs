use zgui::prelude::*;
use zgui::{component, view};

use super::install_drag_stylesheet;

/// Footer row displayed at the bottom of a floating [`super::DragCard`].
#[component]
pub fn DragCardFooter(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_drag_stylesheet();
    view! {
        row(class = "gds-drag-card__footer", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
