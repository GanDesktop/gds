use zgui::prelude::*;
use zgui::{component, view};

/// Secondary setting text; validation and persistence remain with the caller.
#[component]
pub fn SettingDescription(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_settings_stylesheets();
    view! {
        text(class = "gds-setting-description", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
