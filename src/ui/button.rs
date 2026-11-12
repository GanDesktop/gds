//! Shared buttons, fixed action-icon sizing and control cursor policy.

mod icon;
mod menu;
#[cfg(all(test, feature = "gui-tests"))]
mod tests;
pub use icon::IconButton;

use std::rc::Rc;

use crate::app::AppExt as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, CursorStyle, ElementId, InteractiveElement, Interactivity,
    IntoElement, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement,
    StyleRefinement, Styled, Window,
    assets::IconName,
    component::{
        Disableable, FocusableExt, Icon, Selectable, Sizable, Size,
        button::{Button as ComponentButton, ButtonCustomVariant, ButtonVariant, ButtonVariants},
        spinner::Spinner,
    },
    px,
};

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    inner: ComponentButton,
    disabled: bool,
    loading: bool,
    control_icon: Option<IconName>,
    children: Vec<AnyElement>,
    variant: ButtonVariant,
    tooltip: Option<Rc<dyn Fn(&mut Window, &mut App) -> gpui_kit::AnyView>>,
    shortcut: Option<super::Shortcut>,
    label: Option<SharedString>,
    explicit_name: bool,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            inner: ComponentButton::new(id.clone())
                .text_size(crate::appearance::tokens::SMALL)
                .line_height(gpui_kit::relative(1.)),
            disabled: false,
            loading: false,
            control_icon: None,
            children: Vec::new(),
            variant: ButtonVariant::Default,
            tooltip: None,
            id,
            shortcut: None,
            label: None,
            explicit_name: false,
        }
    }

    /// The shared toolbar control: a 32px target with a 16px action icon.
    pub fn icon_button(id: impl Into<ElementId>) -> Self {
        Self::new(id)
            .ghost()
            .small()
            .w(px(crate::appearance::tokens::ICON_BUTTON_SIZE))
            .h(px(crate::appearance::tokens::ICON_BUTTON_SIZE))
            .p_0()
            .flex_shrink_0()
    }

    /// Render an action icon at the suite size, independently of button density.
    pub fn control_icon(mut self, icon: IconName) -> Self {
        self.control_icon = Some(icon);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.inner = self.inner.icon(icon.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.explicit_name = true;
        self.inner = self.inner.accessibility_label(label);
        self
    }

    pub fn tooltip(self, tooltip: impl Into<SharedString>) -> Self {
        let label = tooltip.into();
        self.hoverable_tooltip(move |window, cx| super::tooltip::view(label.clone(), window, cx))
    }

    /// Register this control with the same held-modifier layer as `IconButton`.
    pub fn shortcut(mut self, shortcut: impl Into<super::Shortcut>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// One action supplies both its hover keycaps and held-modifier hint.
    pub fn tooltip_with_shortcut(
        self,
        label: impl Into<SharedString>,
        shortcut: impl Into<super::Shortcut>,
    ) -> Self {
        let shortcut = shortcut.into();
        self.shortcut(shortcut.clone())
            .hoverable_tooltip(super::ShortcutHint::new(label, shortcut).builder())
    }

    pub fn tooltip_with_action(
        self,
        tooltip: impl Into<SharedString>,
        action: &dyn gpui_kit::Action,
        context: Option<&str>,
    ) -> Self {
        let label = tooltip.into();
        let action = action.boxed_clone();
        let context = context
            .map(|context| {
                gpui_kit::KeyContext::parse(context).expect("bundled action contexts are valid")
            })
            .unwrap_or_default();
        self.hoverable_tooltip(move |window, cx| {
            let hint = match window
                .highest_precedence_binding_for_action_in_context(&*action, context.clone())
            {
                Some(binding) => super::ShortcutHint::binding(label.clone(), binding),
                None => super::ShortcutHint::plain(label.clone()),
            };
            hint.builder()(window, cx)
        })
    }

    pub fn compact(mut self) -> Self {
        self.inner = self.inner.compact();
        self
    }

    pub fn dropdown_caret(mut self, show: bool) -> Self {
        self.inner = self.inner.dropdown_caret(show);
        self
    }

    pub fn toggled(mut self, toggled: bool) -> Self {
        self.inner = self.inner.toggled(toggled);
        self
    }

    pub fn hoverable_tooltip(
        mut self,
        tooltip: impl Fn(&mut Window, &mut App) -> gpui_kit::AnyView + 'static,
    ) -> Self {
        self.tooltip = Some(Rc::new(tooltip));
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.inner = self.inner.on_click(handler);
        self
    }

    fn into_component(mut self, window: &mut Window, cx: &mut App) -> ComponentButton {
        if let Some(tooltip) = self.tooltip.take() {
            self.inner
                .interactivity()
                .hoverable_tooltip(move |window, cx| tooltip(window, cx));
        }
        let shortcut = self
            .shortcut
            .take()
            .filter(|_| !self.disabled && !self.loading);
        if let Some(label) = self.label.take() {
            if let Some(shortcut) = shortcut {
                use gds_uim::el;
                use gpui_kit::div;
                if !self.explicit_name {
                    self.inner = self.inner.accessibility_label(label.clone());
                }
                let hint = super::keytips::suffix_anchor(window, cx, self.id, shortcut);
                self.inner = self.inner.child(el! {
                    .div { flex; items_center; min_w_0; } [
                        .div { min_w_0; whitespace_nowrap; text_ellipsis; } [label],
                        hint,
                    ]
                });
            } else {
                self.inner = self.inner.label(label);
            }
        } else if let Some(shortcut) = shortcut {
            self.inner = self
                .inner
                .relative()
                .child(super::keytips::anchor(window, cx, self.id, shortcut));
        }
        if self.variant == ButtonVariant::Ghost {
            let colors = cx.gds_ro().appearance().theme().colors();
            let selected = self.inner.is_selected();
            let variant = ButtonCustomVariant::new(cx)
                .foreground(if selected {
                    colors.selection_foreground
                } else {
                    colors.foreground
                })
                .hover(colors.hover())
                .active(if selected {
                    colors.selection
                } else {
                    colors.pressed()
                });
            self.inner = self.inner.custom(variant);
        }
        if let Some(icon) = self.control_icon {
            // Kit resizes icons supplied through `.icon()` to the button's
            // density. A content icon retains the shared action-icon size.
            let artwork = if self.loading {
                Spinner::new()
                    .with_size(px(crate::appearance::tokens::ICON_SIZE))
                    .into_any_element()
            } else {
                Icon::new(icon)
                    .size(px(crate::appearance::tokens::ICON_SIZE))
                    .into_any_element()
            };
            self.inner = self.inner.child(artwork);
        }
        self.inner.extend(self.children);
        // Explicit styles (e.g. passive status icons) win only while enabled.
        let cursor = if self.disabled || self.loading {
            CursorStyle::Arrow
        } else {
            self.inner
                .style()
                .mouse_cursor
                .unwrap_or(CursorStyle::PointingHand)
        };
        self.inner
            .disabled(self.disabled)
            .loading(self.loading)
            // Pointer reuse must not leave a previous keyboard cursor behind.
            // Kit retains the focus handle; only its decoration follows modality.
            .focus_ring(window.last_input_was_keyboard())
            .cursor(cursor)
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Selectable for Button {
    fn selected(mut self, selected: bool) -> Self {
        self.inner = self.inner.selected(selected);
        self
    }

    fn is_selected(&self) -> bool {
        self.inner.is_selected()
    }
}

impl Sizable for Button {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.inner = self.inner.with_size(size);
        self
    }
}

impl ButtonVariants for Button {
    fn with_variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self.inner = self.inner.with_variant(variant);
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl gpui_kit::component::menu::DropdownMenu for Button {}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.into_component(window, cx).render(window, cx)
    }
}
