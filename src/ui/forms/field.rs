//! Unified GDS input pills, field labels, and searchable choice controls.

pub mod placeholder;
pub use placeholder::Placeholder;

use std::collections::HashMap;

use zgui::geom::{DevicePx, Point};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, Memo, RenderEffect, RwSignal, UnsyncCallback, on_cleanup_local,
};
use zgui::view::{
    AttrName, ClassName, CustomPropertyName, ScrollBehavior, ScrollTarget, ShowProps,
};
use zgui::vocab::{HasPopup, Key, Live, NamedKey, PointerButton, Role, UiState};
use zgui::{component, css, view};
use zgui_ui::{
    AnchoredSurfaceProps, ComboboxStyle, InputGroupAddonAlign, InputGroupAddonProps,
    InputGroupPartStyle, InputGroupProps, InputProps, OverlayState, SelectStyle, TextareaProps,
    Virtualize,
};
use zgui_ui_primitives::{Align, Binding, Controllable, Placement, Side};

use crate::app::Gds;
use crate::i18n::Language;
use crate::ui::icon::{IconName, IconProps};

const FIELD_SHEET: &str = "gds-ui-field";
const COMBOBOX_SHEET: &str = "zui-combobox";
const SELECT_SHEET: &str = "zui-select";
const INPUT_GROUP_PARTS_SHEET: &str = "zui-input-group-parts";

const DEFAULT_CHOICE_ROW_HEIGHT: f32 = 32.0;
const MIN_CHOICE_ROW_HEIGHT: f32 = 28.0;
const POPUP_MAX_LIST_HEIGHT: f32 = 376.0;
const POPUP_OVERSCAN: usize = 3;
const PAGE_JUMP_ROWS: usize = 10;

const FIELD_CSS: &str = css!(
    ".gds-input-field.gds-input-field {
        position: relative;
        display: flex;
        flex-direction: row;
        align-items: center;
        width: 100%;
        min-width: 0px;
        height: var(--gds-control-height, 36px);
        min-height: var(--gds-control-height, 36px);
        padding: 0px 10px;
        box-sizing: border-box;
        background: var(--gds-content);
        color: var(--gds-foreground);
        border: 1px solid var(--gds-outline);
        border-radius: var(--gds-radius-control, 6px);
        box-shadow: none;
        outline: none;
        transition: border-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-input-field.gds-input-field[data-has-leading=\"true\"] {
        padding-left: 0px;
    }
    .gds-input-field.gds-input-field[data-has-trailing=\"true\"] {
        padding-right: 0px;
    }
    .gds-input-field:not([data-has-trailing=\"true\"]) > .gds-input-field__trailing {
        display: none;
    }
    .gds-input-field.gds-input-field[data-compact=\"true\"] {
        height: var(--gds-icon-button-size, 32px);
        min-height: var(--gds-icon-button-size, 32px);
    }
    .gds-input-field.gds-input-field:focus-within {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }
    .gds-input-field.gds-input-field[data-invalid=\"true\"],
    .gds-input-field.gds-input-field[data-invalid=\"true\"]:focus-within {
        border-color: var(--gds-error);
        box-shadow: none;
        outline: none;
    }
    .gds-input-field.gds-input-field[data-disabled=\"true\"] {
        opacity: 0.5;
        pointer-events: none;
    }
    .gds-input-field__leading {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        gap: 4px;
        padding-left: 10px;
        color: var(--gds-muted-foreground);
        cursor: text;
        user-select: none;
    }
    .gds-input-field__trailing {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        gap: 4px;
        padding-right: 4px;
        color: var(--gds-muted-foreground);
    }
    .gds-input-field__clear {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 24px;
        height: 24px;
        flex-shrink: 0;
        padding: 0px;
        border: none;
        border-radius: 4px;
        background: transparent;
        color: var(--gds-muted-foreground);
        cursor: pointer;
        outline: none;
        box-shadow: none;
    }
    .gds-input-field__clear:hover {
        background: var(--gds-hover);
        color: var(--gds-foreground);
    }
    .gds-input-field__clear:active {
        background: var(--gds-pressed);
    }
    .gds-input-field__clear:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }
    .gds-input-field .zui-field.gds-input-field__input {
        flex: 1 1 0%;
        width: auto;
        height: 100%;
        min-width: 0px;
        padding: 0px;
        border: 0px solid transparent;
        border-radius: 0px;
        background: transparent;
        color: var(--gds-foreground);
        font-family: var(--gds-font-sans);
        font-size: var(--gds-font-size-small, var(--gds-font-size-sm, 14px));
        box-shadow: none;
        outline: none;
    }
    .gds-input-field[data-has-leading=\"true\"] .zui-field.gds-input-field__input {
        padding-left: 6px;
    }
    .gds-input-field[data-has-trailing=\"true\"] .zui-field.gds-input-field__input {
        padding-right: 4px;
    }
    .gds-input-field .zui-field.gds-input-field__input:focus,
    .gds-input-field .zui-field.gds-input-field__input:focus-visible,
    .gds-input-field .zui-field.gds-input-field__input:invalid,
    .gds-input-field .zui-field.gds-input-field__input:invalid:focus,
    .gds-input-field .zui-field.gds-input-field__input:invalid:focus-visible {
        border-color: transparent;
        box-shadow: none;
        outline: none;
    }
    .gds-input-field .zui-field.gds-input-field__input:empty::before {
        color: var(--gds-muted-foreground);
    }
    .zui-field.gds-textarea-field {
        background: var(--gds-content);
        color: var(--gds-foreground);
        border: 1px solid var(--gds-outline);
        border-radius: var(--gds-radius-control, 6px);
        font-family: var(--gds-font-sans);
        font-size: var(--gds-font-size-small, var(--gds-font-size-sm, 14px));
        box-shadow: none;
        outline: none;
    }
    .zui-field.gds-textarea-field:focus,
    .zui-field.gds-textarea-field:focus-visible {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }
    .zui-field.gds-textarea-field:invalid,
    .zui-field.gds-textarea-field:invalid:focus,
    .zui-field.gds-textarea-field:invalid:focus-visible {
        border-color: var(--gds-error);
        box-shadow: none;
        outline: none;
    }
    .zui-input-group.gds-search-choice-field {
        position: relative;
        min-height: var(--gds-control-height, 36px);
        background: var(--gds-content);
        color: var(--gds-foreground);
        border: 1px solid var(--gds-outline);
        border-radius: var(--gds-radius-control, 6px);
        box-shadow: none;
        outline: none;
    }
    .zui-input-group.gds-search-choice-field:focus-within {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }
    .gds-search-choice-field .zui-field.gds-search-choice-field__input {
        padding-right: 32px;
        color: var(--gds-foreground);
        font-family: var(--gds-font-sans);
        font-size: var(--gds-font-size-small, var(--gds-font-size-sm, 14px));
    }
    .gds-search-choice-field > .zui-input-group__addon.gds-search-choice-field__addon {
        position: absolute;
        top: 0px;
        right: 0px;
        bottom: 0px;
        padding-right: 10px;
        pointer-events: none;
    }
    .zui-combobox__list.gds-search-choice-popup {
        padding: 4px 0px;
        min-width: max(220px, var(--zui-popper-anchor-width, 220px));
        max-width: min(420px, var(--zui-popper-available-width, 420px));
        max-height: min(384px, var(--zui-popper-available-height, 384px));
        overflow: hidden;
    }
    .gds-search-choice-popup > .zui-combobox__empty {
        padding: var(--zui-space-sm, 8px) 12px;
    }
    .gds-search-choice-list {
        display: block;
        flex: 1 1 auto;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        max-height: min(376px, max(0px, calc(var(--zui-popper-available-height, 384px) - 8px)));
        padding: 0px 4px;
        box-sizing: border-box;
        overflow-y: auto;
        overflow-x: hidden;
    }
    .gds-search-choice-list__items {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
    }
    .zui-select__item.gds-search-choice-item {
        height: var(--gds-choice-row-height, 32px);
        min-height: var(--gds-choice-row-height, 32px);
        flex: none;
        box-sizing: border-box;
        overflow: hidden;
        white-space: nowrap;
    }
    .gds-search-choice-item__label {
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }"
);

pub(crate) fn install_field_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(COMBOBOX_SHEET, ComboboxStyle::CSS);
    zgui::view::install_stylesheet(SELECT_SHEET, SelectStyle::CSS);
    zgui::view::install_stylesheet(INPUT_GROUP_PARTS_SHEET, InputGroupPartStyle::CSS);
    zgui::view::install_stylesheet(FIELD_SHEET, FIELD_CSS);
}

fn clear_button_label() -> String {
    Gds::try_current().map_or_else(
        || "Clear".to_owned(),
        |gds| match gds.i18n().language() {
            Language::Chinese => "清空".to_owned(),
            Language::English => "Clear".to_owned(),
        },
    )
}

fn choice_row_height() -> f32 {
    Gds::try_current().map_or(DEFAULT_CHOICE_ROW_HEIGHT, |gds| {
        gds.appearance()
            .typography()
            .scaled(DEFAULT_CHOICE_ROW_HEIGHT)
            .round()
            .max(MIN_CHOICE_ROW_HEIGHT)
    })
}

/// Accessible field title label bound with `Role::Label` and optional `node_ref` for `labelled_by`.
#[component]
pub fn FieldLabel(
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_field_stylesheets();
    view! {
        text(
            class = "gds-setting-title",
            a11y:role = Role::Label,
            node_ref = node_ref.unwrap_or_default(),
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}

/// Option entry for [`SearchChoiceField`], supporting display labels plus hidden search keywords.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOption {
    pub value: String,
    pub label: String,
    pub keywords: Vec<String>,
}

impl SearchOption {
    #[must_use]
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            keywords: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn matches_query(&self, query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return true;
        }
        let label_lower = self.label.to_lowercase();
        let value_lower = self.value.to_lowercase();
        let keyword_lowers: Vec<String> = self
            .keywords
            .iter()
            .map(|term| term.to_lowercase())
            .collect();
        trimmed.split_whitespace().all(|token| {
            let needle = token.to_lowercase();
            label_lower.contains(&needle)
                || value_lower.contains(&needle)
                || keyword_lowers.iter().any(|term| term.contains(&needle))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreparedOption {
    option: SearchOption,
    label_lower: String,
    value_lower: String,
    keywords_lower: Vec<String>,
}

impl PreparedOption {
    fn new(option: SearchOption) -> Self {
        let label_lower = option.label.to_lowercase();
        let value_lower = option.value.to_lowercase();
        let keywords_lower = option
            .keywords
            .iter()
            .map(|term| term.to_lowercase())
            .collect();
        Self {
            option,
            label_lower,
            value_lower,
            keywords_lower,
        }
    }

    fn matches_tokens(&self, tokens: &[String]) -> bool {
        tokens.iter().all(|needle| {
            self.label_lower.contains(needle.as_str())
                || self.value_lower.contains(needle.as_str())
                || self
                    .keywords_lower
                    .iter()
                    .any(|term| term.contains(needle.as_str()))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchCatalogue {
    entries: Vec<PreparedOption>,
    labels: HashMap<String, String>,
}

impl SearchCatalogue {
    fn from_options(options: Vec<SearchOption>) -> Self {
        let mut labels = HashMap::with_capacity(options.len());
        let mut entries = Vec::with_capacity(options.len());
        for option in options {
            labels
                .entry(option.value.clone())
                .or_insert_with(|| option.label.clone());
            entries.push(PreparedOption::new(option));
        }
        Self { entries, labels }
    }

    fn all(&self) -> Vec<SearchOption> {
        self.entries
            .iter()
            .map(|entry| entry.option.clone())
            .collect()
    }

    fn filter(&self, query: &str) -> Vec<SearchOption> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.all();
        }
        let tokens: Vec<String> = trimmed.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .filter(|entry| entry.matches_tokens(&tokens))
            .map(|entry| entry.option.clone())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollRequest {
    None,
    Top,
    CenterIndex(usize),
    RevealIndex(usize),
}

#[derive(Clone, Copy)]
struct SearchChoiceState {
    surface: OverlayState,
    chosen: Controllable<String>,
    remembered: RwSignal<Option<(String, String)>, LocalStorage>,
    catalogue: Memo<SearchCatalogue>,
    filter: RwSignal<String, LocalStorage>,
    query_active: RwSignal<bool, LocalStorage>,
    filtered: Memo<Vec<SearchOption>>,
    active_index: RwSignal<Option<usize>, LocalStorage>,
    resolved_active: Memo<Option<usize>>,
    active_node: RwSignal<Option<NodeRef>, LocalStorage>,
    scroll_req: RwSignal<ScrollRequest, LocalStorage>,
    row_size: Signal<f32, LocalStorage>,
    disabled: Signal<bool, LocalStorage>,
}

impl SearchChoiceState {
    fn new(
        value: Binding<String>,
        options: Signal<Vec<SearchOption>, LocalStorage>,
        on_change: Option<UnsyncCallback<String>>,
        disabled: Signal<bool, LocalStorage>,
    ) -> Self {
        let filter = RwSignal::new_local(String::new());
        let query_active = RwSignal::new_local(false);
        let active_node: RwSignal<Option<NodeRef>, LocalStorage> = RwSignal::new_local(None);

        let surface = OverlayState::uncontrolled(
            false,
            Some(UnsyncCallback::new(move |open: bool| {
                if !open {
                    if filter.try_with_untracked(|text| !text.is_empty()) == Some(true) {
                        filter.try_set(String::new());
                    }
                    if query_active.try_get_untracked() == Some(true) {
                        query_active.try_set(false);
                    }
                    active_node.try_set(None);
                }
            })),
        );

        let chosen = Controllable::new(value, String::new(), on_change);
        let remembered = RwSignal::new_local(None);
        let catalogue = Memo::new(move |_| SearchCatalogue::from_options(options.get()));
        let filtered = Memo::new(move |_| {
            if !query_active.get() {
                catalogue.with(SearchCatalogue::all)
            } else {
                let query = filter.get();
                catalogue.with(|cat| cat.filter(&query))
            }
        });
        let active_index = RwSignal::new_local(Some(0));
        let resolved_active = Memo::new(move |_| {
            let len = filtered.with(Vec::len);
            if len == 0 {
                None
            } else {
                Some(active_index.get().unwrap_or(0).min(len - 1))
            }
        });
        let scroll_req = RwSignal::new_local(ScrollRequest::None);
        let row_size = Signal::derive_local(choice_row_height);

        let disable_watch = RenderEffect::new(move |_| {
            if disabled.get() && surface.is_open_untracked() {
                surface.close();
            }
        });
        on_cleanup_local(move || drop(disable_watch));

        Self {
            surface,
            chosen,
            remembered,
            catalogue,
            filter,
            query_active,
            filtered,
            active_index,
            resolved_active,
            active_node,
            scroll_req,
            row_size,
            disabled,
        }
    }

    fn chosen_label(self) -> String {
        let current = self.chosen.get();
        self.catalogue
            .with(|cat| cat.labels.get(&current).cloned())
            .or_else(|| {
                self.remembered.with(|pair| {
                    pair.as_ref()
                        .and_then(|(val, label)| (*val == current).then(|| label.clone()))
                })
            })
            .unwrap_or_default()
    }

    fn display_text(self) -> String {
        if self.surface.is_open() && self.query_active.get() {
            self.filter.get()
        } else {
            self.chosen_label()
        }
    }

    fn reset_query(self) {
        if self.filter.try_with_untracked(|text| !text.is_empty()) == Some(true) {
            self.filter.try_set(String::new());
        }
        if self.query_active.try_get_untracked() == Some(true) {
            self.query_active.try_set(false);
        }
    }

    fn set_typed_query(self, text: String) {
        if self.disabled.get_untracked() {
            return;
        }
        self.filter.set(text);
        if !self.query_active.get_untracked() {
            self.query_active.set(true);
        }
        let has_any = self.filtered.with_untracked(|items| !items.is_empty());
        self.active_index.set(has_any.then_some(0));
        self.scroll_req.set(ScrollRequest::Top);
        self.surface.open();
    }

    fn open_on_chosen(self) {
        if self.disabled.get_untracked() {
            return;
        }
        self.reset_query();
        let current = self.chosen.get_untracked();
        let target = self.filtered.with_untracked(|items| {
            if items.is_empty() {
                None
            } else {
                Some(
                    items
                        .iter()
                        .position(|opt| opt.value == current)
                        .unwrap_or(0),
                )
            }
        });
        self.active_index.set(target);
        self.scroll_req
            .set(target.map_or(ScrollRequest::Top, ScrollRequest::CenterIndex));
        self.surface.open();
    }

    fn step_wrap(self, delta: isize) {
        let len = self.filtered.with_untracked(Vec::len);
        if len == 0 {
            return;
        }
        let next = match self.resolved_active.get_untracked() {
            Some(at) => (at as isize + delta).rem_euclid(len as isize) as usize,
            None if delta >= 0 => 0,
            None => len - 1,
        };
        self.active_index.set(Some(next));
        self.scroll_req.set(ScrollRequest::RevealIndex(next));
    }

    fn step_page(self, down: bool) {
        let len = self.filtered.with_untracked(Vec::len);
        if len == 0 {
            return;
        }
        let current = self.resolved_active.get_untracked().unwrap_or(0);
        let next = if down {
            current.saturating_add(PAGE_JUMP_ROWS).min(len - 1)
        } else {
            current.saturating_sub(PAGE_JUMP_ROWS)
        };
        self.active_index.set(Some(next));
        self.scroll_req.set(ScrollRequest::RevealIndex(next));
    }

    fn jump_end(self, last: bool) {
        let len = self.filtered.with_untracked(Vec::len);
        if len == 0 {
            return;
        }
        let next = if last { len - 1 } else { 0 };
        self.active_index.set(Some(next));
        self.scroll_req.set(ScrollRequest::RevealIndex(next));
    }

    fn pick_index(self, index: usize) -> bool {
        if self.disabled.get_untracked() {
            return false;
        }
        let picked = self
            .filtered
            .with_untracked(|items| items.get(index).cloned());
        let Some(option) = picked else {
            return false;
        };
        self.remembered
            .set(Some((option.value.clone(), option.label)));
        self.chosen.set(option.value);
        self.surface.close();
        self.reset_query();
        true
    }

    fn handle_key_down(self, ev: &mut EventCx<'_, events::KeyDown>) {
        if self.disabled.get_untracked() {
            return;
        }
        let open = self.surface.is_open_untracked();
        let Key::Named(named) = &ev.key else {
            return;
        };
        if matches!(named, NamedKey::Tab) {
            if open {
                self.surface.close();
                self.reset_query();
            }
            return;
        }
        if !ev.modifiers.is_empty() {
            return;
        }
        match named {
            NamedKey::ArrowDown => {
                ev.prevent_default();
                ev.stop_propagation();
                if open {
                    self.step_wrap(1);
                } else {
                    self.open_on_chosen();
                }
            }
            NamedKey::ArrowUp => {
                ev.prevent_default();
                ev.stop_propagation();
                if open {
                    self.step_wrap(-1);
                } else {
                    self.open_on_chosen();
                }
            }
            NamedKey::PageDown if open => {
                ev.prevent_default();
                ev.stop_propagation();
                self.step_page(true);
            }
            NamedKey::PageUp if open => {
                ev.prevent_default();
                ev.stop_propagation();
                self.step_page(false);
            }
            NamedKey::Home if open => {
                ev.prevent_default();
                ev.stop_propagation();
                self.jump_end(false);
            }
            NamedKey::End if open => {
                ev.prevent_default();
                ev.stop_propagation();
                self.jump_end(true);
            }
            NamedKey::Enter if open => {
                if let Some(index) = self.resolved_active.get_untracked()
                    && self.pick_index(index)
                {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            }
            NamedKey::Escape if open => {
                ev.prevent_default();
                ev.stop_propagation();
                self.surface.close();
                self.reset_query();
            }
            _ => {}
        }
    }
}

#[component]
fn SearchChoiceRow(
    state: SearchChoiceState,
    index: usize,
    total: Signal<usize, LocalStorage>,
) -> impl IntoView {
    let row_ref = NodeRef::new();
    let entry = Memo::new(move |_| {
        state.filtered.with(|items| {
            items
                .get(index)
                .map(|opt| (opt.value.clone(), opt.label.clone()))
        })
    });
    let is_chosen = Memo::new(move |_| {
        let current = state.chosen.get();
        entry.with(|item| item.as_ref().is_some_and(|(val, _)| *val == current))
    });
    let is_active = Memo::new(move |_| state.resolved_active.get() == Some(index));

    let active_effect = RenderEffect::new(move |_| {
        if is_active.get() {
            if state.active_node.get_untracked() != Some(row_ref) {
                state.active_node.set(Some(row_ref));
            }
        } else if state.active_node.get_untracked() == Some(row_ref) {
            state.active_node.set(None);
        }
    });
    on_cleanup_local(move || {
        drop(active_effect);
        if state.active_node.try_get_untracked().flatten() == Some(row_ref) {
            state.active_node.try_set(None);
        }
    });

    let own = Attrs::new()
        .class_toggle(ClassName::new("zui-select__item"), true)
        .class_toggle(ClassName::new("gds-search-choice-item"), true)
        .attribute(AttrName::new("data-index"), Some(index.to_string()))
        .attribute(AttrName::new("data-value"), move || {
            entry.with(|item| item.as_ref().map(|(val, _)| val.clone()))
        })
        .attribute(AttrName::new("data-state"), move || {
            Some(
                if is_chosen.get() {
                    "checked"
                } else {
                    "unchecked"
                }
                .to_owned(),
            )
        })
        .attribute(AttrName::new("data-active"), move || {
            is_active.get().then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-disabled"), move || {
            state.disabled.get().then(|| "true".to_owned())
        })
        .state(UiState::CHECKED, move || is_chosen.get())
        .state(UiState::DISABLED, move || state.disabled.get())
        .a11y_from(
            A11yBinding::new(Role::ListBoxOption)
                .label(move || {
                    entry.with(|item| {
                        item.as_ref()
                            .map_or_else(String::new, |(_, label)| label.clone())
                    })
                })
                .selected(move || is_chosen.get())
                .disabled(move || state.disabled.get())
                .step(move |a11y| {
                    let len = total.try_get().unwrap_or_default().max(index + 1);
                    a11y.set_position(index + 1, len)
                }),
        );

    view! {
        box(
            node_ref = row_ref,
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| {
                if ev.button == Some(PointerButton::Primary) {
                    ev.prevent_default();
                    ev.activate_now();
                }
            },
            on:click = move |_| {
                state.pick_index(index);
            },
            on:pointer_enter = move |_| {
                if !state.disabled.get_untracked()
                    && state.active_index.get_untracked() != Some(index)
                {
                    state.active_index.set(Some(index));
                }
            },
            {..own}
        ) {
            box(class = "zui-select__indicator") {
                Show(when = move || is_chosen.get(), fallback = || ()) {
                    Icon(name = IconName::Check, size = 14.0)
                }
            }
            text(class = "gds-search-choice-item__label") {
                {move || {
                    entry.with(|item| {
                        item.as_ref()
                            .map_or_else(String::new, |(_, label)| label.clone())
                    })
                }}
            }
        }
    }
}

fn target_scroll_offset(
    req: ScrollRequest,
    row_height: f32,
    port_height: f32,
    current_y: f32,
) -> Option<f32> {
    match req {
        ScrollRequest::None => None,
        ScrollRequest::Top => (current_y > 0.5).then_some(0.0),
        ScrollRequest::CenterIndex(index) => {
            let row_top = index as f32 * row_height;
            let centered = (row_top - (port_height - row_height) * 0.5).max(0.0);
            ((centered - current_y).abs() > 0.5).then_some(centered)
        }
        ScrollRequest::RevealIndex(index) => {
            let row_top = index as f32 * row_height;
            let row_bottom = row_top + row_height;
            if row_top < current_y {
                Some(row_top)
            } else if row_bottom > current_y + port_height {
                Some((row_bottom - port_height).max(0.0))
            } else {
                None
            }
        }
    }
}

#[component]
fn SearchChoicePopup(
    state: SearchChoiceState,
    empty_text: Signal<String, LocalStorage>,
) -> impl IntoView {
    let viewport = NodeRef::new();
    let scroll_pos = viewport.observe_scroll();
    let count = Signal::derive_local(move || state.filtered.with(Vec::len));
    let is_empty = Memo::new(move |_| count.get() == 0);
    let seen = Virtualize::new(viewport, count, state.row_size, POPUP_OVERSCAN);
    let window = Signal::derive_local(move || seen.window());

    let scroll_effect = RenderEffect::new(move |_| {
        let req = state.scroll_req.get();
        if req == ScrollRequest::None || viewport.get().is_none() {
            return;
        }
        let pos = scroll_pos.get();
        let scale = viewport.scale();
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let row_h = state.row_size.get_untracked().max(1.0);
        let measured_port = pos.scrollport.height.0 / scale;
        let cur_y = pos.offset.y.0 / scale;
        let effective_port = if measured_port > 0.0 {
            measured_port
        } else {
            POPUP_MAX_LIST_HEIGHT
        };

        if let Some(target_y) = target_scroll_offset(req, row_h, effective_port, cur_y) {
            viewport.scroll_to(
                ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(target_y * scale))),
                ScrollBehavior::Instant,
            );
        }
        if measured_port > 0.0 || matches!(req, ScrollRequest::Top) {
            state.scroll_req.set(ScrollRequest::None);
        }
    });
    on_cleanup_local(move || drop(scroll_effect));

    let list_attrs = Attrs::new().custom_property(
        CustomPropertyName::new("gds-choice-row-height"),
        move || state.row_size.try_get().map(|size| format!("{size}px")),
    );

    view! {
        Show(when = move || !is_empty.get(), fallback = || ()) {
            scroll(
                node_ref = viewport,
                class = "gds-search-choice-list",
                {..list_attrs.clone()}
            ) {
                column(
                    class = "gds-search-choice-list__items",
                    style:padding-top = move || window.try_get().map(|w| format!("{}px", w.lead)),
                    style:padding-bottom = move || window.try_get().map(|w| format!("{}px", w.trail)),
                ) {
                    for index in move || window.try_get().map(|w| w.indices()).unwrap_or_default(),
                        key = |index: &usize| *index
                    {
                        SearchChoiceRow(state = state, index = index, total = count)
                    }
                }
            }
        }
        Show(when = move || is_empty.get(), fallback = || ()) {
            box(
                class = "zui-combobox__empty",
                a11y:live = Live::Polite,
            ) {
                {move || empty_text.get()}
            }
        }
    }
}

/// Searchable choice combobox supporting matching on both visible labels and hidden keywords,
/// backed by a pre-normalized catalogue and virtualized popup list.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn SearchChoiceField(
    #[prop(into, optional)] value: Binding<String>,
    #[prop(into)] options: Signal<Vec<SearchOption>, LocalStorage>,
    #[prop(into, optional)] placeholder: Option<String>,
    #[prop(into, default = Signal::stored_local("No matches".to_owned()))] empty_text: Signal<
        String,
        LocalStorage,
    >,
    #[prop(optional)] on_change: Option<UnsyncCallback<String>>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, optional)] label: Option<String>,
    #[prop(optional)] labelled_by: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_field_stylesheets();
    let state = SearchChoiceState::new(value, options, on_change, disabled);
    let surface = state.surface.provide();
    let trigger = surface.trigger();

    let typed = Binding::controlled(
        Signal::derive_local(move || state.display_text()),
        move |text: String| state.set_typed_query(text),
    );

    let mut semantics = A11yBinding::new(Role::ComboBox)
        .has_popup(HasPopup::Listbox)
        .expanded(move || surface.is_open())
        .controls(surface.content())
        .value(move || zgui::vocab::SharedString::from(state.display_text()))
        .active_descendant(move || {
            state
                .active_node
                .get()
                .and_then(|node_ref| node_ref.get())
                .map_or(zgui::vocab::NodeId(0), |id| {
                    zgui::vocab::NodeId(id.as_u64())
                })
        });
    if let Some(text) = label {
        semantics = semantics.label(text);
    }
    if let Some(target) = labelled_by {
        semantics = semantics.labelled_by(target);
    }

    let own_input = Attrs::new()
        .class_toggle(ClassName::new("zui-input-group__field"), true)
        .class_toggle(ClassName::new("zui-combobox__input"), true)
        .class_toggle(ClassName::new("gds-search-choice-field__input"), true)
        .attribute(AttrName::new("data-state"), move || {
            Some(surface.state_name().to_owned())
        })
        .a11y_from(semantics);

    let on_pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button == Some(PointerButton::Primary)
            && !state.disabled.get_untracked()
            && !surface.is_open_untracked()
        {
            state.open_on_chosen();
        }
    };
    let on_click = move |_: &mut EventCx<'_, events::Click>| {
        if !state.disabled.get_untracked() && !surface.is_open_untracked() {
            state.open_on_chosen();
        }
    };
    let on_key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
        state.handle_key_down(ev);
    };

    let input_group = view! {
        InputGroup(
            disabled = disabled,
            class = "gds-search-choice-field",
        ) {
            Input(
                node_ref = trigger,
                value = typed,
                placeholder = placeholder,
                disabled = disabled,
                on:pointer_down = on_pointer_down,
                on:click = on_click,
                on:key_down = on_key_down,
                {..own_input},
                {..attrs},
                class = class,
            ) {}
            InputGroupAddon(
                align = InputGroupAddonAlign::InlineEnd,
                class = "gds-search-choice-field__addon",
                a11y:hidden = true,
            ) {
                Icon(name = IconName::ChevronDown, size = 16.0, class = "zui-combobox__mark")
            }
        }
    };

    let popup = view! {
        AnchoredSurface(
            state = surface,
            placement = {Placement::new(Side::Bottom, Align::Start)},
            offset = 4.0,
            role = {Role::ListBox},
            dismiss_on_outside_press = {true},
            class = "zui-combobox__list gds-search-choice-popup",
        ) {
            SearchChoicePopup(state = state, empty_text = empty_text)
        }
    };

    (input_group.into_view(), popup.into_view())
}

/// Unified `6px`-radius GDS single-line input pill with optional leading icon/view,
/// trailing view, `32px` compact mode, and inline clear button.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn InputField(
    #[prop(into, optional)] value: Binding<String>,
    #[prop(into, optional)] default_value: Option<String>,
    #[prop(into, optional)] placeholder: Option<String>,
    #[prop(optional)] on_change: Option<UnsyncCallback<String>>,
    #[prop(optional)] on_clear: Option<UnsyncCallback<()>>,
    #[prop(optional)] leading_icon: Option<IconName>,
    #[prop(optional)] leading: Option<AnyView>,
    #[prop(optional)] trailing: Option<AnyView>,
    #[prop(default = false)] clearable: bool,
    #[prop(default = false)] compact: bool,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] read_only: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] required: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] invalid: Signal<bool, LocalStorage>,
    #[prop(into, optional)] secure: Option<Signal<bool, LocalStorage>>,
    #[prop(into, optional)] label: Option<String>,
    #[prop(optional)] labelled_by: Option<NodeRef>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_field_stylesheets();
    let field_ref = node_ref.unwrap_or_default();
    let unbound_non_empty =
        RwSignal::new_local(default_value.as_ref().is_some_and(|s| !s.is_empty()));

    let combined_on_change = UnsyncCallback::new(move |next: String| {
        if !value.is_bound() {
            unbound_non_empty.set(!next.is_empty());
        }
        if let Some(cb) = on_change {
            cb.run(next);
        }
    });

    let show_clear = Signal::derive_local(move || {
        if !clearable || disabled.get() || read_only.get() {
            return false;
        }
        value
            .get()
            .map_or_else(|| unbound_non_empty.get(), |text| !text.is_empty())
    });

    let has_leading = leading_icon.is_some() || leading.is_some();
    let has_static_trailing = trailing.is_some();
    let has_trailing = move || has_static_trailing || show_clear.get();

    let leading_slot = has_leading.then(|| {
        AnyView::new(view! {
            row(
                class = "gds-input-field__leading",
                a11y:hidden = true,
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    if ev.button == Some(PointerButton::Primary) && !disabled.get_untracked() {
                        ev.prevent_default();
                        field_ref.focus();
                    }
                },
            ) {
                {leading_icon.map(|icon| view! { Icon(name = icon, size = 16.0) }.into_view())}
                {leading}
            }
        })
    });

    let clear_view = clearable.then(|| {
        let on_clear_click = move |_: &mut EventCx<'_, events::Click>| {
            if disabled.get_untracked() || read_only.get_untracked() {
                return;
            }
            if value.is_bound() {
                value.write(String::new());
            } else {
                field_ref.set_value("");
                unbound_non_empty.set(false);
            }
            if let Some(cb) = on_change {
                cb.run(String::new());
            }
            if let Some(cb) = on_clear {
                cb.run(());
            }
            field_ref.focus();
        };
        AnyView::new(view! {
            Show(when = move || show_clear.get(), fallback = || ()) {
                control(
                    class = "gds-input-field__clear",
                    a11y:role = Role::Button,
                    a11y:label = clear_button_label,
                    on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| {
                        if ev.button == Some(PointerButton::Primary) {
                            ev.prevent_default();
                            ev.activate_now();
                        }
                    },
                    on:click = on_clear_click,
                ) {
                    Icon(name = IconName::X, size = 14.0)
                }
            }
        })
    });

    let trailing_slot = (clearable || has_static_trailing).then(|| {
        AnyView::new(view! {
            row(class = "gds-input-field__trailing") {
                {clear_view}
                {trailing}
            }
        })
    });

    let own = Attrs::new()
        .attribute(
            AttrName::new("data-compact"),
            compact.then(|| "true".to_owned()),
        )
        .attribute(
            AttrName::new("data-has-leading"),
            has_leading.then(|| "true".to_owned()),
        )
        .attribute(AttrName::new("data-has-trailing"), move || {
            has_trailing().then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-disabled"), move || {
            disabled.get().then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-invalid"), move || {
            invalid.get().then(|| "true".to_owned())
        });

    view! {
        row(
            class = "gds-input-field",
            {..own},
            {..attrs},
            class = class,
        ) {
            {leading_slot}
            Input(
                value = value,
                default_value = default_value,
                placeholder = placeholder,
                on_change = combined_on_change,
                disabled = disabled,
                read_only = read_only,
                required = required,
                invalid = invalid,
                secure = secure,
                label = label,
                labelled_by = labelled_by,
                node_ref = field_ref,
                class = "gds-input-field__input",
            ) {}
            {trailing_slot}
        }
    }
}

/// Multi-line GDS input editor delegating to `zgui_ui::Textarea`.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn TextareaField(
    #[prop(into, optional)] value: Binding<String>,
    #[prop(into, optional)] placeholder: Option<String>,
    #[prop(optional)] on_change: Option<UnsyncCallback<String>>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] invalid: Signal<bool, LocalStorage>,
    #[prop(into, optional)] label: Option<String>,
    #[prop(optional)] labelled_by: Option<NodeRef>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_field_stylesheets();
    view! {
        Textarea(
            value = value,
            placeholder = placeholder,
            on_change = on_change,
            disabled = disabled,
            invalid = invalid,
            label = label,
            labelled_by = labelled_by,
            node_ref = node_ref,
            class = "gds-textarea-field",
            class = class,
            {..attrs}
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;
    use std::sync::Arc;

    use zgui::app::Fonts;
    use zgui::geom::{CssPx, DevicePx, Point, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{LocalStorage, RwSignal, UnsyncCallback};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::vocab::{
        KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction,
        PointerButton, PointerEvent, Timestamp,
    };
    use zgui_platform_headless::{Harness, OffscreenSurface};

    use super::{
        ScrollRequest, SearchCatalogue, SearchChoiceFieldProps, SearchOption, target_scroll_offset,
    };

    #[derive(Default)]
    struct HeadlessSink {
        atlas: zgui::atlas::MemorySink,
        target: Option<RenderTarget>,
    }

    impl Renderer for HeadlessSink {
        fn capabilities(&self) -> RenderCapabilities {
            RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.target
        }

        fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
            TextureHandle(0)
        }

        fn release_external(&mut self, _: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::default()
        }

        fn draw(&mut self, _: &zgui::scene::Scene, _: &zgui::bits::DamageSet) -> FrameOutcome {
            FrameOutcome::Presented(FrameStats::default())
        }

        fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
            &mut self.atlas
        }
    }

    fn build_600_font_options() -> Vec<SearchOption> {
        let mut items = Vec::with_capacity(600);
        items.push(SearchOption::new("__system__", "System default").with_keywords(["auto"]));
        items.extend((0..199).map(|i| {
            SearchOption::new(format!("font-alpha-{i:03}"), format!("Adwaita Sans {i:03}"))
        }));
        items.extend(
            (0..150).map(|i| {
                SearchOption::new(format!("font-noto-{i:03}"), format!("Noto Sans {i:03}"))
            }),
        );
        items.extend((0..79).map(|i| {
            SearchOption::new(
                format!("font-sarasa-{i:03}"),
                format!("Sarasa Gothic {i:03}"),
            )
        }));
        items.push(
            SearchOption::new("Sarasa UI SC", "Sarasa UI SC").with_keywords(["cjk", "chinese"]),
        );
        items.extend((0..40).map(|i| {
            SearchOption::new(
                format!("font-source-{i:03}"),
                format!("Source Code Pro {i:03}"),
            )
        }));
        items
            .extend((0..130).map(|i| {
                SearchOption::new(format!("font-zed-{i:03}"), format!("Zed Mono {i:03}"))
            }));
        assert_eq!(items.len(), 600);
        items
    }

    struct NodeSnap {
        role: String,
        label: Option<String>,
        value: Option<String>,
        is_expanded: Option<bool>,
        is_selected: Option<bool>,
        active_descendant: Option<String>,
        bounds: Option<(f64, f64, f64, f64)>,
        translate: (f64, f64),
        children: Vec<String>,
    }

    struct A11yTreeState {
        nodes: HashMap<String, NodeSnap>,
        root: Option<String>,
    }

    impl A11yTreeState {
        fn new() -> Self {
            Self {
                nodes: HashMap::new(),
                root: None,
            }
        }

        fn sync(&mut self, offscreen: &OffscreenSurface) {
            for update in offscreen.take_a11y_log() {
                if let Some(tree) = update.tree {
                    self.root = Some(format!("{:?}", tree.root));
                }
                for (id, node) in update.nodes {
                    let translate = match node.transform() {
                        Some(affine) => {
                            let c = affine.as_coeffs();
                            (c[4], c[5])
                        }
                        None => (0.0, 0.0),
                    };
                    let bounds = node.bounds().map(|b| (b.x0, b.y0, b.x1, b.y1));
                    let snap = NodeSnap {
                        role: format!("{:?}", node.role()),
                        label: node.label().map(str::to_owned),
                        value: node.value().map(str::to_owned),
                        is_expanded: node.is_expanded(),
                        is_selected: node.is_selected(),
                        active_descendant: node.active_descendant().map(|t| format!("{t:?}")),
                        bounds,
                        translate,
                        children: node
                            .children()
                            .iter()
                            .map(|child| format!("{child:?}"))
                            .collect(),
                    };
                    self.nodes.insert(format!("{id:?}"), snap);
                }
            }
        }

        fn live_ids(&self) -> HashSet<String> {
            let mut visited = HashSet::new();
            let Some(root_id) = &self.root else {
                return visited;
            };
            let mut stack = vec![root_id.clone()];
            while let Some(id) = stack.pop() {
                if !visited.insert(id.clone()) {
                    continue;
                }
                if let Some(node) = self.nodes.get(&id) {
                    for child in &node.children {
                        stack.push(child.clone());
                    }
                }
            }
            visited
        }

        fn window_rects(&self) -> HashMap<String, (f32, f32, f32, f32)> {
            let mut rects = HashMap::new();
            let Some(root_id) = &self.root else {
                return rects;
            };
            let Some(root_node) = self.nodes.get(root_id) else {
                return rects;
            };
            let mut stack: Vec<_> = root_node
                .children
                .iter()
                .map(|child| (child.clone(), (0.0_f64, 0.0_f64)))
                .collect();
            while let Some((id, (ox, oy))) = stack.pop() {
                let Some(node) = self.nodes.get(&id) else {
                    continue;
                };
                let (nx, ny) = (ox + node.translate.0, oy + node.translate.1);
                if let Some((x0, y0, x1, y1)) = node.bounds {
                    rects.insert(
                        id,
                        (
                            (nx + x0) as f32,
                            (ny + y0) as f32,
                            (nx + x1) as f32,
                            (ny + y1) as f32,
                        ),
                    );
                }
                for child in &node.children {
                    stack.push((child.clone(), (nx, ny)));
                }
            }
            rects
        }

        fn combobox(&self) -> (String, &NodeSnap) {
            let live = self.live_ids();
            self.nodes
                .iter()
                .find(|(id, node)| live.contains(*id) && node.role == "ComboBox")
                .map(|(id, node)| (id.clone(), node))
                .expect("live Role::ComboBox node")
        }

        fn live_role_count(&self, role_name: &str) -> usize {
            let live = self.live_ids();
            self.nodes
                .iter()
                .filter(|(id, node)| live.contains(*id) && node.role == role_name)
                .count()
        }

        fn active_option_label(&self) -> Option<&str> {
            let (_, combo) = self.combobox();
            let active_id = combo.active_descendant.as_ref()?;
            let live = self.live_ids();
            if !live.contains(active_id) {
                return None;
            }
            self.nodes.get(active_id)?.label.as_deref()
        }

        fn option_is_selected(&self, label: &str) -> Option<bool> {
            let live = self.live_ids();
            self.nodes
                .iter()
                .find(|(id, node)| {
                    live.contains(*id)
                        && node.role == "ListBoxOption"
                        && node.label.as_deref() == Some(label)
                })
                .and_then(|(_, node)| node.is_selected)
        }

        fn option_rect(&self, label: &str) -> Option<(f32, f32, f32, f32)> {
            let live = self.live_ids();
            let rects = self.window_rects();
            let (id, _) = self.nodes.iter().find(|(id, node)| {
                live.contains(*id)
                    && node.role == "ListBoxOption"
                    && node.label.as_deref() == Some(label)
            })?;
            rects.get(id).copied()
        }

        fn contains_live_text(&self, needle: &str) -> bool {
            let live = self.live_ids();
            self.nodes.iter().any(|(id, node)| {
                live.contains(id)
                    && (node.label.as_deref() == Some(needle)
                        || node.value.as_deref() == Some(needle))
            })
        }
    }

    fn click_point<A: AppHandler>(harness: &mut Harness<A>, x: f32, y: f32) {
        let pt = Point::new(CssPx(x), CssPx(y));
        for action in [PointerAction::Pressed, PointerAction::Released] {
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action,
                event: PointerEvent::mouse(pt).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
        }
        harness.settle(8);
    }

    fn press_named<A: AppHandler>(harness: &mut Harness<A>, key: NamedKey, code: KeyCode) {
        for state in [KeyState::Pressed, KeyState::Released] {
            harness.deliver_to_first(SurfaceEvent::Key {
                state,
                event: KeyEvent::named(key, PhysicalKey::Code(code)),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
        }
        harness.settle(6);
    }

    fn backspace_times<A: AppHandler>(harness: &mut Harness<A>, count: usize) {
        for _ in 0..count {
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: KeyEvent::named(
                        NamedKey::Backspace,
                        PhysicalKey::Code(KeyCode::Backspace),
                    ),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
        }
        harness.settle(8);
    }

    fn select_all_and_replace<A: AppHandler>(harness: &mut Harness<A>, text: &str) {
        for state in [KeyState::Pressed, KeyState::Released] {
            harness.deliver_to_first(SurfaceEvent::Key {
                state,
                event: KeyEvent::character("a"),
                modifiers: Modifiers::CONTROL,
                timestamp: Timestamp::ORIGIN,
            });
        }
        if text.is_empty() {
            backspace_times(harness, 1);
            return;
        }
        for ch in text.chars() {
            let ev = KeyEvent::character(ch.to_string());
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: ev.clone(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
        }
        harness.settle(8);
    }

    #[test]
    fn prepared_catalogue_filters_keywords_and_computes_scroll_targets() {
        let catalogue = SearchCatalogue::from_options(build_600_font_options());
        assert_eq!(
            catalogue.labels.get("Sarasa UI SC").map(String::as_str),
            Some("Sarasa UI SC")
        );
        assert_eq!(
            catalogue.labels.get("__system__").map(String::as_str),
            Some("System default")
        );
        assert_eq!(catalogue.filter("").len(), 600);
        assert_eq!(catalogue.filter("sarasa cjk").len(), 1);
        assert_eq!(catalogue.filter("Sarasa").len(), 80);
        assert!(catalogue.filter("S").len() > 100);
        assert!(catalogue.filter("zzzz-no-font").is_empty());

        assert_eq!(
            target_scroll_offset(ScrollRequest::Top, 32.0, 376.0, 0.0),
            None
        );
        assert_eq!(
            target_scroll_offset(ScrollRequest::Top, 32.0, 376.0, 128.0),
            Some(0.0)
        );
        assert_eq!(
            target_scroll_offset(ScrollRequest::RevealIndex(2), 32.0, 376.0, 0.0),
            None
        );
        assert_eq!(
            target_scroll_offset(ScrollRequest::RevealIndex(12), 32.0, 376.0, 0.0),
            Some(40.0)
        );
        assert_eq!(
            target_scroll_offset(ScrollRequest::RevealIndex(1), 32.0, 376.0, 160.0),
            Some(32.0)
        );
    }

    #[test]
    fn search_choice_field_virtualizes_600_fonts_opens_on_click_and_commits_keyboard_selection() {
        let value_slot: Rc<RefCell<Option<RwSignal<String, LocalStorage>>>> =
            Rc::new(RefCell::new(None));
        let changes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let value_view = Rc::clone(&value_slot);
        let changes_view = Rc::clone(&changes);
        let value_driver = Rc::clone(&value_slot);
        let changes_driver = Rc::clone(&changes);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(640.0),
                DevicePx(520.0),
            )));
            let offscreen = harness.platform().offscreens()[0].clone();
            offscreen.set_a11y_listener(true);
            harness.settle(8);

            let mut tree = A11yTreeState::new();
            tree.sync(&offscreen);

            // 1. Closed state: shows chosen label ("Sarasa UI SC") with ZERO option DOM nodes.
            let (combo_id, combo) = tree.combobox();
            assert_eq!(combo.is_expanded, Some(false));
            assert_eq!(combo.value.as_deref(), Some("Sarasa UI SC"));
            assert_eq!(tree.live_role_count("ListBox"), 0);
            assert_eq!(
                tree.live_role_count("ListBoxOption"),
                0,
                "closed SearchChoiceField must not mount hidden catalogue or option DOM nodes"
            );

            // 2. Clicking the closed combobox opens the popup, scrolls to chosen option (#429),
            //    and mounts only the virtualized viewport window (<= 24 rows, never 600).
            let rects = tree.window_rects();
            let (x0, y0, x1, y1) = rects.get(&combo_id).copied().expect("combobox bounds");
            click_point(&mut harness, (x0 + x1) * 0.5, (y0 + y1) * 0.5);
            tree.sync(&offscreen);

            assert_eq!(tree.combobox().1.is_expanded, Some(true));
            assert_eq!(tree.live_role_count("ListBox"), 1);
            let open_mounted = tree.live_role_count("ListBoxOption");
            assert!(
                (1..=24).contains(&open_mounted),
                "opening 600-option popup must mount only virtualized window (<= 24), got {open_mounted}"
            );
            assert_eq!(tree.active_option_label(), Some("Sarasa UI SC"));
            assert_eq!(tree.option_is_selected("Sarasa UI SC"), Some(true));

            // 3. Filtering 600 options from "Sarasa UI SC" -> "Sarasa" -> "S" -> "" keeps
            //    mounted option DOM nodes strictly bounded (<= 24) at every step.
            select_all_and_replace(&mut harness, "Sarasa UI SC");
            tree.sync(&offscreen);
            assert_eq!(tree.live_role_count("ListBoxOption"), 1);
            assert_eq!(tree.active_option_label(), Some("Sarasa UI SC"));

            // Delete " UI SC" (6 chars) -> "Sarasa" (80 matches).
            backspace_times(&mut harness, 6);
            tree.sync(&offscreen);
            assert_eq!(tree.combobox().1.value.as_deref(), Some("Sarasa"));
            let sarasa_mounted = tree.live_role_count("ListBoxOption");
            assert!(
                (1..=24).contains(&sarasa_mounted),
                "filtering 'Sarasa' (80 matches) must mount <= 24 rows, got {sarasa_mounted}"
            );
            assert_eq!(tree.active_option_label(), Some("Sarasa Gothic 000"));

            // Delete "arasa" (5 chars) -> "S" (120+ matches).
            backspace_times(&mut harness, 5);
            tree.sync(&offscreen);
            assert_eq!(tree.combobox().1.value.as_deref(), Some("S"));
            let s_mounted = tree.live_role_count("ListBoxOption");
            assert!(
                (1..=24).contains(&s_mounted),
                "deleting down to 'S' (120+ matches) must mount <= 24 rows, got {s_mounted}"
            );

            // Delete final "S" -> "" (all 600 options match!).
            backspace_times(&mut harness, 1);
            tree.sync(&offscreen);
            assert_eq!(tree.combobox().1.value.as_deref(), Some(""));
            let all_mounted = tree.live_role_count("ListBoxOption");
            assert!(
                (1..=24).contains(&all_mounted),
                "deleting down to '' (all 600 fonts) must mount <= 24 rows, got {all_mounted}"
            );
            assert_eq!(tree.active_option_label(), Some("System default"));

            // 4. ArrowDown / ArrowUp / wrap-to-end / Home / Escape navigation.
            press_named(&mut harness, NamedKey::ArrowDown, KeyCode::ArrowDown);
            tree.sync(&offscreen);
            assert_eq!(tree.active_option_label(), Some("Adwaita Sans 000"));

            press_named(&mut harness, NamedKey::ArrowUp, KeyCode::ArrowUp);
            tree.sync(&offscreen);
            assert_eq!(tree.active_option_label(), Some("System default"));

            // Wrapping ArrowUp from index 0 jumps to index 599 and scrolls virtual window cleanly.
            press_named(&mut harness, NamedKey::ArrowUp, KeyCode::ArrowUp);
            tree.sync(&offscreen);
            let end_mounted = tree.live_role_count("ListBoxOption");
            assert!(
                (1..=24).contains(&end_mounted),
                "scrolling to end of 600 options must mount <= 24 rows, got {end_mounted}"
            );
            assert_eq!(tree.active_option_label(), Some("Zed Mono 129"));

            press_named(&mut harness, NamedKey::Home, KeyCode::Home);
            tree.sync(&offscreen);
            assert_eq!(tree.active_option_label(), Some("System default"));

            press_named(&mut harness, NamedKey::PageDown, KeyCode::PageDown);
            tree.sync(&offscreen);
            assert_eq!(tree.active_option_label(), Some("Adwaita Sans 009"));

            press_named(&mut harness, NamedKey::PageUp, KeyCode::PageUp);
            tree.sync(&offscreen);
            assert_eq!(tree.active_option_label(), Some("System default"));

            press_named(&mut harness, NamedKey::End, KeyCode::End);
            tree.sync(&offscreen);
            assert_eq!(tree.active_option_label(), Some("Zed Mono 129"));

            // Escape cancels query, closes popup, and restores chosen label without mutating value.
            press_named(&mut harness, NamedKey::Escape, KeyCode::Escape);
            harness.settle(16);
            tree.sync(&offscreen);
            let sig = value_driver.borrow().expect("value signal");
            assert_eq!(tree.combobox().1.is_expanded, Some(false));
            assert_eq!(tree.combobox().1.value.as_deref(), Some("Sarasa UI SC"));
            assert_eq!(sig.get_untracked(), "Sarasa UI SC");
            assert!(changes_driver.borrow().is_empty());

            // 5. Re-open by clicking the right-end chevron zone, filter "Noto" (150 matches),
            //    ArrowDown + Enter commits value & on_change.
            click_point(&mut harness, x1 - 8.0, (y0 + y1) * 0.5);
            tree.sync(&offscreen);
            assert_eq!(tree.combobox().1.is_expanded, Some(true));

            select_all_and_replace(&mut harness, "Noto");
            tree.sync(&offscreen);
            let noto_mounted = tree.live_role_count("ListBoxOption");
            assert!(
                (1..=24).contains(&noto_mounted),
                "filtering 'Noto' (150 matches) must mount <= 24 rows, got {noto_mounted}"
            );
            assert_eq!(tree.active_option_label(), Some("Noto Sans 000"));

            press_named(&mut harness, NamedKey::ArrowDown, KeyCode::ArrowDown);
            tree.sync(&offscreen);
            assert_eq!(tree.active_option_label(), Some("Noto Sans 001"));

            press_named(&mut harness, NamedKey::Enter, KeyCode::Enter);
            harness.settle(16);
            tree.sync(&offscreen);
            assert_eq!(sig.get_untracked(), "font-noto-001");
            assert_eq!(*changes_driver.borrow(), vec!["font-noto-001".to_owned()]);
            assert_eq!(tree.combobox().1.is_expanded, Some(false));
            assert_eq!(tree.combobox().1.value.as_deref(), Some("Noto Sans 001"));

            // 6. Zero matches shows empty_text with 0 option nodes; outside press dismisses & restores.
            select_all_and_replace(&mut harness, "zzzz-missing");
            tree.sync(&offscreen);
            assert_eq!(tree.combobox().1.is_expanded, Some(true));
            assert_eq!(tree.live_role_count("ListBoxOption"), 0);
            assert!(tree.contains_live_text("No matching fonts"));

            click_point(&mut harness, 600.0, 500.0);
            harness.settle(16);
            tree.sync(&offscreen);
            assert_eq!(tree.combobox().1.is_expanded, Some(false));
            assert_eq!(tree.combobox().1.value.as_deref(), Some("Noto Sans 001"));

            // 7. Clicking an option row directly in the virtualized popup commits & closes cleanly.
            click_point(&mut harness, (x0 + x1) * 0.5, (y0 + y1) * 0.5);
            tree.sync(&offscreen);
            let (ox0, oy0, ox1, oy1) = tree
                .option_rect("Noto Sans 002")
                .expect("Noto Sans 002 visible in virtualized window around Noto Sans 001");
            click_point(&mut harness, (ox0 + ox1) * 0.5, (oy0 + oy1) * 0.5);
            harness.settle(16);
            tree.sync(&offscreen);
            assert_eq!(sig.get_untracked(), "font-noto-002");
            assert_eq!(
                *changes_driver.borrow(),
                vec!["font-noto-001".to_owned(), "font-noto-002".to_owned()]
            );
            assert_eq!(tree.combobox().1.is_expanded, Some(false));
            assert_eq!(tree.combobox().1.value.as_deref(), Some("Noto Sans 002"));

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_title("search-choice-virtualized-test")
            .with_fonts(Fonts::shipped_only())
            .with_size(640.0, 520.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = HeadlessSink::default();
                Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn Renderer>)
            }))
            .run_on(driver, move || {
                let value = RwSignal::new_local("Sarasa UI SC".to_owned());
                let options = RwSignal::new_local(build_600_font_options());
                let log = Rc::clone(&changes_view);
                *value_view.borrow_mut() = Some(value);
                view! {
                    column(style:padding = "16px", style:width = "280px") {
                        SearchChoiceField(
                            value = value,
                            options = options,
                            placeholder = "Select font",
                            empty_text = "No matching fonts".to_owned(),
                            label = "Font family",
                            on_change = UnsyncCallback::new(move |chosen: String| {
                                log.borrow_mut().push(chosen);
                            }),
                        )
                    }
                }
            })
            .expect("headless SearchChoiceField test ran");
    }
}
