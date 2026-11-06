use super::{
    api::Service,
    model::{Form, Kind},
};
use crate::shared::{i18n::tr, ui};
use bas::service::{pb, tls::ConnectionOptions};
use gds_uim::el;
use gpui_kit::{
    App, Context, Entity, PathPromptOptions, Render, Window,
    assets::IconName,
    component::{input::InputState, *},
    div,
    prelude::*,
    px,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) struct ChooseDirectory;
impl gpui_kit::EventEmitter<ChooseDirectory> for Panel {}

type Opened = Box<dyn FnOnce(bas::model::Workspace, &mut App)>;
pub(crate) fn open(
    target: ConnectionOptions,
    local: bool,
    runtime: tokio::runtime::Handle,
    opened: impl FnOnce(bas::model::Workspace, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Panel> {
    cx.new(|cx| {
        Panel::new(
            Service {
                target,
                local,
                runtime,
            },
            Box::new(opened),
            window,
            cx,
        )
    })
}

enum Picker {
    Choices {
        field: String,
        source: String,
        page: pb::QueryWorkspaceChoicesResponse,
    },
}

pub(crate) struct Panel {
    service: Service,
    active: bool,
    dialog_open: bool,
    native_field: Option<String>,
    providers: Vec<pb::WorkspaceProvider>,
    selected: usize,
    form: Option<Form>,
    inputs: BTreeMap<String, Entity<InputState>>,
    values: BTreeMap<String, Value>,
    search: Entity<InputState>,
    picker: Option<Picker>,
    busy: bool,
    error: Option<String>,
    opened: Option<Opened>,
}

impl Panel {
    fn new(service: Service, opened: Opened, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let request = service.providers();
        cx.spawn_in(window, async move |view, cx| {
            let result = request.await;
            let _ = view.update_in(cx, |this: &mut Self, window, cx| {
                this.busy = false;
                match result {
                    Ok(providers) => {
                        this.providers = providers;
                        this.select_provider(0, window, cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.present(window, cx);
                cx.notify();
            });
        })
        .detach();
        Self {
            service,
            active: true,
            dialog_open: false,
            native_field: None,
            providers: Vec::new(),
            selected: 0,
            form: None,
            inputs: BTreeMap::new(),
            values: BTreeMap::new(),
            search: cx.new(|cx| InputState::new(window, cx)),
            picker: None,
            busy: true,
            error: None,
            opened: Some(opened),
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    fn present(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.service.local
            && self.providers.len() == 1
            && let Some(field) = self.form.as_ref().and_then(Form::directory_field)
        {
            self.native_field = Some(field.key());
            cx.emit(ChooseDirectory);
        } else {
            self.show_dialog(window, cx);
        }
    }

    pub(crate) fn choosing_directory(&self) -> bool {
        self.native_field.is_some()
    }

    pub(crate) fn directory_chosen(
        &mut self,
        result: Result<Option<std::path::PathBuf>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.native_field.take() else {
            return;
        };
        match result {
            Ok(Some(path)) => match path.to_str() {
                Some(path) => {
                    self.set_value(field, Value::String(path.into()), window, cx);
                    self.submit(window, cx);
                }
                None => self.error = Some(tr!("workspace-path-encoding").into()),
            },
            Ok(None) => {
                self.active = false;
                self.opened = None;
            }
            Err(error) => self.error = Some(error),
        }
        if self.error.is_some() {
            self.show_dialog(window, cx);
        }
        cx.notify();
    }

    fn show_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open || !self.active {
            return;
        }
        self.dialog_open = true;
        let panel = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let submit = panel.clone();
            let cancel = panel.clone();
            let close = panel.clone();
            let state = panel.read(cx);
            let footer = if state.busy || state.form.is_none() || state.picker.is_some() {
                el! { .gds::ui::dialog::DialogFooter::default [
                    gds::ui::dialog::DialogCommand::Cancel.gds::ui::dialog::DialogDecision::new,
                ] }
                .into_any_element()
            } else {
                el! { .gds::ui::DialogActions::default {
                    primary_label: tr!("workspace-open"); primary_icon: IconName::FolderOpen;
                } }
                .into_any_element()
            };
            el! { dialog {
                confirm; width: 480.0.px; title: tr!("workspace-add"); footer: footer;
                on_ok: $move |_, window, cx| {
                    submit.update(cx, |this, cx| this.submit(window, cx));
                    false
                };
                on_cancel: $move |_, _, cx| !cancel.read(cx).busy;
                on_close: $move |_, _, cx| {
                    close.update(cx, |this, cx| {
                        this.active = false;
                        this.dialog_open = false;
                        this.opened = None;
                        cx.notify();
                    });
                };
            } [$(panel.clone())] }
        });
        if let Some(input) = self.inputs.values().next() {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    fn select_provider(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = index;
        self.form = None;
        self.inputs.clear();
        self.values.clear();
        self.picker = None;
        self.error = None;
        let result = self
            .providers
            .get(index)
            .ok_or_else(|| tr!("workspace-provider-empty").to_owned())
            .and_then(|provider| {
                serde_json::from_slice(&provider.schema_json).map_err(|error| error.to_string())
            })
            .and_then(Form::new);
        match result {
            Ok(form) => {
                self.form = Some(form);
                self.build_inputs(window, cx);
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn build_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inputs.clear();
        self.values.clear();
        let Some(form) = &self.form else {
            return;
        };
        for field in &form.fields {
            let key = field.key();
            if let Some(value) = &field.default {
                self.values.insert(key.clone(), value.clone());
            }
            match &field.kind {
                Kind::Const(_) | Kind::Enum(_) => {}
                Kind::Boolean => {
                    if field.required {
                        self.values.entry(key).or_insert(Value::Bool(false));
                    }
                }
                _ => {
                    let text = field
                        .default
                        .as_ref()
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        })
                        .unwrap_or_default();
                    self.inputs.insert(
                        key,
                        cx.new(|cx| {
                            let mut input = InputState::new(window, cx);
                            input.set_value(text, window, cx);
                            input
                        }),
                    );
                }
            }
        }
    }

    fn parameters(&self, validate: bool, cx: &App) -> Result<Value, String> {
        let form = self
            .form
            .as_ref()
            .ok_or_else(|| tr!("workspace-provider-empty").to_owned())?;
        let mut values = self.values.clone();
        for field in &form.fields {
            if let Some(input) = self.inputs.get(&field.key()) {
                let text = input.read(cx).value();
                // Dynamic choices receive a partial form, including unfinished numeric inputs.
                match field.value(&text) {
                    Ok(Some(value)) => {
                        values.insert(field.key(), value);
                    }
                    Ok(None) => {
                        values.remove(&field.key());
                    }
                    Err(_) if !validate => {
                        values.remove(&field.key());
                    }
                    Err(error) => return Err(format!("{}: {error}", field.title)),
                }
            }
        }
        form.parameters(&values, validate)
    }

    fn set_value(
        &mut self,
        field: String,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.inputs.get(&field) {
            input.update(cx, |input, cx| {
                input.set_value(
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string()),
                    window,
                    cx,
                )
            });
        }
        self.values.insert(field, value);
        self.picker = None;
        cx.notify();
    }

    fn directory(&mut self, field: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !self.service.local {
            let path = self
                .inputs
                .get(&field)
                .map(|input| input.read(cx).value().to_string())
                .filter(|text| !text.is_empty());
            self.remote_directory(field, path, window, cx);
            return;
        }
        self.busy = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("workspace-choose-folder").into()),
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = async { paths.await? }.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Some(paths)) => {
                        if let Some(path) = paths.first() {
                            match path.to_str() {
                                Some(path) => {
                                    this.set_value(field, Value::String(path.into()), window, cx);
                                }
                                None => this.error = Some(tr!("workspace-path-encoding").into()),
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                if this.error.is_some() {
                    this.show_dialog(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remote_directory(
        &mut self,
        field: String,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gds::ui::{
            DialogActions,
            picker::directory::{DirectoryPicker, Loader},
        };
        let service = self.service.clone();
        let loader: Loader =
            std::rc::Rc::new(move |request| Box::pin(service.browse(request.path, request.cursor)));
        let picker = cx.new(|cx| DirectoryPicker::new(loader, path, window, cx));
        let owner = cx.entity().downgrade();
        let title = format!(
            "{} — {}",
            gds::i18n::text("picker-title", cx),
            self.service.target.target
        );
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let selected = picker.clone();
            let cancelled = picker.clone();
            let owner = owner.clone();
            let field = field.clone();
            let width = (window.bounds().size.width - px(48.)).min(px(640.));
            el! { dialog {
                confirm; width: width; title: $(title.clone());
                footer: .DialogActions::default {
                    primary_label: $(gds::i18n::text("picker-select", cx));
                    primary_disabled: $(picker.read(cx).selected_path(cx).is_none());
                };
                on_ok: $move |_, window, cx| {
                    let Some(path) = selected.read(cx).selected_path(cx) else { return false; };
                    let _ = owner.update(cx, |this, cx| this.set_value(field.clone(), Value::String(path), window, cx));
                    true
                };
                on_close: $move |_, window, cx| {
                    cancelled.update(cx, |picker, cx| picker.cancel_loading(window, cx));
                };
            } [$(picker.clone())] }
        });
    }

    fn choices(
        &mut self,
        field: String,
        source: String,
        cursor: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        let parameters = match self.parameters(false, cx) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let provider = &self.providers[self.selected];
        let request = pb::QueryWorkspaceChoicesRequest {
            provider_id: provider.id.clone(),
            schema_version: provider.schema_version,
            source: source.clone(),
            parameters_json: serde_json::to_vec(&parameters).expect("JSON values serialize"),
            query: self.search.read(cx).value().to_string(),
            cursor: cursor.clone(),
        };
        self.busy = true;
        self.error = None;
        let request = self.service.choices(request);
        cx.spawn_in(window, async move |view, cx| {
            let result = request.await;
            let _ = view.update_in(cx, |this, _, cx| {
                this.busy = false;
                match result {
                    Ok(mut page) => {
                        if cursor.is_some()
                            && let Some(Picker::Choices { page: previous, .. }) = &mut this.picker
                        {
                            let mut entries = std::mem::take(&mut previous.choices);
                            entries.append(&mut page.choices);
                            page.choices = entries;
                        }
                        this.picker = Some(Picker::Choices {
                            field,
                            source,
                            page,
                        });
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.picker.is_some() || !self.active {
            return;
        }
        let parameters = match self.parameters(true, cx) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                self.show_dialog(window, cx);
                cx.notify();
                return;
            }
        };
        let provider = &self.providers[self.selected];
        let request = self.service.open(pb::OpenWorkspaceRequest {
            provider_id: provider.id.clone(),
            schema_version: provider.schema_version,
            parameters_json: serde_json::to_vec(&parameters).expect("JSON values serialize"),
            name: None,
        });
        self.busy = true;
        self.error = None;
        cx.spawn_in(window, async move |view, cx| {
            let result = request.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(workspace) => {
                        this.active = false;
                        if this.dialog_open {
                            this.dialog_open = false;
                            window.close_dialog(cx);
                        }
                        if let Some(opened) = this.opened.take() {
                            cx.defer(move |cx| opened(workspace, cx));
                        }
                    }
                    Err(error) => {
                        this.error = Some(error);
                        this.show_dialog(window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

mod view;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
