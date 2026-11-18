use crate::ui::Icon;
use anti_webui::prelude::*;
use gds_uim::el;

#[derive(Clone, PartialEq)]
pub struct SearchOption<T> {
    pub value: T,
    pub label: String,
    keywords: &'static str,
    disabled: bool,
}

impl<T> SearchOption<T> {
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            keywords: "",
            disabled: false,
        }
    }
    pub fn keywords(mut self, keywords: &'static str) -> Self {
        self.keywords = keywords;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// A compact settings choice with an anchored search field and keyboard navigation.
pub fn search_choice<T: Clone + PartialEq + 'static>(
    value: Binding<T>,
    choices: Vec<SearchOption<T>>,
    caption: impl Into<String>,
    placeholder: impl Into<String>,
) -> Div {
    let choices = signal(choices);
    let caption = caption.into();
    let placeholder = placeholder.into();
    let open = signal(false);
    let anchor = node_ref();
    let dismiss = move || {
        open.set(false);
        anchor.focus();
    };
    let face = el! { .div.{
        flex; items_center; justify_between; gap: 8.0_f32; min_w: 0.0_f32;
        w_full; h: 24.0_f32; px: 8.0_f32; rounded: 5.0_f32; border: 1.0_f32;
        node_ref: anchor; focusable; cursor: CursorIcon::Pointer;
        access: move a => $(a.role(Role::ComboBox).expanded(open.get()));
        dyn_style: s => $(s.bg(theme().page).border_color(theme().border_strong));
        hover: s => $(s.bg(theme().hover));
        focused: s => $(s.outline(1.0_f32).outline_offset(-1.0_f32).outline_color(theme().focus_ring));
        on_click: move _ => open.{set: true};
        on_key_down: move event => ${
            if !open.get_untracked()
                && matches!(event.key, Key::Named(NamedKey::Enter | NamedKey::Space | NamedKey::ArrowDown))
            {
                event.prevent_default();
                event.stop_propagation();
                open.set(true);
            }
        };
    }.[
        (move () => ${
            let value = value.get();
            choices.with(|items| items.iter().find(|item| item.value == value)
                .map_or_else(|| caption.clone(), |item| item.label.clone()))
        }).text.{ min_w: 0.0_f32; ellipsis },
        $(Icon::ChevronDown.view(12.0)),
    ] };
    el! { face.[
        (move () => open.{get}, move () => ${
            let query = signal(String::new());
            let cursor = signal(None);
            let field = node_ref();
            let list = node_ref();
            let results = memo(move || {
                let query = query.get().to_lowercase();
                choices.with(|items| items.iter()
                    .filter(|item| item.label.to_lowercase().contains(&query) || item.keywords.to_lowercase().contains(&query))
                    .cloned().collect::<Vec<_>>())
            });
            effect(move || {
                let selected = value.get_untracked();
                let index = results.with(|items| {
                    items.iter().position(|item| !item.disabled && item.value == selected)
                        .or_else(|| items.iter().position(|item| !item.disabled))
                });
                cursor.set_if_changed(index);
            });
            let choose = move |index: Option<usize>| {
                if let Some(next) = untrack(|| results.with(|items| items.get(index?).filter(|item| !item.disabled).cloned())) {
                    dismiss();
                    value.set(next.value);
                }
            };
            let placeholder = placeholder.clone();
            request_frame(move |_| {
                field.focus();
                if cursor.is_alive() && let Some(index) = cursor.get_untracked() {
                    list.scroll_to_after_layout(0, window_scale().px(index as f32 * 28.0));
                }
            });
            let rows = virtual_list(move || results.with(Vec::len), 28.0, move |index| {
                dyn_view(move || {
                    let Some(item) = results.with(|items| items.get(index).cloned()) else {
                        return div();
                    };
                    let disabled = item.disabled;
                    let selected = value.get_untracked() == item.value;
                    el! { .div.{
                        flex; items_center; w_full; h: 28.0_f32; px: 8.0_f32; rounded: 4.0_f32;
                        cursor: $(if disabled { CursorIcon::Default } else { CursorIcon::Pointer });
                        disabled: move () => disabled;
                        on_hover: move inside => $(if inside && !disabled { cursor.set(Some(index)); });
                        access: move a => $(a.role(Role::ListOption).selected(cursor.get() == Some(index)));
                        dyn_style: move s => $(s.bg(if cursor.get() == Some(index) { theme().selected } else { Color::TRANSPARENT })
                            .text_color(if disabled { theme().text_disabled } else { theme().text }));
                        on_click: move _ => $(choose(Some(index)));
                    }.[item->label.text.{ flex_1; ellipsis }, if selected [$(Icon::Check.view(12.0))]] }
                })
            });
            el! { .popup.{ anchor: anchor; gap: 4; match_anchor_width; on_dismiss: dismiss }.[
                .div.{
                    flex; flex_col; gap: 4.0_f32; p: 4.0_f32; rounded: 6.0_f32; border: 1.0_f32;
                    shadow_lg; dyn_style: s => $(s.bg(theme().overlay).border_color(theme().border));
                    on_click: event => event.{stop_propagation};
                    on_key_down: move event => ${
                        let forward = match event.key {
                            Key::Named(NamedKey::ArrowDown) => true,
                            Key::Named(NamedKey::ArrowUp) => false,
                            Key::Named(NamedKey::Escape) => {
                                event.stop_propagation();
                                event.prevent_default();
                                dismiss();
                                return;
                            }
                            _ => return,
                        };
                        let current = cursor.get_untracked();
                        let next = untrack(|| results.with(|items| {
                            let mut active = items.iter().enumerate().filter(|(_, item)| !item.disabled);
                            if forward {
                                active.find_map(|(index, _)| current.is_none_or(|current| index > current).then_some(index))
                            } else {
                                active.rev().find_map(|(index, _)| current.is_none_or(|current| index < current).then_some(index))
                            }
                        }));
                        if let Some(next) = next {
                            cursor.set(Some(next));
                            anti_webui::widgets::scroll_row_into_view(list, next, 28.0);
                        }
                        event.prevent_default();
                        event.stop_propagation();
                    };
                }.[
                    query.text_field.{ node_ref: field; placeholder: placeholder; h: 28.0_f32;
                        on_submit: move _ => $(choose(cursor.get_untracked()));
                    },
                    rows.{
                        node_ref: list; flex_none;
                        dyn_style: move s => $(s.h(28.0 * results.with(|items| items.len().clamp(1, 8)) as f32));
                        access: a => $(a.role(Role::ListBox));
                    },
                ],
            ] }
        }).show,
    ] }
}
