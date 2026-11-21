use zgui::prelude::*;
use zgui::{component, view};

/// The flexible label column, optionally containing a description.
#[component]
pub fn SettingLabel(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_settings_stylesheets();
    view! {
        column(class = "gds-setting-label", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
