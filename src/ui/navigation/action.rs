use super::*;
use gds_uim::el;
use gds_uim::m;

/// A fixed trailing slot that replaces a passive status with an action on row
/// hover. Neither presentation changes the space available to the row's title.
pub fn hover_action(
    group: SharedString,
    status: AnyElement,
    action: Option<AnyElement>,
) -> AnyElement {
    let has_action = action.is_some();
    el! {
        .div { relative; flex_shrink_0; size: ICON_BUTTON_SIZE.px } [
            .div {
                size_full; flex; items_center; justify_center;
                when: has_action, {
                    group_hover: $(group.clone()), m! { invisible; };
                };
            } [status],
            @if (let Some(action) = action)
                .div {
                    absolute; inset_0; size_full; invisible;
                    group_hover: group, m! { visible; };
                } [action],
            @end,
        ]
    }
    .into_any_element()
}
