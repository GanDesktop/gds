use super::*;
use gds_uim::el;

const PIN_BADGE_SIZE: f32 = 12.;

/// A row's own icon, marked with a corner badge when the row is pinned.
///
/// The icon keeps its identity and its shared size whether or not the row is
/// pinned: replacing it with a pin would drop what the row points at, and
/// resizing it would shift the row's label between the two states.
pub fn pinned_icon(icon: IconName, pinned: bool, cx: &App) -> AnyElement {
    with_pin(Icon::new(icon).size(px(SIDEBAR_ICON_SIZE)), pinned, cx)
}

/// Apply the same corner marker to an application-provided navigation icon.
pub fn with_pin(icon: impl IntoElement, pinned: bool, _cx: &App) -> AnyElement {
    el! {
        .div { relative; flex_shrink_0; size: SIDEBAR_ICON_SIZE.px } [
            icon,
            @if (pinned)
                .div {
                    absolute; left: (-6.0).px; top: (-6.0).px;
                    flex; items_center; justify_center;
                    size: PIN_BADGE_SIZE.px;
                } [
                    IconName::Pin.Icon::new { size: PIN_BADGE_SIZE.px; },
                ],
            @end,
        ]
    }
    .into_any_element()
}
