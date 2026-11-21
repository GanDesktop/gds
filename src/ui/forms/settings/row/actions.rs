mod reset;

pub use reset::{SettingReset, SettingResetProps};

use super::super::CONTROL_GAP;
use zgui::prelude::*;
use zgui::{component, view};

/// The row's control group, including optional reset actions.
#[component]
pub fn SettingActions(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::super::install_settings_stylesheets();
    let gap = format!("{CONTROL_GAP}px");
    view! {
        row(class = "gds-setting-actions", var:--gds-setting-gap = gap, class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
