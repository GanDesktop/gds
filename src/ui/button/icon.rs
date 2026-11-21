use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Signal};
use zgui::{component, view};
use zgui_ui::{ButtonProps as ZuiButtonProps, ButtonSize, ButtonVariant, SpinnerProps};

use crate::ui::Shortcut;
use crate::ui::actions::Command;
use crate::ui::icon::{IconName, IconProps};
use crate::ui::keytips::anchor_when;

const fn icon_px_for_size(size: ButtonSize) -> f32 {
    match size {
        ButtonSize::IconXs | ButtonSize::Xs => 12.0,
        ButtonSize::IconSm | ButtonSize::Sm => 14.0,
        ButtonSize::IconLg | ButtonSize::Lg => 18.0,
        ButtonSize::Icon | ButtonSize::Md => crate::appearance::tokens::ICON_SIZE,
    }
}

/// Shared `32x32px` icon-only control wrapping `zgui_ui::Button` with accessible name, hover
/// crossfade, loading spinner, `Corner` hold-to-reveal keytip, and `ShortcutHint` tooltip.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn IconButton(
    #[prop(into)] icon: Signal<IconName, LocalStorage>,
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(into, optional)] hover_icon: Option<Signal<IconName, LocalStorage>>,
    #[prop(optional)] icon_size: Option<f32>,
    #[prop(default = ButtonVariant::Ghost)] variant: ButtonVariant,
    #[prop(default = ButtonSize::Icon)] size: ButtonSize,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] loading: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] active: Signal<bool, LocalStorage>,
    #[prop(into, optional)] toggled: Option<Signal<bool, LocalStorage>>,
    #[prop(optional)] command: Option<Command>,
    #[prop(optional)] shortcut: Option<Shortcut>,
    #[prop(into, default = Signal::stored_local(true))] shortcut_enabled: Signal<
        bool,
        LocalStorage,
    >,
    #[prop(default = true)] tooltip: bool,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    super::install_button_stylesheets();
    let resolved_ref = node_ref.unwrap_or_default();
    let effective_disabled = Signal::derive_local(move || disabled.get() || loading.get());
    let effective_shortcut = shortcut.or_else(|| command.map(Shortcut::command));
    let can_advertise =
        Signal::derive_local(move || shortcut_enabled.get() && !effective_disabled.get());

    if let Some(ref key) = effective_shortcut {
        anchor_when(resolved_ref, key.clone(), can_advertise);
    }

    let advertised_a11y = effective_shortcut
        .as_ref()
        .filter(|_| shortcut_enabled.get_untracked());
    let own = super::button_attrs(
        Attrs::new().a11y_from(A11yBinding::unspecified().label(move || label.get())),
        toggled,
        loading,
        active,
        advertised_a11y,
    );
    let glyph_size = icon_size.unwrap_or_else(|| icon_px_for_size(size));

    let core = AnyView::new(view! {
        ZuiButton(
            variant = variant,
            size = size,
            disabled = effective_disabled,
            node_ref = Some(resolved_ref),
            class = "gds-icon-button",
            class = class,
            {..own},
            {..attrs}
        ) {
            {move || {
                if loading.get() {
                    AnyView::new(view! { Spinner() })
                } else if let Some(hover) = hover_icon {
                    AnyView::new(view! {
                        box(class = "gds-icon-button__artwork") {
                            Icon(
                                name = icon,
                                size = glyph_size,
                                class = "gds-icon-button__primary",
                            )
                            Icon(
                                name = hover,
                                size = glyph_size,
                                class = "gds-icon-button__hover",
                            )
                        }
                    })
                } else {
                    AnyView::new(view! { Icon(name = icon, size = glyph_size) })
                }
            }}
        }
    });

    if tooltip {
        super::wrap_tooltip(core, Some(label), effective_shortcut, can_advertise)
    } else {
        core
    }
}
