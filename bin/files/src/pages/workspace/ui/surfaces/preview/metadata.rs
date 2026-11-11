//! Named label/value slots adapted to Kit's description-list item model.
use super::{LONG_VALUE_CHARS, TOOLTIP_DELAY, text_hint};
use gds_uim::{component, el};
use gpui_kit::{
    IntoElement,
    component::{ActiveTheme, description_list::DescriptionItem, tag::Tag},
    prelude::*,
    *,
};

#[component(gpui = gpui_kit)]
#[derive(Default)]
pub(in crate::pages::workspace) struct MetadataRow {
    #[attr(into)]
    label: SharedString,
    #[attr]
    text: SharedString,
    #[attr(any_element, option)]
    value: Option<AnyElement>,
}

impl From<MetadataRow> for DescriptionItem {
    fn from(row: MetadataRow) -> Self {
        let label = el! { .MetadataLabel::default { text: $(row.label.clone()); } };
        let value = row.value.unwrap_or_else(|| {
            el! { .MetadataText::default { label: $(row.label); text: $(row.text); } }
                .into_any_element()
        });
        el! {
            $(label.into_any_element()).DescriptionItem::new {
                value: $(el! { .div { min_w_0; text_xs; overflow_hidden; } [value] }.into_any_element());
            }
        }
    }
}

#[component]
#[derive(Default, IntoElement)]
pub(in crate::pages::workspace) struct MetadataTags {
    #[attr]
    tags: Vec<String>,
}

impl RenderOnce for MetadataTags {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        // Match the surrounding property text instead of the Tag's padded default.
        let line_height = window.text_style().line_height;
        let chips = self.tags.into_iter().enumerate().map(|(index, tag)| {
            let label: SharedString = tag.into();
            el! { .Tag::secondary {
                text_xs; line_height: line_height;
                py_0; px_1p5; border_0; rounded: 3.0.px; max_w_full;
            } [
                .div { # $("detail-tag", index); min_w_0; truncate;
                    tooltip_show_delay: TOOLTIP_DELAY; tooltip: $(text_hint(label.clone()));
                } [label],
            ] }
        });
        el! { .div { flex; flex_wrap; gap_1; min_w_0; } [..chips] }
    }
}

#[component]
#[derive(Default, IntoElement)]
struct MetadataLabel {
    #[attr]
    text: SharedString,
}

impl RenderOnce for MetadataLabel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! { .div { text_xs; text_color: $(cx.theme().muted_foreground); } [$(self.text)] }
    }
}

#[component]
#[derive(Default, IntoElement)]
struct MetadataText {
    #[attr]
    label: SharedString,
    #[attr]
    text: SharedString,
}

impl RenderOnce for MetadataText {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let hint =
            (self.text.chars().count() > LONG_VALUE_CHARS).then(|| text_hint(self.text.clone()));
        el! {
            .div {
                # format!("detail-value:{}", self.label); min_w_0; truncate; text_xs; text_ellipsis;
                @if (let Some(hint) = hint) tooltip_show_delay: TOOLTIP_DELAY; tooltip: hint; @end;
            } [$(self.text)]
        }
    }
}
