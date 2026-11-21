use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, view};

use crate::appearance::tokens::{SIDEBAR_ICON_SIZE, SMALL_ICON_SIZE};
use crate::ui::icon::{IconName, IconProps};

/// Navigation icon paired with a corner pin badge when `pinned` is true.
#[component]
pub fn PinnedIcon(
    #[prop(into)] icon: Signal<IconName, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] pinned: Signal<bool, LocalStorage>,
) -> impl IntoView {
    view! {
        WithPin(
            icon = AnyView::new(view! { Icon(name = icon, size = SIDEBAR_ICON_SIZE) }),
            pinned = pinned
        )
    }
}

/// Convenience helper returning a [`PinnedIcon`] view.
#[must_use]
pub fn pinned_icon(icon: IconName, pinned: bool) -> AnyView {
    AnyView::new(view! { PinnedIcon(icon = icon, pinned = pinned) })
}

/// Wraps an arbitrary navigation icon view with an optional corner pin marker.
#[component]
pub fn WithPin(
    icon: AnyView,
    #[prop(into, default = Signal::stored_local(false))] pinned: Signal<bool, LocalStorage>,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    view! {
        box(class = "gds-pinned-icon") {
            {icon}
            if move || pinned.get() {
                box(class = "gds-pinned-icon__badge") {
                    Icon(name = IconName::Pin, size = SMALL_ICON_SIZE)
                }
            } else {}
        }
    }
}

/// Convenience helper wrapping an icon element with [`WithPin`].
#[must_use]
pub fn with_pin(icon: impl IntoView, pinned: bool) -> AnyView {
    let icon = AnyView::new(icon);
    AnyView::new(view! { WithPin(icon = icon, pinned = pinned) })
}
