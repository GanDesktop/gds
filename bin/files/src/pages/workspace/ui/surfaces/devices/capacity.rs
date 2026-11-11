use super::{Browser, storage};
use gds::{i18n, ui::capacity::CapacityBar};
use gds_uim::{component, el};
use gpui_kit::{IntoElement, prelude::*, *};

#[component]
#[derive(Default, IntoElement)]
pub(in crate::pages::workspace) struct DeviceCapacity {
    #[attr(into)]
    id: SharedString,
    #[attr]
    state: Option<storage::CapacityState>,
    #[attr]
    compact: bool,
    #[attr]
    selected: bool,
    #[attr]
    embedded: bool,
}

impl RenderOnce for DeviceCapacity {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let label = match &self.state {
            Some(storage::CapacityState::Ready(capacity)) => Browser::capacity_label(*capacity, cx),
            Some(storage::CapacityState::Unavailable(_)) => {
                i18n::text("device-capacity-unavailable", cx)
            }
            _ => i18n::text("device-capacity-loading", cx),
        };
        el! {
            $(self.id).CapacityBar::new {
                label: label; compact: $(self.compact); selected: $(self.selected);
                @if (self.embedded) embedded; @end;
                @if (let Some(storage::CapacityState::Ready(capacity)) = &self.state)
                    value: $(capacity.unavailable_fraction());
                @end;
                @if (matches!(self.state, Some(storage::CapacityState::Unavailable(_)))) unavailable;
                @end;
            }
        }
    }
}
