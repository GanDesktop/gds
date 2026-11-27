//! Directory filter input and micro-tier footer/floating filter controls.

use gds::i18n;
use gds::ui::IconName;
use gds::ui::button::{ButtonSize, IconButtonProps};
use gds::ui::field::InputFieldProps;
use gds::ui::interaction::keytips::{self, HintPlacement};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, on_cleanup_local};
use zgui::view::install_stylesheet as install_sheet;
use zgui::{css, view};
use zgui_ui_primitives::Binding;

use super::{Chrome, install_chrome_stylesheets, node_contains_focus, track_language};
use crate::pages::workspace::{
    navigation::Navigator, projection::UiState, state::context::FilterFocusMode,
};
use crate::shared::ui::actions::Command;

const FILTER_SHEET: &str = "gds-files-chrome-filter";

const FILTER_CSS: &str = css!(
    r#"
    .gds-files-filter-wrap.gds-files-filter-wrap {
        display: flex;
        width: 200px;
        min-width: 96px;
        flex-shrink: 1;
    }

    .gds-files-filter-field.gds-files-filter-field {
        width: 100%;
        min-width: 0px;
    }

    .gds-files-micro-filter.gds-files-micro-filter {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        min-width: 0px;
        flex: 0 1 auto;
    }

    .gds-files-micro-filter.gds-files-micro-filter[data-floating="true"] {
        position: absolute;
        left: 12px;
        bottom: 10px;
        z-index: 6;
        padding: 3px;
        border-radius: var(--gds-radius-control, 6px);
        background-color: var(--gds-panel);
        border: 1px solid var(--gds-border);
    }

    .gds-files-micro-filter-input-wrap.gds-files-micro-filter-input-wrap {
        display: flex;
        width: 184px;
        min-width: 88px;
        max-width: 220px;
        flex: 0 1 auto;
        transition: width var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-input-field.gds-input-field.gds-files-micro-filter-field {
        width: 100%;
        min-width: 0px;
        height: 24px;
        min-height: 24px;
        border-radius: 5px;
    }

    .gds-input-field.gds-files-micro-filter-field .gds-input-field__leading {
        padding-left: 7px;
    }

    .gds-input-field.gds-files-micro-filter-field .gds-input-field__trailing {
        padding-right: 2px;
    }

    .gds-input-field.gds-files-micro-filter-field .gds-input-field__clear {
        width: 18px;
        height: 18px;
    }

    .gds-input-field.gds-files-micro-filter-field .zui-field.gds-input-field__input {
        font-size: var(--gds-text-xs, 12px);
        padding: 0px 6px;
    }
    "#
);

pub(super) fn install_stylesheet() {
    install_sheet(FILTER_SHEET, FILTER_CSS);
}

impl Chrome<'_> {
    pub(in crate::pages::workspace) fn folder_filter(&self) -> AnyView {
        install_chrome_stylesheets();
        let filter_sig = self.0.0.filter;
        let filter_node = self.0.0.filter_node;
        let focus_req = self.0.0.filter_focus_request;
        let available_ctx = self.0.clone();
        let change_ctx = self.0.clone();
        let wrap_ref = NodeRef::new();

        let visible = Signal::derive_local(move || UiState(&available_ctx).filter_available());
        keytips::stage_shortcut(
            wrap_ref,
            Command::Filter.shortcut(),
            HintPlacement::Trailing,
            visible,
        );

        AnyView::new(view! {
            Show(when = move || visible.get()) {
                box(node_ref = wrap_ref, class = "gds-files-filter-wrap") {
                    {{
                        attach_filter_focus_effect(filter_node, filter_sig, focus_req);
                        let change_ctx = change_ctx.clone();
                        move || {
                            track_language();
                            let change_c = change_ctx.clone();
                            view! {
                                InputField(
                                    node_ref = filter_node,
                                    value = Binding::from(filter_sig),
                                    placeholder = i18n::text("filter-placeholder"),
                                    label = i18n::text("filter"),
                                    leading_icon = IconName::Search,
                                    clearable = true,
                                    compact = true,
                                    class = "gds-files-filter-field",
                                    on_change = UnsyncCallback::new(move |value: String| {
                                        Navigator(&change_c).filter(value);
                                    })
                                )
                            }
                        }
                    }}
                }
            }
        })
    }

    /// Subordinate `24px` directory filter used in `HeightTier::Micro` (`H < 520px`), mounted
    /// either inside the `32px` footer's status slot (`floating = false`) or as a bottom-left
    /// floating pill when Save-picker mode occupies the footer status slot (`floating = true`).
    pub(in crate::pages::workspace) fn micro_filter(&self, floating: bool) -> AnyView {
        install_chrome_stylesheets();
        let filter_sig = self.0.0.filter;
        let filter_node = self.0.0.filter_node;
        let footer_open = self.0.0.footer_filter_open;
        let focus_req = self.0.0.filter_focus_request;
        let open_ctx = self.0.clone();
        let change_ctx = self.0.clone();
        let clear_ctx = self.0.clone();
        let expanded =
            Signal::derive_local(move || footer_open.get() || !filter_sig.get().is_empty());

        AnyView::new(view! {
            row(
                class = "gds-files-micro-filter",
                attr:data-floating = floating.then(|| "true".to_owned()),
            ) {
                {{move || {
                    if expanded.get() {
                        let wrap_ref = NodeRef::new();
                        keytips::stage_shortcut(
                            wrap_ref,
                            Command::Filter.shortcut(),
                            HintPlacement::Trailing,
                            Signal::stored_local(true),
                        );
                        attach_filter_focus_effect(filter_node, filter_sig, focus_req);
                        let cc = change_ctx.clone();
                        let clc = clear_ctx.clone();
                        AnyView::new(view! {
                            box(node_ref = wrap_ref, class = "gds-files-micro-filter-input-wrap") {
                                {{move || {
                                    track_language();
                                    let change_c = cc.clone();
                                    let clear_c = clc.clone();
                                    view! {
                                        InputField(
                                            node_ref = filter_node,
                                            value = Binding::from(filter_sig),
                                            placeholder = i18n::text("filter-placeholder"),
                                            label = i18n::text("filter"),
                                            leading_icon = IconName::Search,
                                            clearable = true,
                                            class = "gds-files-micro-filter-field",
                                            on_change = UnsyncCallback::new(move |value: String| {
                                                Navigator(&change_c).filter(value);
                                            }),
                                            on_clear = UnsyncCallback::new(move |()| {
                                                Navigator(&clear_c).filter(String::new());
                                                clear_c.0.footer_filter_open.set(false);
                                                clear_c.focus_content();
                                            }),
                                            on:focus_out = move |_| {
                                                if filter_sig.get_untracked().is_empty()
                                                    && !node_contains_focus(filter_node)
                                                {
                                                    footer_open.set(false);
                                                }
                                            },
                                        )
                                    }
                                }}}
                            }
                        })
                    } else {
                        let trigger_c = open_ctx.clone();
                        AnyView::new(view! {
                            IconButton(
                                icon = IconName::Search,
                                size = ButtonSize::IconXs,
                                label = Signal::derive_local(|| i18n::text("filter")),
                                shortcut = Some(Command::Filter.shortcut()),
                                on:click = move |_| UiState(&trigger_c).activate_filter(),
                            )
                        })
                    }
                }}}
            }
        })
    }
}

fn attach_filter_focus_effect(
    filter_node: NodeRef,
    filter_sig: RwSignal<String, LocalStorage>,
    focus_request: RwSignal<Option<FilterFocusMode>, LocalStorage>,
) {
    let focus_fx = RenderEffect::new(move |_| {
        let Some(mode) = focus_request.get() else {
            return;
        };
        if filter_node.get().is_none() {
            return;
        }
        focus_request.set(None);
        let current = filter_sig.get_untracked();
        filter_node.focus();
        filter_node.set_value(&current);
        match mode {
            FilterFocusMode::SelectAll => filter_node.select_all(),
            FilterFocusMode::CaretEnd => filter_node.set_selection(current.len()..current.len()),
        }
    });
    on_cleanup_local(move || drop(focus_fx));
}
