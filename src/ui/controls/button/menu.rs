use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Signal};
use zgui::view::PropValue;
use zgui::{component, view};
use zgui_ui::{ButtonProps as ZuiButtonProps, ButtonSize, ButtonVariant};

use crate::ui::Shortcut;
use crate::ui::actions::Command;
use crate::ui::icon::{IconName, IconProps};
use crate::ui::keytips::anchor_when;
use crate::ui::metadata::Application;

/// Leading `32x32px` toolbar button that anchors the primary application menu.
///
/// Displays the application's `24px` monochrome brand mark at rest and swaps to a centered `16px`
/// `Menu` icon strictly on pointer `:hover` (never on `:focus-visible`, `README.md` §3).
#[allow(clippy::too_many_arguments)]
#[component]
pub fn ApplicationMenuButton(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(optional)] application: Option<&'static Application>,
    #[prop(optional)] command: Option<Command>,
    #[prop(optional)] shortcut: Option<Shortcut>,
    #[prop(into, default = Signal::stored_local(true))] shortcut_enabled: Signal<
        bool,
        LocalStorage,
    >,
    #[prop(into, default = Signal::stored_local(false))] active: Signal<bool, LocalStorage>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    super::install_button_stylesheets();
    let resolved_ref = node_ref.unwrap_or_default();
    let effective_shortcut = shortcut.or_else(|| command.map(Shortcut::command));

    if let Some(ref key) = effective_shortcut {
        anchor_when(resolved_ref, key.clone(), shortcut_enabled);
    }

    let advertised_a11y = effective_shortcut
        .as_ref()
        .filter(|_| shortcut_enabled.get_untracked());
    let own = super::button_attrs(
        Attrs::new().a11y_from(A11yBinding::unspecified().label(move || label.get())),
        None,
        Signal::stored_local(false),
        active,
        advertised_a11y,
    );

    let brand_svg = application.and_then(Application::artwork_svg);

    let core = AnyView::new(view! {
        ZuiButton(
            variant = ButtonVariant::Ghost,
            size = ButtonSize::Icon,
            node_ref = Some(resolved_ref),
            class = "gds-icon-button gds-app-menu-button",
            class = class,
            {..own},
            {..attrs}
        ) {
            box(class = "gds-app-menu-button__artwork") {
                {match brand_svg {
                    Some(markup) => AnyView::new(view! {
                        vector(
                            class = "gds-app-menu-button__brand",
                            prop:svg = PropValue::from(markup),
                        )
                    }),
                    None => AnyView::new(view! {
                        Icon(
                            name = IconName::Menu,
                            size = crate::appearance::tokens::ICON_SIZE,
                            class = "gds-app-menu-button__brand",
                        )
                    }),
                }}
                Icon(
                    name = IconName::Menu,
                    size = crate::appearance::tokens::ICON_SIZE,
                    class = "gds-app-menu-button__glyph",
                )
            }
        }
    });

    super::wrap_tooltip(core, Some(label), effective_shortcut, shortcut_enabled)
}
