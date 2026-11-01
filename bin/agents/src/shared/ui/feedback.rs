use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::{
    App, ElementId, Hsla, IntoElement, RenderOnce, SharedString, TestSupportExt, Window, div,
    prelude::*,
};

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

pub(crate) fn copy_button(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> gpui_kit::Div {
    use gpui_kit::component::{Sizable as _, Size, clipboard::Clipboard};
    let label = tr!("feedback-copy");
    div()
        .size(gpui_kit::px(super::CONTROL))
        .flex_shrink_0()
        .child(
            Clipboard::new(id)
                .value(text)
                .with_size(Size::Size(gpui_kit::px(super::CONTROL)))
                .accessibility_label(label)
                .tooltip(label)
                .tooltip_show_delay(gds::appearance::tokens::TOOLTIP_DELAY),
        )
}

pub(crate) fn message(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    color: Hsla,
) -> Feedback {
    Feedback {
        id: id.into(),
        text: text.into(),
        color,
        compact: false,
        copyable: true,
    }
}

#[derive(IntoElement)]
pub(crate) struct Feedback {
    id: SharedString,
    text: SharedString,
    color: Hsla,
    compact: bool,
    copyable: bool,
}

impl Feedback {
    pub(crate) fn without_copy(mut self) -> Self {
        self.copyable = false;
        self
    }

    pub(crate) fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl RenderOnce for Feedback {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let hint = self.text.clone();
        let display = if self.compact {
            self.text.replace('\n', " ")
        } else {
            super::tooltip::wrap_tokens(&self.text)
        };
        el! {
            .div {
                # $(self.id.clone());
                test_support;
                role: gpui_kit::Role::Status;
                aria_label: $(self.text.clone());
                flex;
                items_start;
                gap_2;
                min_w_0;
                w_full;
                text_size: super::SMALL;
                min_h: $(gpui_kit::px(super::CONTROL));
                text_color: $(self.color);
                when: $(self.compact), {
                    items_center;
                };
            } [
                .div {
                    # format!("{}-message", self.id);
                    test_support;
                    role: gpui_kit::Role::Label;
                    aria_label: $(self.text.clone());
                    flex_1;
                    min_w_0;
                    when: $(self.compact), {
                        truncate;
                    };
                    hoverable_tooltip: $(move |window, cx| super::tooltip::view(hint.clone(), window, cx));
                } [
                    display,
                ],
                ..$(self.copyable.then(|| copy_button(format!("{}-copy", self.id), self.text).flex_shrink_0())),
            ]
        }
    }
}
