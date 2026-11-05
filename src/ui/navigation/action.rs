use super::*;
use gds_uim::el;
use gds_uim::m;
use gpui_kit::component::Sizable;

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

/// A small close/unpin control for a collapsed navigation row.
pub fn corner_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .small()
        .size(px(16.))
        .p_0()
        .icon(Icon::new(icon).size(px(11.)))
        .accessibility_label(label.clone())
        .tooltip(label)
}

/// Overlay the row's top trailing corner without widening its icon column.
pub fn corner_action(group: SharedString, button: Button, revealed: bool) -> AnyElement {
    el! {
        .div {
            absolute; top: 1.0.px; right: 1.0.px;
            when: $(!revealed), {
                invisible;
                group_hover: group, m! { visible; };
            };
            on_mouse_down: MouseButton::Left, $(|_, _, cx| cx.stop_propagation());
        } [button]
    }
    .into_any_element()
}
