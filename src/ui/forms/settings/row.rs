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
    let window = zgui::prelude::try_use_window();
    view! {
        row(
            class = "gds-setting-row",
            attr:data-micro = move || {
                window
                    .as_ref()
                    .is_some_and(|w| {
                        let width = w.size().get().width.0;
                        width > 0.0
                            && crate::ui::window::WidthTier::from_width(width)
                                == crate::ui::window::WidthTier::Micro
                    })
                    .then(|| "true".to_owned())
            },
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}
