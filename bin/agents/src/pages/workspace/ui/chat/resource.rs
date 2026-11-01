//! Open supported resource references without interpreting Agent URIs as commands.

use super::super::*;
use crate::shared::i18n::tr;
use gds_uim::el;
use gds_uim::m;
use gpui_kit::component::attachment::{
    Attachment, AttachmentActions, AttachmentContent, AttachmentDescription, AttachmentMedia,
    AttachmentTitle,
};

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

pub(super) fn render(
    id: &str,
    resource: &bas::acp::ResourceLink,
    links: &links::Context,
    cx: &App,
) -> AnyElement {
    let target = links.target(&resource.uri).ok();
    let hint = if target.is_some() {
        tr!("resource-open")
    } else {
        tr!("resource-open-unavailable")
    };
    let uri = resource.uri.clone();
    let group = SharedString::from(format!("resource-{id}"));
    let description = el! {
        .AttachmentContent::default { gap_0 } [
            .div {
                # format!("resource-name-{id}");
                test_support;
            } [
                $(resource.name.clone()).AttachmentTitle::new { text_size: crate::shared::ui::SMALL; font_weight: FontWeight::NORMAL },
            ],
            .div {
                # format!("resource-uri-{id}");
                test_support;
            } [$(uri.clone()).AttachmentDescription::new { text_size: crate::shared::ui::SMALL }],
        ]
    };
    el! {
        .div {
            # $(group.clone());
            test_support;
            group: $(group.clone());
            min_w_0;
        } [
            .Attachment::default {
                w_full; h_auto; p_3; gap_2; bg: $(gpui_kit::rgba(0));
                media: .AttachmentMedia::default { size: crate::shared::ui::ICON.px } [
                    IconName::File.Icon::new { size: crate::shared::ui::ICON.px; text_color: $(cx.theme().muted_foreground) },
                ];
                content: description;
                actions: .AttachmentActions::default [
                    (format!("open-resource-{id}"), IconName::ExternalLink, cx).crate::shared::ui::subtle_icon_button {
                        disabled: $(target.is_none());
                        tooltip: hint;
                        accessibility_label: tr!("resource-open-name", name = resource.name.clone());
                        on_click: $(move |_, _, cx| { if let Some(target) = &target { target.open(cx); } });
                    },
                    .div {
                        invisible;
                        group_hover: group, m! {visible;};
                    } [
                        $(SharedString::from(id.to_owned())).gpui_kit::component::clipboard::Clipboard::new {
                            value: uri;
                            tooltip: tr!("action-copy-resource");
                            accessibility_label: tr!("action-copy-resource");
                            with_size: $(gpui_kit::component::Size::Size(px(crate::shared::ui::CONTROL)));
                            tooltip_show_delay: gds::appearance::tokens::TOOLTIP_DELAY;
                        },
                    ],
                ];
            },
        ]
    }.into_any_element()
}
