use super::*;
use gpui_kit::{
    AnyElement,
    component::form::{Field as FormField, Form as FormView},
};

impl Panel {
    fn fields(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(form) = &self.form else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        let mut fields = FormView::default().small().label_text_size(ui::SMALL);
        for branch in &form.branches {
            let buttons = branch.titles.iter().enumerate().map(|(index, title)| {
                let key = branch.key.clone();
                el! { ($(format!("branch-{}-{index}", branch.key))).ui::action_button {
                    label: $(title.clone()); selected: $(index == branch.selected); disabled: $(self.busy);
                    on_click: $(cx.listener(move |this, _, window, cx| {
                        if let Some(form) = &mut this.form {
                            match form.select(key.clone(), index) {
                                Ok(()) => { this.error = None; this.build_inputs(window, cx); }
                                Err(error) => this.error = Some(error),
                            }
                        }
                        cx.notify();
                    }));
                }}.into_any_element()
            }).collect::<Vec<_>>();
            rows.push(el! { .div { flex; flex_wrap; gap_2; } [..buttons] }.into_any_element());
        }
        for field in &form.fields {
            if matches!(field.kind, Kind::Const(_)) {
                continue;
            }
            let key = field.key();
            let control = match &field.kind {
                Kind::Boolean => {
                    let checked = self
                        .values
                        .get(&key)
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let name = key.clone();
                    el! { ($(format!("boolean-{key}"))).ui::checkbox {
                        label: $(field.title.clone()); checked: checked; disabled: $(self.busy);
                        on_click: $(cx.listener(move |this, checked: &bool, window, cx| this.set_value(name.clone(), Value::Bool(*checked), window, cx)));
                    }}.into_any_element()
                }
                Kind::Enum(values) => {
                    let choices = values.iter().enumerate().map(|(index, value)| {
                        let name = key.clone(); let value = value.clone();
                        el! { ($(format!("enum-{key}-{index}"))).ui::action_button {
                            label: $(value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()));
                            selected: $(self.values.get(&key) == Some(&value)); disabled: $(self.busy);
                            on_click: $(cx.listener(move |this, _, window, cx| this.set_value(name.clone(), value.clone(), window, cx)));
                        }}.into_any_element()
                    }).collect::<Vec<_>>();
                    el! { .div { flex; flex_wrap; gap_2; } [..choices] }.into_any_element()
                }
                _ => gds::ui::field::input(&self.inputs[&key], cx)
                    .small()
                    .disabled(self.busy)
                    .w_full()
                    .into_any_element(),
            };
            let mut actions = Vec::new();
            if field.input.semantic_type.is_some() {
                let name = key.clone();
                actions.push(el! { ($(format!("browse-{key}")), IconName::FolderOpen, tr!("workspace-browse")).ui::icon_action {
                    disabled: $(self.busy);
                    on_click: $(cx.listener(move |this, _, window, cx| this.directory(name.clone(), window, cx)));
                }}.into_any_element());
            }
            if let Some(source) = &field.input.choices {
                let source = source.clone();
                let name = key.clone();
                actions.push(
                    el! { ($(format!("choices-{key}"))).ui::action_button {
                        label: tr!("workspace-select"); disabled: $(self.busy);
                        on_click: $(cx.listener(move |this, _, window, cx| {
                            this.search.update(cx, |input, cx| input.set_value("", window, cx));
                            this.choices(name.clone(), source.clone(), None, window, cx);
                        }));
                    }}
                    .into_any_element(),
                );
            }
            let label = if field.input.semantic_type.is_some() {
                tr!("workspace-directory").to_owned()
            } else {
                field.title.clone()
            };
            fields = fields.child(el! { .FormField::new {
                label: label; required: $(field.required);
                @if (let Some(description) = &field.description)
                    description: $(description.clone());
                @end;
            } [
                .div { flex; items_center; gap_2; w_full; } [
                    .div { flex_1; min_w_0; } [control],
                    ..actions,
                ],
            ] });
        }
        rows.push(fields.into_any_element());
        rows
    }

    fn picker(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(picker) = &self.picker else {
            return Vec::new();
        };
        let mut rows = vec![
            gds::ui::field::input(&self.search, cx)
                .small()
                .disabled(self.busy)
                .w_full()
                .into_any_element(),
        ];
        match picker {
            Picker::Choices {
                field,
                source,
                page,
            } => {
                let name = field.clone();
                let source = source.clone();
                rows.push(el! { ("choices-search").ui::action_button {
                    label: tr!("workspace-search"); disabled: $(self.busy);
                    on_click: $(cx.listener(move |this, _, window, cx| this.choices(name.clone(), source.clone(), None, window, cx)));
                }}.into_any_element());
                for (index, choice) in page.choices.iter().enumerate() {
                    let name = field.clone();
                    let bytes = choice.value_json.clone();
                    rows.push(el! { ($(format!("choice-{index}"))).ui::action_button {
                        label: $(choice.label.clone()); disabled: $(self.busy);
                        on_click: $(cx.listener(move |this, _, window, cx| match serde_json::from_slice(&bytes) {
                            Ok(value) => this.set_value(name.clone(), value, window, cx),
                            Err(error) => { this.error = Some(error.to_string()); cx.notify(); },
                        }));
                    }}.into_any_element());
                }
                if let Some(cursor) = &page.next_cursor {
                    let name = field.clone();
                    let Picker::Choices { source, .. } = picker;
                    let source = source.clone();
                    let cursor = cursor.clone();
                    rows.push(el! { ("choices-more").ui::action_button {
                        label: tr!("workspace-load-more"); disabled: $(self.busy);
                        on_click: $(cx.listener(move |this, _, window, cx| this.choices(name.clone(), source.clone(), Some(cursor.clone()), window, cx)));
                    }}.into_any_element());
                }
            }
        }
        rows.push(
            el! { ("picker-back").ui::action_button {
                label: tr!("action-cancel"); disabled: $(self.busy);
                on_click: $(cx.listener(|this, _, _, cx| { this.picker = None; cx.notify(); }));
            }}
            .into_any_element(),
        );
        rows
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let providers = (self.providers.len() > 1).then(|| {
            let buttons = self.providers.iter().enumerate().map(|(index, provider)| el! {
                ($(format!("provider-{index}"))).ui::action_button {
                    label: $(provider.title.clone()); selected: $(self.selected == index); disabled: $(self.busy);
                    on_click: $(cx.listener(move |this, _, window, cx| this.select_provider(index, window, cx)));
                }
            }.into_any_element()).collect::<Vec<_>>();
            el! { .div { flex; flex_wrap; gap_2; } [..buttons] }
        });
        let content = if self.picker.is_some() {
            self.picker(cx)
        } else {
            self.fields(cx)
        };
        el! {
            .div { # "workspace-launch-form"; flex; flex_col; gap_3; min_w_0; text_size: ui::SMALL; } [
                ..providers,
                .div { # "workspace-launch-scroll"; max_h: 320.0.px; overflow_y_scroll; flex; flex_col; gap_3; } [..content],
                ..$(self.error.clone().map(|error| ui::feedback("workspace-launch-error", error, cx.theme().danger))),
            ]
        }
    }
}
