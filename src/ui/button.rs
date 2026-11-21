//! Shared GDS buttons wrapping `zgui-ui::Button` with `36px` / `32x32px` sizing, `1px` `:focus-visible`
//! contours, `ShortcutHint` tooltips, and `140ms` inline / corner hold-to-reveal keytips.

mod icon;
mod menu;
#[cfg(test)]
mod tests;

pub use icon::{IconButton, IconButtonProps};
pub use menu::{ApplicationMenuButton, ApplicationMenuButtonProps};
pub use zgui_ui::{ButtonSize, ButtonVariant};

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Signal};
use zgui::view::{AttrName, current_host};
use zgui::vocab::Toggled;
use zgui::{component, css, view};
use zgui_ui::{
    ButtonProps as ZuiButtonProps, SpinnerProps, TooltipContentProps, TooltipProps,
    TooltipTriggerProps,
};

use crate::app::Gds;
use crate::appearance::tokens::ICON_SIZE;
use crate::ui::actions::Command;
use crate::ui::icon::{IconName, IconProps};
use crate::ui::keytips::KeytipSuffixProps;
use crate::ui::shortcut::install_shortcut_hint_stylesheet;
use crate::ui::tooltip::{TOOLTIP_DELAY, TooltipTextProps};
use crate::ui::{Shortcut, ShortcutBadgeProps};

const SHEET: &str = css!(
    r#"
    .zui-button.gds-button,
    .zui-button.gds-icon-button {
        --zui-button-ring: 0 0 transparent;
        box-shadow: none;
        outline: none;
        font-family: var(--gds-font-sans);
    }

    .zui-button.gds-button:not([data-size="xs"]):not([data-size="sm"]):not([data-size="lg"]):not([data-size^="icon"]) {
        height: 36px;
        gap: 8px;
        font-size: var(--gds-font-size-small, 14px);
    }

    .zui-button.gds-button[data-size="sm"] {
        gap: 6px;
    }

    .zui-button.gds-button[data-size="xs"] {
        gap: 4px;
    }

    .zui-button.gds-icon-button:not([data-size="icon-xs"]):not([data-size="icon-sm"]):not([data-size="icon-lg"]) {
        width: var(--gds-icon-button-size, 32px);
        height: var(--gds-icon-button-size, 32px);
        padding: 0;
        flex-shrink: 0;
    }

    .zui-button.gds-button:not(:disabled):not([data-loading="true"]),
    .zui-button.gds-icon-button:not(:disabled):not([data-loading="true"]) {
        cursor: pointer;
    }

    .zui-button.gds-button:disabled,
    .zui-button.gds-button[data-loading="true"],
    .zui-button.gds-icon-button:disabled,
    .zui-button.gds-icon-button[data-loading="true"] {
        cursor: default;
    }

    .zui-button.gds-button[data-variant="outline"]:not([data-toggled="true"]):not([data-active="true"]),
    .zui-button.gds-icon-button[data-variant="outline"]:not([data-toggled="true"]):not([data-active="true"]) {
        background-color: var(--gds-control);
        color: var(--gds-control-foreground);
        border-color: var(--gds-control-border);
    }

    .zui-button.gds-button[data-variant="outline"]:not([data-toggled="true"]):not([data-active="true"]):hover,
    .zui-button.gds-icon-button[data-variant="outline"]:not([data-toggled="true"]):not([data-active="true"]):hover {
        background-color: var(--gds-control-hover);
        color: var(--gds-control-foreground);
        border-color: var(--gds-control-border);
    }

    .zui-button.gds-button[data-variant="outline"]:not([data-toggled="true"]):not([data-active="true"]):active,
    .zui-button.gds-icon-button[data-variant="outline"]:not([data-toggled="true"]):not([data-active="true"]):active {
        background-color: var(--gds-control-pressed);
        color: var(--gds-control-foreground);
        border-color: var(--gds-control-border);
    }

    .zui-button.gds-button[data-variant="ghost"]:not([data-toggled="true"]):not([data-active="true"]):hover,
    .zui-button.gds-icon-button[data-variant="ghost"]:not([data-toggled="true"]):not([data-active="true"]):hover {
        background-color: var(--gds-hover);
        color: inherit;
    }

    .zui-button.gds-button[data-variant="ghost"]:not([data-toggled="true"]):not([data-active="true"]):active,
    .zui-button.gds-icon-button[data-variant="ghost"]:not([data-toggled="true"]):not([data-active="true"]):active {
        background-color: var(--gds-pressed);
        color: inherit;
    }

    .zui-button.gds-button[data-toggled="true"],
    .zui-button.gds-button[data-active="true"],
    .zui-button.gds-icon-button[data-toggled="true"],
    .zui-button.gds-icon-button[data-active="true"] {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
        border-color: transparent;
    }

    .zui-button.gds-button:focus-visible,
    .zui-button.gds-icon-button:focus-visible {
        --zui-button-ring: 0 0 transparent;
        box-shadow: none;
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .zui-button.gds-button[data-variant="default"]:focus-visible,
    .zui-button.gds-button[data-variant="destructive"]:focus-visible,
    .zui-button.gds-icon-button[data-variant="default"]:focus-visible,
    .zui-button.gds-icon-button[data-variant="destructive"]:focus-visible {
        outline-color: var(--gds-primary-foreground);
    }

    .zui-button.gds-button > .gds-keytip-suffix {
        margin-left: -8px;
    }

    .zui-button.gds-button[data-size="sm"] > .gds-keytip-suffix {
        margin-left: -6px;
    }

    .zui-button.gds-button[data-size="xs"] > .gds-keytip-suffix {
        margin-left: -4px;
    }

    /* Application menu swaps brand mark -> Menu icon strictly on pointer :hover, never :focus-visible. */
    .zui-button.gds-app-menu-button:focus-visible:not(:hover) .gds-app-menu-button__brand {
        opacity: 1;
        transform: scale(1);
    }

    .zui-button.gds-app-menu-button:focus-visible:not(:hover) .gds-app-menu-button__glyph {
        opacity: 0;
        transform: scale(0.85);
    }

    .zui-button.gds-app-menu-button:hover .gds-app-menu-button__brand {
        opacity: 0;
        transform: scale(0.88);
    }

    .zui-button.gds-app-menu-button:hover .gds-app-menu-button__glyph {
        opacity: 1;
        transform: scale(1);
    }
    "#
);

pub(super) fn install_button_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    if current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-button", SHEET);
    }
}

pub(super) fn command_fallback_label(command: Command) -> Signal<String, LocalStorage> {
    Signal::derive_local(move || {
        Gds::try_current().map_or_else(
            || match command {
                Command::FocusNotifications => "Focus notifications".to_owned(),
                Command::CloseNotification => "Close current notification".to_owned(),
                Command::CloseAllNotifications => "Close all notifications".to_owned(),
                Command::Cancel => "Close".to_owned(),
            },
            |gds| gds.i18n().text(command.label_id()),
        )
    })
}

pub(super) fn apply_toggled(attrs: Attrs, toggled: Option<Signal<bool, LocalStorage>>) -> Attrs {
    let Some(toggled) = toggled else {
        return attrs;
    };
    attrs
        .attribute(AttrName::new("data-toggled"), move || {
            toggled.get().then(|| "true".to_owned())
        })
        .a11y_from(A11yBinding::unspecified().toggled(move || {
            if toggled.get() {
                Toggled::True
            } else {
                Toggled::False
            }
        }))
}

pub(super) fn button_attrs(
    base: Attrs,
    toggled: Option<Signal<bool, LocalStorage>>,
    loading: Signal<bool, LocalStorage>,
    active: Signal<bool, LocalStorage>,
    shortcut: Option<&Shortcut>,
) -> Attrs {
    let mut attrs = apply_toggled(base, toggled)
        .attribute(AttrName::new("data-loading"), move || {
            loading.get().then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-active"), move || {
            active.get().then(|| "true".to_owned())
        })
        .a11y_from(A11yBinding::unspecified().busy(move || loading.get()));
    if let Some(key) = shortcut {
        let hint = zgui::vocab::SharedString::from(key.display_tokens().join("+"));
        attrs = attrs.a11y_from(A11yBinding::unspecified().keyboard_shortcut(hint));
    }
    attrs
}

pub(super) fn wrap_tooltip(
    core: AnyView,
    label: Option<Signal<String, LocalStorage>>,
    shortcut: Option<Shortcut>,
    shortcut_enabled: Signal<bool, LocalStorage>,
) -> AnyView {
    if label.is_none() && shortcut.is_none() {
        return core;
    }
    install_shortcut_hint_stylesheet();
    AnyView::new(view! {
        Tooltip(delay = TOOLTIP_DELAY) {
            TooltipTrigger {{core}}
            TooltipContent(class = "gds-tooltip-content", arrow = false) {
                row(class = "gds-shortcut-hint") {
                    {label.map(|text| {
                        view! {
                            box(class = "gds-shortcut-hint__label") {
                                TooltipText(text = text)
                            }
                        }
                        .into_view()
                    })}
                    {{
                        let shortcut = shortcut.clone();
                        move || {
                            shortcut
                                .as_ref()
                                .filter(|_| shortcut_enabled.get())
                                .map(|key| {
                                    view! {
                                        box(class = "gds-shortcut-hint__keys") {
                                            ShortcutBadge(shortcut = key.clone())
                                        }
                                    }
                                    .into_view()
                                })
                        }
                    }}
                }
            }
        }
    })
}

/// Standard `36px` GDS button wrapping `zgui_ui::Button` with optional leading icon, loading
/// spinner, `ShortcutHint` tooltip, and `140ms` width-animated inline keytip suffix.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn Button(
    #[prop(default = ButtonVariant::Default)] variant: ButtonVariant,
    #[prop(default = ButtonSize::Md)] size: ButtonSize,
    #[prop(optional)] icon: Option<IconName>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] loading: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] active: Signal<bool, LocalStorage>,
    #[prop(into, optional)] toggled: Option<Signal<bool, LocalStorage>>,
    #[prop(into, optional)] tooltip: Option<Signal<String, LocalStorage>>,
    #[prop(optional)] command: Option<Command>,
    #[prop(optional)] shortcut: Option<Shortcut>,
    #[prop(into, default = Signal::stored_local(true))] shortcut_enabled: Signal<
        bool,
        LocalStorage,
    >,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_button_stylesheets();
    let resolved_ref = node_ref.unwrap_or_default();
    let effective_disabled = Signal::derive_local(move || disabled.get() || loading.get());
    let effective_shortcut = shortcut.or_else(|| command.map(Shortcut::command));
    let can_advertise =
        Signal::derive_local(move || shortcut_enabled.get() && !effective_disabled.get());

    let advertised_a11y = effective_shortcut
        .as_ref()
        .filter(|_| shortcut_enabled.get_untracked());
    let own = button_attrs(Attrs::new(), toggled, loading, active, advertised_a11y);
    let tip_signal = tooltip.or_else(|| command.map(command_fallback_label));
    let inline_shortcut = effective_shortcut.clone();

    let core = AnyView::new(view! {
        ZuiButton(
            variant = variant,
            size = size,
            disabled = effective_disabled,
            node_ref = Some(resolved_ref),
            class = "gds-button",
            class = class,
            {..own},
            {..attrs}
        ) {
            {move || {
                if loading.get() {
                    Some(AnyView::new(view! { Spinner() }))
                } else {
                    icon.map(|name| AnyView::new(view! { Icon(name = name, size = ICON_SIZE) }))
                }
            }}
            {children.into_view_once()}
            {inline_shortcut.map(|key| {
                view! {
                    KeytipSuffix(shortcut = key, enabled = can_advertise)
                }
                .into_view()
            })}
        }
    });

    if tip_signal.is_some() {
        wrap_tooltip(core, tip_signal, effective_shortcut, can_advertise)
    } else {
        core
    }
}
