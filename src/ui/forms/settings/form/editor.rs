use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal, UnsyncCallback, on_cleanup_local};
use zgui::view::ShowProps;
use zgui::{component, view};
use zgui_ui::{
    SelectContentProps, SelectItemProps, SelectProps, SelectTriggerProps, SelectValueProps,
};
use zgui_ui_primitives::{Binding, Controllable};

use super::{Choice, Field, Kind, Model};
use crate::app::Gds;
use crate::ui::button::IconButtonProps;
use crate::ui::forms::field::{
    FieldLabelProps, InputFieldProps, SearchChoiceFieldProps, SearchOption, TextareaFieldProps,
};
use crate::ui::forms::switch::ToggleSwitchProps;
use crate::ui::icon::IconName;
use crate::ui::settings::{
    NumberControlProps, SettingActionsProps, SettingControlProps, SettingDescriptionProps,
    SettingLabelProps, SettingResetProps, SettingRowProps, install_settings_stylesheets,
};

/// Emitted whenever a validated settings form edit produces a new model value.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed<T> {
    pub value: T,
    pub field: &'static str,
}

impl<T> Changed<T> {
    #[must_use]
    pub const fn new(value: T, field: &'static str) -> Self {
        Self { value, field }
    }
}

/// Declarative settings form view derived for any [`Model`].
pub struct Editor<T: Model> {
    value: Binding<T>,
    on_change: Option<UnsyncCallback<Changed<T>>>,
    disabled: Signal<bool, LocalStorage>,
    filter: Signal<String, LocalStorage>,
    reset_focus: Option<NodeRef>,
}

impl<T: Model> Editor<T> {
    #[must_use]
    pub fn new(value: impl Into<Binding<T>>) -> Self {
        Self {
            value: value.into(),
            on_change: None,
            disabled: Signal::stored_local(false),
            filter: Signal::stored_local(String::new()),
            reset_focus: None,
        }
    }

    #[must_use]
    pub fn on_change(mut self, callback: impl Fn(Changed<T>) + 'static) -> Self {
        self.on_change = Some(UnsyncCallback::new(callback));
        self
    }

    #[must_use]
    pub fn disabled(mut self, disabled: impl Into<Signal<bool, LocalStorage>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Filter rendered fields by matching case-insensitive query tokens against field
    /// labels, descriptions, groups, and identifiers.
    #[must_use]
    pub fn filter(mut self, query: impl Into<Signal<String, LocalStorage>>) -> Self {
        self.filter = query.into();
        self
    }

    #[must_use]
    pub fn reset_focus(mut self, focus: NodeRef) -> Self {
        self.reset_focus = Some(focus);
        self
    }

    /// Build the settings editor view tree.
    pub fn view(self) -> AnyView
    where
        T: Serialize + DeserializeOwned,
    {
        let active_field: Rc<Cell<&'static str>> = Rc::new(Cell::new(""));
        let active_for_cb = Rc::clone(&active_field);
        let forward = self.on_change.map(|cb| {
            UnsyncCallback::new(move |value: T| {
                cb.run(Changed {
                    value,
                    field: active_for_cb.get(),
                });
            })
        });
        let state = Controllable::new(self.value, T::default(), forward);
        AnyView::new(view! {
            EditorBody(
                state = state,
                active_field = active_field,
                disabled = self.disabled,
                filter = self.filter,
                reset_focus = self.reset_focus,
            )
        })
    }
}

impl<T: Model + Serialize + DeserializeOwned> IntoView for Editor<T> {
    type Output = AnyView;

    fn into_view(self) -> Self::Output {
        self.view()
    }
}

pub(super) fn field_matches_query<T: Model>(field: &Field<T>, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return true;
    }
    let gds = Gds::current();
    let i18n = gds.i18n();
    let label = i18n.text(field.label).to_lowercase();
    let help = if field.help.is_empty() {
        String::new()
    } else {
        i18n.text(field.help).to_lowercase()
    };
    let group = if field.group.is_empty() {
        String::new()
    } else {
        i18n.text(field.group).to_lowercase()
    };
    let id = field.id.to_lowercase();

    trimmed.split_whitespace().all(|token| {
        let needle = token.to_lowercase();
        label.contains(&needle)
            || help.contains(&needle)
            || group.contains(&needle)
            || id.contains(&needle)
    })
}

#[component]
fn EditorBody<T: Model + Serialize + DeserializeOwned>(
    state: Controllable<T>,
    active_field: Rc<Cell<&'static str>>,
    disabled: Signal<bool, LocalStorage>,
    filter: Signal<String, LocalStorage>,
    reset_focus: Option<NodeRef>,
) -> impl IntoView {
    install_settings_stylesheets();
    let defaults = Rc::new(T::default());

    let mut groups: Vec<(&'static str, Vec<Rc<Field<T>>>)> = Vec::new();
    for field in T::fields() {
        let field = Rc::new(field);
        if let Some((last_group, items)) = groups.last_mut()
            && *last_group == field.group
        {
            items.push(field);
        } else {
            groups.push((field.group, vec![field]));
        }
    }

    let sections: Vec<AnyView> = groups
        .into_iter()
        .map(|(group, fields)| {
            let group_fields = Rc::new(fields);
            let section_fields = Rc::clone(&group_fields);
            let group_visible = Signal::derive_local(move || {
                let query = filter.get();
                section_fields
                    .iter()
                    .any(|field| field_matches_query(field, &query))
            });

            let row_views: Vec<AnyView> = group_fields
                .iter()
                .map(|field| {
                    let field_filter = Rc::clone(field);
                    let row_visible = move || field_matches_query(&field_filter, &filter.get());
                    let field_row = Rc::clone(field);
                    let defaults_row = Rc::clone(&defaults);
                    let active_row = Rc::clone(&active_field);
                    AnyView::new(view! {
                        Show(when = row_visible, fallback = || ()) {
                            EditorFieldRow(
                                state = state,
                                active_field = Rc::clone(&active_row),
                                field = Rc::clone(&field_row),
                                defaults = Rc::clone(&defaults_row),
                                disabled = disabled,
                                reset_focus = reset_focus,
                            )
                        }
                    })
                })
                .collect();

            AnyView::new(view! {
                column(class = "gds-setting-section") {
                    {(!group.is_empty()).then(|| view! {
                        Show(when = move || group_visible.get(), fallback = || ()) {
                            text(class = "gds-setting-group") {{move || Gds::current().i18n().text(group)}}
                        }
                    }.into_view())}
                    {row_views}
                }
            })
        })
        .collect();

    view! {
        column(
            class = "gds-setting-form",
            attr:data-disabled = move || disabled.get().then(|| "true".to_owned()),
        ) {
            {sections}
        }
    }
}

#[component]
fn EditorFieldRow<T: Model + Serialize + DeserializeOwned>(
    state: Controllable<T>,
    active_field: Rc<Cell<&'static str>>,
    field: Rc<Field<T>>,
    defaults: Rc<T>,
    disabled: Signal<bool, LocalStorage>,
    reset_focus: Option<NodeRef>,
) -> impl IntoView {
    let error: RwSignal<Option<String>, LocalStorage> = RwSignal::new_local(None);
    let label_key = field.label;
    let help_key = field.help;
    let field_id = field.id;
    let title_ref = NodeRef::new();

    // Clear stale validation errors when an external signal update changes this field's value,
    // without clearing errors produced by rejecting an invalid local edit.
    let field_sync = Rc::clone(&field);
    let last_json = RefCell::new((field_sync.get)(&state.get_untracked()));
    let sync_error = RenderEffect::new(move |_| {
        let current = (field_sync.get)(&state.get());
        let changed = *last_json.borrow() != current;
        if changed {
            *last_json.borrow_mut() = current;
            error.set(None);
        }
    });
    on_cleanup_local(move || drop(sync_error));

    let apply_json = {
        let field = Rc::clone(&field);
        let active_field = Rc::clone(&active_field);
        move |json: Value| {
            if disabled.get_untracked() {
                return;
            }
            let mut next = state.get_untracked();
            if let Err(err) = (field.set)(&mut next, json) {
                error.set(Some(err));
                return;
            }
            if let Err(err) = next.validate() {
                error.set(Some(err));
                return;
            }
            error.set(None);
            active_field.set(field_id);
            state.set(next);
        }
    };

    let modified = {
        let field = Rc::clone(&field);
        let defaults = Rc::clone(&defaults);
        Signal::derive_local(move || state.get().is_modified(&defaults, &field))
    };

    let reset = {
        let field = Rc::clone(&field);
        let defaults = Rc::clone(&defaults);
        let active_field = Rc::clone(&active_field);
        move |_: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
            if disabled.get_untracked() {
                return;
            }
            let default_json = (field.get)(&defaults);
            let mut next = state.get_untracked();
            if (field.set)(&mut next, default_json).is_ok() && next.validate().is_ok() {
                error.set(None);
                active_field.set(field_id);
                state.set(next);
            }
            if let Some(target) = reset_focus {
                target.focus();
            }
        }
    };

    let control_view = render_field_control(state, &field, disabled, error, title_ref, apply_json);

    view! {
        SettingRow(attr:data-field = field_id) {
            SettingLabel {
                FieldLabel(node_ref = title_ref) {{move || Gds::current().i18n().text(label_key)}}
                if move || !help_key.is_empty() {
                    SettingDescription {{move || Gds::current().i18n().text(help_key)}}
                } else {}
                {move || error.get().map(|msg| view! {
                    text(class = "gds-setting-error") {{msg}}
                }.into_view())}
            }
            SettingActions {
                SettingReset {
                    if move || modified.get() {
                        IconButton(
                            icon = IconName::RotateCcw,
                            label = Signal::derive_local(|| Gds::current().i18n().text("form-reset")),
                            disabled = disabled,
                            on:click = reset.clone(),
                        )
                    } else {}
                }
                SettingControl {
                    {control_view}
                }
            }
        }
    }
}

trait FieldModifiedExt<T> {
    fn is_modified(&self, defaults: &T, field: &Field<T>) -> bool;
}

impl<T> FieldModifiedExt<T> for T {
    fn is_modified(&self, defaults: &T, field: &Field<T>) -> bool {
        field.is_modified(self, defaults)
    }
}

fn render_field_control<T: Model + Serialize + DeserializeOwned>(
    state: Controllable<T>,
    field: &Rc<Field<T>>,
    disabled: Signal<bool, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
    title_ref: NodeRef,
    apply_json: impl Fn(Value) + Clone + 'static,
) -> AnyView {
    let _ = field.label;
    match field.kind {
        Kind::Toggle => {
            let field = Rc::clone(field);
            let binding = Binding::controlled(
                Signal::derive_local(move || {
                    field.effective(&state.get()).as_bool().unwrap_or(false)
                }),
                move |next: bool| apply_json(Value::Bool(next)),
            );
            AnyView::new(view! {
                ToggleSwitch(
                    checked = binding,
                    disabled = disabled,
                    labelled_by = Some(title_ref),
                )
            })
        }
        Kind::Number(number) => {
            let range = number.range();
            let field = Rc::clone(field);
            let read_display = Signal::derive_local(move || {
                let raw = field.effective(&state.get()).as_f64().unwrap_or(number.min);
                f64::from(range.quantize((raw * number.scale) as f32))
            });
            let apply_for_slider = apply_json.clone();
            let binding = Binding::controlled(read_display, move |display: f64| {
                if disabled.get_untracked() {
                    return;
                }
                match number.value(display) {
                    Ok(val) => apply_for_slider(val),
                    Err(msg) => error.set(Some(msg)),
                }
            });
            AnyView::new(view! {
                row(class = "gds-setting-number-row") {
                    NumberControl(
                        value = binding,
                        range = range,
                        disabled = disabled,
                        labelled_by = Some(title_ref),
                    )
                    text(class = "gds-setting-number-value") {{move || number.label(read_display.get())}}
                }
            })
        }
        Kind::Text => {
            let field = Rc::clone(field);
            let nullable = field.nullable;
            let binding = Binding::controlled(
                Signal::derive_local(move || {
                    (field.get)(&state.get())
                        .as_str()
                        .map_or_else(String::new, str::to_owned)
                }),
                move |text: String| {
                    let val = if nullable && text.is_empty() {
                        Value::Null
                    } else {
                        Value::String(text)
                    };
                    apply_json(val);
                },
            );
            let invalid = Signal::derive_local(move || error.get().is_some());
            AnyView::new(view! {
                InputField(
                    value = binding,
                    compact = true,
                    disabled = disabled,
                    invalid = invalid,
                    labelled_by = Some(title_ref),
                )
            })
        }
        Kind::StringList => {
            let field = Rc::clone(field);
            let binding = Binding::controlled(
                Signal::derive_local(move || {
                    (field.get)(&state.get())
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default()
                }),
                move |text: String| {
                    let list: Vec<Value> = text
                        .lines()
                        .filter(|line| !line.is_empty())
                        .map(|line| Value::String(line.to_owned()))
                        .collect();
                    apply_json(Value::Array(list));
                },
            );
            let invalid = Signal::derive_local(move || error.get().is_some());
            AnyView::new(view! {
                TextareaField(
                    value = binding,
                    disabled = disabled,
                    invalid = invalid,
                    labelled_by = Some(title_ref),
                )
            })
        }
        Kind::Font | Kind::Language | Kind::Choice(_) => {
            let field_read = Rc::clone(field);
            let field_choices = Rc::clone(field);
            let choices = Signal::derive_local(move || field_choices.choices(&state.get()));
            let value_binding = Binding::controlled(
                Signal::derive_local(move || (field_read.get)(&state.get()).to_string()),
                move |encoded: String| {
                    if let Ok(json) = serde_json::from_str::<Value>(&encoded) {
                        apply_json(json);
                    }
                },
            );

            let searchable = matches!(field.kind, Kind::Font | Kind::Language)
                || choices
                    .get_untracked()
                    .iter()
                    .any(|choice| !choice.terms().is_empty());

            if searchable {
                let search_options = Signal::derive_local(move || {
                    choices
                        .get()
                        .into_iter()
                        .map(|choice| {
                            let terms = choice.terms().split_whitespace();
                            SearchOption::new(choice.value.to_string(), choice.title)
                                .with_keywords(terms)
                        })
                        .collect()
                });
                AnyView::new(view! {
                    SearchChoiceField(
                        value = value_binding,
                        options = search_options,
                        disabled = disabled,
                        labelled_by = Some(title_ref),
                    )
                })
            } else {
                AnyView::new(view! {
                    Select(value = value_binding) {
                        SelectTrigger(
                            disabled = disabled,
                            labelled_by = Some(title_ref),
                        ) {
                            SelectValue()
                        }
                        SelectContent {
                            for choice in move || choices.get(), key = |c| (c.value.to_string(), c.title.clone()) {
                                {select_choice_item(choice)}
                            }
                        }
                    }
                })
            }
        }
    }
}

fn select_choice_item(choice: Choice) -> AnyView {
    let encoded = choice.value.to_string();
    let title = choice.title;
    let title_sig = Signal::stored_local(title.clone());
    AnyView::new(view! {
        SelectItem(value = encoded, text = title) {
            {move || title_sig.get()}
        }
    })
}
