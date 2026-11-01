use crate::shared::i18n::tr;
use gds_uim::el;
use gds_uim::m;
use gpui_kit::base::text::{CodeBlock, TableData};
use gpui_kit::{AvailableSpace, canvas};

use super::*;

const CODE_GROUP: &str = "markdown-code-block";
const TABLE_GROUP: &str = "markdown-table-actions";

pub(super) fn code(block: &CodeBlock, _: &mut Window, _: &mut App) -> AnyElement {
    let source = block.code().to_string();
    let start = block.span.map_or(0, |span| span.start);
    // The code style owns an overflow clip, so this mask is the visible part
    // of this code block, including its padding. Keep the native text tree
    // intact: replacing blocks with nested TextViews breaks range selection.
    el! {
        $(canvas(
                move |anchor, window, cx| {
                    let region = window.content_mask().bounds;
                    let mut actions = el! {.div {
                            # $(CODE_GROUP, start);
                            test_support;
                            group: CODE_GROUP;
                            w: $(region.size.width);
                            h: $(region.size.height);
                            relative;
                        } [
                            $(copy_button(
                                    "copy-markdown-code",
                                    tr!("action-copy-code"),
                                    source,
                                    cx,
                            )).m!{absolute;
                                left: $(anchor.left() - region.left() - px(crate::shared::ui::COMPACT_CONTROL));
                                top: $(anchor.top() - region.top());
                                invisible;
                                group_hover: CODE_GROUP, m! {visible;};},
                    ]}
                    .into_any_element();
                    actions.prepaint_as_root(
                        region.origin,
                        region.size.map(AvailableSpace::Definite),
                        window,
                        cx,
                    );
                    actions
                },
                |_, mut actions, window, cx| actions.paint(window, cx),
        )) {
            size: $(px(0.));
        }
    }
    .into_any_element()
}

pub(super) fn table(table: &TableData, _: &mut Window, cx: &mut App) -> AnyElement {
    el! {
        .div {
            # TABLE_GROUP;
            test_support;
            group: TABLE_GROUP;
            flex;
            justify_end;
        } [
            $(copy_button(
                    "copy-markdown-table",
                    tr!("action-copy-table"),
                    table.markdown.clone(),
                    cx,
            )).m!{invisible;
                group_hover: TABLE_GROUP, m! {visible;};},
        ]
    }
    .into_any_element()
}

fn copy_button(
    id: &'static str,
    label: &'static str,
    source: String,
    _: &App,
) -> gpui_kit::Stateful<Div> {
    el! {
        .div { # format!("{id}-surface"); size: crate::shared::ui::CONTROL.px; flex_shrink_0 } [
            id.gpui_kit::component::clipboard::Clipboard::new {
                value: source;
                accessibility_label: label;
                tooltip: label;
                tooltip_show_delay: gds::appearance::tokens::TOOLTIP_DELAY;
                with_size: $(gpui_kit::component::Size::Size(px(crate::shared::ui::CONTROL)));
            },
        ]
    }
}
