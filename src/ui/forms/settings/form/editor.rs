//! Native controls over the existing configuration schema.
use super::{Field, Kind, Model, Value};
use crate::ui::{
    Icon, IconButton,
    field::{SearchOption, search_choice},
    settings::{SettingsActions, SettingsControl, SettingsLabel, SettingsReset, SettingsRow},
};
use anti_webui::prelude::*;
use gds_uim::el;
use std::rc::Rc;

pub struct Editor<T: Model> {
    state: Signal<T>,
    on_change: Rc<dyn Fn(T)>,
    disabled: bool,
    reset_focus: Option<NodeRef>,
}
impl<T: Model> Editor<T> {
    pub fn new(state: Signal<T>) -> Self {
        Self {
            state,
            on_change: Rc::new(move |next| state.set(next)),
            disabled: false,
            reset_focus: None,
        }
    }
    pub fn on_change(mut self, callback: impl Fn(T) + 'static) -> Self {
        self.on_change = Rc::new(callback);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn reset_focus(mut self, focus: NodeRef) -> Self {
        self.reset_focus = Some(focus);
        self
    }
}
impl<T: Model> IntoView for Editor<T> {
    fn into_view(self) -> View {
        let mut rows = Vec::new();
        let mut group = "";
        for field in T::fields() {
            if field.group != group {
                group = field.group;
                if !group.is_empty() {
                    rows.push(
                        el! { (move () => group.crate::i18n::text).text.{
                            pt: 16.0_f32;
                            dyn_style: s => $(s.text_color(theme().text_muted));
                        } }
                        .into_view(),
                    );
                }
            }
            rows.push(
                Control {
                    state: self.state,
                    field: Rc::new(field),
                    error: signal(None),
                    on_change: self.on_change.clone(),
                    disabled: self.disabled,
                    reset_focus: self.reset_focus,
                }
                .view()
                .into_view(),
            );
        }
        el! { .div.{ flex; flex_col; w_full }.[..rows] }.into_view()
    }
}

struct Control<T: Model> {
    state: Signal<T>,
    field: Rc<Field<T>>,
    error: Signal<Option<String>>,
    on_change: Rc<dyn Fn(T)>,
    disabled: bool,
    reset_focus: Option<NodeRef>,
}
impl<T: Model> Clone for Control<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state,
            field: self.field.clone(),
            error: self.error,
            on_change: self.on_change.clone(),
            disabled: self.disabled,
            reset_focus: self.reset_focus,
        }
    }
}
impl<T: Model> Control<T> {
    fn apply(&self, value: Value) {
        let mut next = self.state.get_untracked();
        match (self.field.set)(&mut next, value).and_then(|()| next.validate()) {
            Ok(()) => {
                self.error.set(None);
                (self.on_change)(next);
            }
            Err(error) => self.error.set(Some(error)),
        }
    }
    fn reset(&self) {
        if !self.disabled {
            self.apply((self.field.get)(&T::default()));
            if let Some(focus) = self.reset_focus {
                focus.focus();
            }
        }
    }
    fn value(&self) -> Value {
        self.state.with(|state| self.field.effective(state))
    }
    fn view(self) -> impl IntoView {
        let id = self.field.id;
        let label = self.field.label;
        let help = self.field.help;
        let error = self.error;
        let control = self.clone().input();
        let reset = self.clone();
        let modified = self.clone();
        let title = el! { .div.[ (move () => label.crate::i18n::text).text ] };
        let title = if help.is_empty() {
            title
        } else {
            tooltip(title, crate::i18n::text(help))
        };
        el! { .SettingsRow::default.[
            .SettingsLabel::default.[title],
            .SettingsActions::default.[
                .SettingsReset::default.[
                    (move () => modified->state.{ with: state => $(modified.field.is_modified(state, &T::default())) }, move () => ${
                        let reset = reset.clone();
                        el! { format!("reset-{id}").IconButton::new.{ icon: Icon::Reset; disabled: reset->disabled; label_fn: () => "form-reset".crate::i18n::text; on_click: move _ => &reset.{ reset } } }
                    }).show,
                ],
            .SettingsControl::default.[ .div.{flex;flex_col;items_end;w_full}.[ control,
                    (move () => ${
                        el! { .div.[if let Some(problem)=error.{get} [problem.text.{text_size:12.0_f32;dyn_style:s=>$(s.text_color(theme().danger_text))}]] }
                    }).dyn_view,
                ] ],
            ],
        ] }
    }
    fn input(self) -> View {
        match self.field.kind {
            Kind::Toggle => {
                let getter = self.clone();
                let setter = self.clone();
                let binding = Binding::new(
                    move || getter.value().as_bool().unwrap_or(false),
                    move |value| setter.apply(Value::Bool(value)),
                );
                crate::ui::settings::compact_switch(binding, move || self.disabled).into_view()
            }
            Kind::Number(number) => {
                let value = signal(untrack(|| {
                    self.value().as_f64().unwrap_or(number.min) * number.scale
                }));
                let getter = self.clone();
                effect(move || {
                    value.set_if_changed(
                        getter.value().as_f64().unwrap_or(number.min) * number.scale,
                    );
                });
                let setter = self.clone();
                effect(move || {
                    let current = value.get();
                    let saved =
                        untrack(|| setter.value().as_f64().unwrap_or(number.min) * number.scale);
                    if current != saved {
                        match number.value(current as f32, &crate::app::Gds::current()) {
                            Ok(value) => setter.apply(value),
                            Err(error) => setter.error.set(Some(error)),
                        }
                    }
                });
                let reading = value;
                let slider_value = Binding::new(
                    move || value.get() as f32,
                    move |next| value.set(f64::from(next)),
                );
                let disabled = self.disabled;
                el! { .div.{flex;items_center;w_full;gap:8.0_f32}.[
                    $(crate::ui::settings::setting_slider(slider_value, crate::ui::settings::NumberRange {
                        min: (number.min * number.scale) as f32,
                        max: (number.max * number.scale) as f32,
                        step: (number.step * number.scale) as f32,
                    }, disabled)).{flex_1;min_w:0.0_f32},
                    (move()=>$(format!("{}{}", reading.get(), number.unit))).text.{
                        w:72.0_f32; text_align:TextAlign::Right;
                        dyn_style: move s => $(if disabled {s.text_color(theme().text_muted)} else {s});
                    },
                ] }.into_view()
            }
            Kind::Choice(_) | Kind::Language | Kind::Font => dyn_view(move || {
                let options = self
                    .field
                    .choices(&self.state.get_untracked(), &crate::app::Gds::current())
                    .into_iter()
                    .map(|choice| {
                        SearchOption::new(choice.value, choice.title).keywords(choice.search_terms)
                    })
                    .collect();
                let getter = self.clone();
                let setter = self.clone();
                let binding = Binding::new(
                    move || getter.state.with(|state| (getter.field.get)(state)),
                    move |value| setter.apply(value),
                );
                search_choice(
                    binding,
                    options,
                    crate::i18n::text(self.field.label),
                    crate::i18n::text("form-search"),
                )
                .disabled({
                    let disabled = self.disabled;
                    move || disabled
                })
            })
            .into_view(),
            Kind::Text | Kind::StringList => {
                let initial = untrack(|| self.value());
                let value = signal(Self::text_value(&initial));
                let getter = self.clone();
                effect(move || {
                    value.set_if_changed(Self::text_value(&getter.value()));
                });
                let field = self.clone();
                let input = match self.field.kind {
                    Kind::StringList => text_area_field(value),
                    _ => text_field(value),
                };
                let height = if matches!(self.field.kind, Kind::StringList) {
                    46.0_f32
                } else {
                    24.0_f32
                };
                el! { input.{ w_full; h: height; py:1.0_f32; rounded:5.0_f32;
                    dyn_style: s => $(s.bg(theme().page).border_color(theme().border_strong));
                    focused:s=>$(s.outline(0.0_f32).border_color(theme().focus_ring)); disabled: move () => self->disabled;
                    on_input: move content => ${
                        let value = match field.field.kind {
                            Kind::StringList => Value::Array(content.lines().map(|line| Value::String(line.to_owned())).collect()),
                            _ if field.field.nullable && content.is_empty() => Value::Null,
                            _ => Value::String(content.to_owned()),
                        };
                        field.apply(value);
                    };
                } }.into_view()
            }
        }
    }
    fn text_value(value: &Value) -> String {
        match value {
            Value::String(value) => value.clone(),
            Value::Array(values) => values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }
}
