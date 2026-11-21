//! Placeholder policy helper for custom empty-state text hints.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, view};

/// Renders muted placeholder hint text when `visible` is true.
#[component]
pub fn Placeholder(
    #[prop(into)] text: Signal<String, LocalStorage>,
    #[prop(into, default = Signal::stored_local(true))] visible: Signal<bool, LocalStorage>,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    view! {
        if move || visible.get() {
            text(class = "gds-field-placeholder", class = class.clone()) {{move || text.get()}}
        } else {}
    }
}
