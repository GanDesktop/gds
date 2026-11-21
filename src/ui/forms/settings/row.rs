mod actions;
mod control;
mod description;
mod label;

pub use actions::{SettingActions, SettingActionsProps, SettingReset, SettingResetProps};
pub use control::{SettingControl, SettingControlProps};
pub use description::{SettingDescription, SettingDescriptionProps};
pub use label::{SettingLabel, SettingLabelProps};

use zgui::prelude::*;
use zgui::{component, view};

/// A setting's label column and aligned action controls.
#[component]
pub fn SettingRow(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    super::install_settings_stylesheets();
    view! {
        row(class = "gds-setting-row", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}
