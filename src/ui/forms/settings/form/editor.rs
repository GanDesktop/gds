use crate::appearance::tokens;
use gds_uim::el;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IndexPath, Sizable,
        input::{InputEvent, InputState, Textarea, TextareaState},
        select::{SearchableVec, SelectEvent, SelectState},
        slider::SliderValue,
        switch::Switch,
    },
    prelude::*,
    *,
};
use serde_json::Value;

use super::{Choice, Field, Kind, Model};
use crate::{
    app::AppExt as _,
    i18n,
    ui::{self, settings::*},
};

#[derive(Clone)]
pub struct Changed<T> {
    pub value: T,
    pub field: &'static str,
}

enum Control {
    Toggle,
    Number(NumberControl),
    Text(Entity<InputState>),
    StringList(Entity<TextareaState>),
    Choice {
        state: Entity<SelectState<SearchableVec<Choice>>>,
        choices: Vec<Choice>,
    },
}

/// Native field state and typed changes; the owning app applies and persists them.
pub struct Editor<T: Model> {
    value: T,
    fields: Vec<Field<T>>,
    controls: Vec<Control>,
    errors: Vec<Option<String>>,
    language: i18n::Language,
    disabled: bool,
    _subscriptions: Vec<Subscription>,
}

impl<T: Model> EventEmitter<Changed<T>> for Editor<T> {}

impl<T: Model> Editor<T> {
    pub fn new(value: T, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fields = T::fields();
        let mut subscriptions = Vec::new();
        let controls = fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let control = match field.kind {
                    Kind::Toggle => Control::Toggle,
                    Kind::Number(number) => {
                        let value =
                            field.effective(&value).as_f64().unwrap_or(number.min) * number.scale;
                        let control = NumberControl::new(value as f32, number.range(), cx);
                        subscriptions.push(cx.observe_in(
                            &control.state,
                            window,
                            move |this, state, window, cx| {
                                let value = state.read(cx).value();
                                let Control::Number(control) = &this.controls[index] else {
                                    return;
                                };
                                if !control.notification_is_programmatic(value) {
                                    this.number_changed(index, value, window, cx);
                                }
                            },
                        ));
                        Control::Number(control)
                    }
                    Kind::Text => {
                        let value = (field.get)(&value);
                        let state = cx.new(|cx| {
                            InputState::new(window, cx).default_value(editable_text(&value))
                        });
                        subscriptions.push(cx.subscribe_in(
                            &state,
                            window,
                            move |this, input, event: &InputEvent, window, cx| {
                                if matches!(event, InputEvent::Change) {
                                    let text = input.read(cx).value().to_string();
                                    let value = if this.fields[index].nullable && text.is_empty() {
                                        Value::Null
                                    } else {
                                        Value::String(text)
                                    };
                                    this.change(index, value, window, cx);
                                }
                            },
                        ));
                        Control::Text(state)
                    }
                    Kind::StringList => {
                        let value = (field.get)(&value);
                        let state = cx.new(|cx| {
                            TextareaState::new(window, cx)
                                .auto_grow(2, 5)
                                .default_value(editable_text(&value))
                        });
                        subscriptions.push(cx.subscribe_in(
                            &state,
                            window,
                            move |this, input, event: &InputEvent, window, cx| {
                                if matches!(event, InputEvent::Change) {
                                    let value = Value::Array(
                                        input
                                            .read(cx)
                                            .value()
                                            .split('\n')
                                            .filter(|line| !line.is_empty())
                                            .map(|line| Value::String(line.to_owned()))
                                            .collect(),
                                    );
                                    this.change(index, value, window, cx);
                                }
                            },
                        ));
                        Control::StringList(state)
                    }
                    Kind::Font | Kind::Language | Kind::Choice(_) => {
                        let choices = field.choices(&value, cx);
                        let current = (field.get)(&value);
                        let selected = choices
                            .iter()
                            .position(|choice| choice.value == current)
                            .map(IndexPath::new);
                        let state = cx.new(|cx| {
                            SelectState::new(
                                SearchableVec::new(choices.clone()),
                                selected,
                                window,
                                cx,
                            )
                            .searchable(true)
                        });
                        subscriptions.push(cx.subscribe_in(
                            &state,
                            window,
                            move |this,
                                  _,
                                  event: &SelectEvent<SearchableVec<Choice>>,
                                  window,
                                  cx| {
                                if let SelectEvent::Confirm(Some(value)) = event {
                                    this.change(index, value.clone(), window, cx);
                                }
                            },
                        ));
                        Control::Choice { state, choices }
                    }
                };
                control
            })
            .collect();
        Self {
            errors: vec![None; fields.len()],
            value,
            fields,
            controls,
            language: cx.gds().i18n().language(),
            disabled: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    /// Application acceptance or rollback synchronizes native widgets without emitting changes.
    pub fn set_value(&mut self, value: T, window: &mut Window, cx: &mut Context<Self>) {
        if self.value == value && self.language == cx.gds().i18n().language() {
            return;
        }
        self.value = value;
        self.errors.fill(None);
        self.sync(window, cx);
        cx.notify();
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.language = cx.gds().i18n().language();
        for (field, control) in self.fields.iter().zip(&mut self.controls) {
            let current = (field.get)(&self.value);
            match control {
                Control::Number(control) => {
                    let Kind::Number(number) = field.kind else {
                        unreachable!()
                    };
                    let display =
                        field.effective(&self.value).as_f64().unwrap_or(number.min) * number.scale;
                    control.sync(display as f32, window, cx);
                }
                Control::StringList(state) => {
                    let text = editable_text(&current);
                    if state.read(cx).value().as_str() != text {
                        state.update(cx, |state, cx| state.set_value(text, window, cx));
                    }
                }
                Control::Text(state) => {
                    let text = editable_text(&current);
                    if state.read(cx).value().as_str() != text.as_str() {
                        state.update(cx, |state, cx| state.set_value(text, window, cx));
                    }
                }
                Control::Choice { state, choices } => {
                    Self::sync_choice(field, &self.value, &current, state, choices, window, cx);
                }
                Control::Toggle => {}
            }
        }
    }

    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.language = cx.gds().i18n().language();
        for (field, control) in self.fields.iter().zip(&mut self.controls) {
            if let Control::Choice { state, choices } = control {
                let current = (field.get)(&self.value);
                Self::sync_choice(field, &self.value, &current, state, choices, window, cx);
            }
        }
    }

    fn sync_choice(
        field: &Field<T>,
        model: &T,
        current: &Value,
        state: &Entity<SelectState<SearchableVec<Choice>>>,
        choices: &mut Vec<Choice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = field.choices(model, cx);
        if *choices != next {
            *choices = next;
            state.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(choices.clone()), window, cx)
            });
        }
        if state.read(cx).selected_value() != Some(current) {
            let index = choices
                .iter()
                .position(|choice| &choice.value == current)
                .map(IndexPath::new);
            state.update(cx, |state, cx| state.set_selected_index(index, window, cx));
        }
    }

    fn change(&mut self, index: usize, value: Value, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            self.sync(window, cx);
            cx.notify();
            return;
        }
        let field = &self.fields[index];
        let mut next = self.value.clone();
        let result = (field.set)(&mut next, value).and_then(|_| next.validate());
        match result {
            Err(error) => self.errors[index] = Some(error),
            Ok(()) => {
                self.errors[index] = None;
                if next != self.value {
                    self.value = next;
                    cx.emit(Changed {
                        value: self.value.clone(),
                        field: field.id,
                    });
                }
            }
        }
        cx.notify();
    }

    fn number_changed(
        &mut self,
        index: usize,
        value: SliderValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            self.sync(window, cx);
            cx.notify();
            return;
        }
        let SliderValue::Single(display) = value else {
            return;
        };
        let field = &self.fields[index];
        let Kind::Number(number) = field.kind else {
            return;
        };
        let Control::Number(control) = &self.controls[index] else {
            return;
        };
        let current = field.effective(&self.value).as_f64().unwrap_or(number.min) * number.scale;
        if control.range.equivalent(current as f32, display) {
            return;
        }
        match number.value(display, cx) {
            Ok(value) => self.change(index, value, window, cx),
            Err(error) => {
                self.errors[index] = Some(error);
                cx.notify();
            }
        }
    }

    #[cfg(all(test, feature = "gui-tests"))]
    pub(super) fn number_control(&self, id: &str) -> &NumberControl {
        let index = self
            .fields
            .iter()
            .position(|field| field.id == id)
            .expect("settings field");
        match &self.controls[index] {
            Control::Number(control) => control,
            _ => panic!("field {id} is not a numeric control"),
        }
    }

    fn reset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let value = (self.fields[index].get)(&T::default());
        self.change(index, value, window, cx);
        self.sync(window, cx);
    }

    fn control(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let field = &self.fields[index];
        let id = SharedString::from(format!("config-field-{}", field.id));
        let label = i18n::text(field.label, cx);
        match &self.controls[index] {
            Control::Toggle => Switch::new(id)
                .checked(field.effective(&self.value).as_bool().unwrap_or_default())
                .disabled(self.disabled)
                .small()
                .accessibility_label(label)
                .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                    this.change(index, Value::Bool(*checked), window, cx)
                }))
                .into_any_element(),
            Control::StringList(state) => el! {
                .div { # $id.clone(); test_support; w_full; } [
                    $(Textarea::new(state)
                        .accessibility_id(id)
                        .small()
                        .disabled(self.disabled)
                        .w_full()
                        .aria_label(label)
                        .bg(cx.gds().appearance().theme().colors().content)),
                ]
            }
            .into_any_element(),
            Control::Text(state) => ui::field::input(state, cx)
                .id(id)
                .small()
                .disabled(self.disabled)
                .w_full()
                .aria_label(label)
                .into_any_element(),
            Control::Choice { state, .. } => ui::field::select(state, cx)
                .id(id)
                .small()
                .disabled(self.disabled)
                .w_full()
                .accessibility_label(label)
                .search_placeholder(i18n::text("form-search", cx))
                .into_any_element(),
            Control::Number(control) => {
                let Kind::Number(number) = field.kind else {
                    unreachable!()
                };
                let display =
                    field.effective(&self.value).as_f64().unwrap_or(number.min) * number.scale;
                control
                    .element(
                        id.into(),
                        label,
                        number.label(display as f32).into(),
                        self.disabled,
                    )
                    .into_any_element()
            }
        }
    }
}

impl<T: Model> Render for Editor<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.language != cx.gds().i18n().language() {
            self.sync_language(window, cx);
        }
        let mut rows = Vec::new();
        let mut group = "";
        let defaults = T::default();
        for index in 0..self.fields.len() {
            let field = &self.fields[index];
            if field.group != group {
                group = field.group;
                if !group.is_empty() {
                    rows.push(
                        div()
                            .pt_4()
                            .text_size(tokens::SMALL)
                            .text_color(cx.theme().muted_foreground)
                            .child(i18n::text(group, cx))
                            .into_any_element(),
                    );
                }
            }
            let mut title = div()
                .id(SharedString::from(format!("config-title-{}", field.id)))
                .child(i18n::text(field.label, cx));
            if !field.help.is_empty() {
                let help = i18n::text(field.help, cx);
                title = title.hoverable_tooltip(move |window, cx| {
                    ui::tooltip::view(help.clone(), window, cx)
                });
            }
            let modified = field.is_modified(&self.value, &defaults);
            let reset = modified.then(|| {
                ui::button::Button::icon_button(SharedString::from(format!(
                    "config-reset-{}",
                    field.id
                )))
                .control_icon(gpui_kit::assets::IconName::RotateCcw)
                .accessibility_label(i18n::text("form-reset", cx))
                .tooltip(i18n::text("form-reset", cx))
                .disabled(self.disabled)
                .on_click(cx.listener(move |this, _, window, cx| this.reset(index, window, cx)))
                .into_any_element()
            });
            let control = self.control(index, cx);
            let row = el! {
                .SettingRow::default [
                    .SettingLabel::default [title],
                    .SettingActions::default [
                        .SettingReset::default [..reset],
                        .SettingControl::default [control],
                    ],
                ]
            };
            rows.push(
                div()
                    .id(SharedString::from(format!("config-row-{}", field.id)))
                    .test_support()
                    .child(row)
                    .children(self.errors[index].as_ref().map(|error| {
                        div()
                            .id(SharedString::from(format!("config-error-{}", field.id)))
                            .test_support()
                            .text_size(tokens::SMALL)
                            .text_color(cx.theme().danger)
                            .child(error.clone())
                    }))
                    .into_any_element(),
            );
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .when(self.disabled, |this| this.opacity(0.6))
            .children(rows)
    }
}

fn editable_text(value: &Value) -> String {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => value.as_str().unwrap_or_default().to_owned(),
    }
}
