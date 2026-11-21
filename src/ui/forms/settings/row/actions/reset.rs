use super::super::super::RESET_WIDTH;
use zgui::prelude::*;
use zgui::{component, view};

/// Reserves the reset target's space even when no action is shown.
#[component]
pub fn SettingReset(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    super::super::super::install_settings_stylesheets();
    let size = format!("{RESET_WIDTH}px");
    view! {
        box(class = "gds-setting-reset", var:--gds-setting-reset-size = size, class = class, {..attrs}) {
            {children.map(Children::into_view_once)}
        }
    }
}
