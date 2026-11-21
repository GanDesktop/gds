use super::super::CONTROL_WIDTH;
use zgui::prelude::*;
use zgui::{component, view};

/// The shared control column, keeping controls aligned between rows.
#[component]
pub fn SettingControl(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_settings_stylesheets();
    let width = format!("{CONTROL_WIDTH}px");
    view! {
        box(class = "gds-setting-control", var:--gds-setting-control-width = width, class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
