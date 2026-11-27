use zgui::prelude::*;
use zgui::{component, view};

#[component]
pub fn NavItemActions(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    let no_drag = zgui::prelude::try_use_window().map(|w| w.no_drag_handler());
    view! {
        row(
            class = "gds-nav-item__actions",
            class = class,
            on:pointer_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                ev.stop_propagation();
                if let Some(ref handler) = no_drag {
                    handler(ev);
                }
            },
            on:click = |ev: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                ev.stop_propagation();
            },
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}
