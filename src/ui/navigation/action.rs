use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback};
use zgui::{component, view};
use zgui_ui::ButtonSize;

use crate::ui::button::IconButtonProps;
use crate::ui::icon::IconName;

/// Fixed trailing slot replacing a passive status view with a hover action button.
#[component]
pub fn HoverAction(
    status: AnyView,
    #[prop(optional)] action: Option<AnyView>,
    #[prop(into, default = Signal::stored_local(false))] revealed: Signal<bool, LocalStorage>,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    let has_action = action.is_some();
    view! {
        box(
            class = "gds-hover-slot",
            attr:data-revealed = move || revealed.get().then(|| "true".to_owned()),
            attr:data-has-action = has_action.then(|| "true".to_owned()),
            class = class
        ) {
            box(class = "gds-hover-slot__status") {{status}}
            {action.map(|el| view! { box(class = "gds-hover-slot__action") {{el}} })}
        }
    }
}

/// Convenience helper for building a [`HoverAction`] view.
#[must_use]
pub fn hover_action(status: impl IntoView, action: Option<AnyView>) -> AnyView {
    let status = AnyView::new(status);
    AnyView::new(view! { HoverAction(status = status, action = action) })
}

/// Compact close/unpin control placed on collapsed navigation rows.
#[component]
pub fn CornerButton(
    #[prop(into)] icon: Signal<IconName, LocalStorage>,
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    let no_drag = zgui::prelude::try_use_window().map(|w| w.no_drag_handler());
    view! {
        IconButton(
            icon = icon,
            label = label,
            size = ButtonSize::IconXs,
            node_ref = node_ref,
            class = "gds-corner-button",
            class = class,
            on:pointer_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                ev.stop_propagation();
                if let Some(ref handler) = no_drag {
                    handler(ev);
                }
            },
            on:click = |ev: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                ev.stop_propagation();
            },
            {..attrs}
        )
    }
}

/// Convenience builder for a [`CornerButton`] wired to an [`UnsyncCallback`].
#[must_use]
pub fn corner_button(
    icon: IconName,
    label: impl Into<String>,
    on_click: UnsyncCallback<()>,
) -> AnyView {
    let label = label.into();
    AnyView::new(view! {
        CornerButton(
            icon = icon,
            label = label,
            on:click = move |_| on_click.run(())
        )
    })
}

/// Corner overlay container revealed on collapsed row hover/focus or when `revealed` is true.
#[component]
pub fn CornerAction(
    #[prop(into, default = Signal::stored_local(false))] revealed: Signal<bool, LocalStorage>,
    button: AnyView,
) -> impl IntoView {
    super::install_navigation_stylesheets();
    let no_drag = zgui::prelude::try_use_window().map(|w| w.no_drag_handler());
    view! {
        box(
            class = "gds-corner-action",
            attr:data-revealed = move || revealed.get().then(|| "true".to_owned()),
            on:pointer_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                ev.stop_propagation();
                if let Some(ref handler) = no_drag {
                    handler(ev);
                }
            },
            on:click = |ev: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                ev.stop_propagation();
            }
        ) {
            {button}
        }
    }
}

/// Convenience helper for wrapping a button inside [`CornerAction`].
#[must_use]
pub fn corner_action(button: impl IntoView, revealed: Signal<bool, LocalStorage>) -> AnyView {
    let button = AnyView::new(button);
    AnyView::new(view! { CornerAction(revealed = revealed, button = button) })
}
