//! Save decisions and caller-supplied chooser controls.
use super::*;
use crate::features::pick_files::{SaveDecision, prepare_save_targets};
use gds::ui::DialogActions;
use gds_uim::el;
use std::ffi::OsString;

pub(super) struct Mnemonic {
    pub text: String,
    pub key: Option<char>,
}

impl Mnemonic {
    pub fn parse(label: &str) -> Self {
        let mut text = String::new();
        let mut key = None;
        let mut chars = label.chars();
        while let Some(character) = chars.next() {
            if character == '_' {
                if let Some(next) = chars.next() {
                    if next != '_' && key.is_none() {
                        key = Some(next);
                    }
                    text.push(next);
                }
            } else {
                text.push(character);
            }
        }
        Self { text, key }
    }
}

impl State {
    pub(super) fn save_name(&self, cx: &App) -> Option<OsString> {
        let value = self.name.as_ref()?.read(cx).value();
        if let Some(original) = self
            .original_name
            .as_ref()
            .filter(|name| name.to_string_lossy() == value.as_str())
        {
            Some(original.clone())
        } else {
            Some(value.to_string().into())
        }
    }
}

impl Browser {
    pub(super) fn confirm_save(
        &mut self,
        paths: Vec<PathBuf>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mode) = self
            .picker
            .as_ref()
            .and_then(|picker| picker.session.options.portal.as_ref())
            .map(|request| request.mode.clone())
        else {
            return;
        };
        let window = self.window.handle;
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Prepare picker save targets", move || {
                prepare_save_targets(&mode, paths)
            });
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            let _ = window.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| match outcome {
                    Ok(Ok(SaveDecision::Ready(targets))) => {
                        this.complete_picker_selection(targets, cx);
                    }
                    Ok(Ok(SaveDecision::ConfirmOverwrite(targets))) => {
                        this.open_overwrite_dialog(targets, window, cx);
                    }
                    Ok(Err(failure)) => {
                        this.picker_notice(failure.message_id, failure.detail, cx);
                    }
                    Err(error) => {
                        this.picker_notice("picker-missing", Some(error.to_string()), cx);
                    }
                });
            });
        })
        .detach();
    }

    fn open_overwrite_dialog(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let browser = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog: AlertDialog, _, cx| {
            let browser = browser.clone();
            let paths = paths.clone();
            let colors = gds::appearance::theme::Colors::current(cx);
            let footer = el! { .DialogActions::default {
                primary_label: $(i18n::text("picker-save", cx));
                cancel_label: $(i18n::text("picker-cancel", cx));
            } };
            let filename = paths
                .first()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let on_ok = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(browser) = browser.upgrade() {
                    browser.update(cx, |browser, cx| {
                        browser.complete_picker_selection(paths.clone(), cx);
                    });
                }
                true
            };
            el! {
                dialog {
                    confirm;
                    width: $(px(460.));
                    bg: $(colors.panel);
                    text_color: $(colors.panel_foreground);
                    title: $(i18n::text("picker-overwrite-title", cx));
                    description: $(i18n::text("picker-overwrite-message", cx));
                    footer: footer;
                    on_ok: on_ok;
                } [
                    .div { min_w_0; w_full; text_sm; truncate; } [filename],
                ]
            }
        });
    }

    pub(in crate::pages::workspace::ui) fn render_picker_name(
        &self,
        available: bool,
    ) -> Option<AnyElement> {
        let input = self.picker.as_ref()?.name.as_ref()?;
        Some(
            el! {
                .div { flex_1; min_w_0; max_w: 480.0.px; }
                    [input.Input::new { small; disabled: $(!available); }]
            }
            .into_any_element(),
        )
    }

    pub(in crate::pages::workspace::ui) fn render_picker_extras(
        &self,
        available: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(picker) = &self.picker else {
            return Vec::new();
        };
        let mut controls = Vec::new();
        for choice in &picker.session.choices {
            let id = choice.id.clone();
            let view = cx.entity().downgrade();
            if choice.options.is_empty() {
                controls.push(
                    el! { $(SharedString::from(format!("picker-choice-{id}"))).Checkbox::new {
                        label: $(choice.label.clone());
                        small;
                        disabled: $(!available);
                        checked: $(choice.selected == "true");
                        on_change: $(move |checked, _, cx| {
                            if let Some(view) = view.upgrade() {
                                view.update(cx, |this, cx| {
                                    this.set_picker_choice(
                                        &id,
                                        if *checked { "true" } else { "false" },
                                        cx,
                                    )
                                });
                            }
                        });
                    } }
                    .into_any_element(),
                );
            } else {
                let selected = choice
                    .options
                    .iter()
                    .find(|(id, _)| id == &choice.selected)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_default();
                let options = choice.options.clone();
                controls.push(
                    el! { $(SharedString::from(format!("picker-choice-{id}"))).Button::new {
                        small;
                        label: $(format!("{}: {selected}", choice.label));
                        disabled: $(!available);
                        dropdown_menu: $(move |mut menu, _, _| {
                            for (value, label) in &options {
                                let view = view.clone();
                                let id = id.clone();
                                let value = value.clone();
                                menu = menu.item(
                                    el! { $(label.clone()).PopupMenuItem::new {
                                        checked: $(label == &selected);
                                        on_click: $(move |_, _, cx| {
                                            if let Some(view) = view.upgrade() {
                                                view.update(cx, |this, cx| {
                                                    this.set_picker_choice(&id, &value, cx)
                                                });
                                            }
                                        });
                                    } },
                                );
                            }
                            menu
                        });
                    } }
                    .into_any_element(),
                );
            }
        }
        controls
    }

    fn set_picker_choice(&mut self, id: &str, selected: &str, cx: &mut Context<Self>) {
        if let Some(choice) = self.picker.as_mut().and_then(|picker| {
            picker
                .session
                .choices
                .iter_mut()
                .find(|choice| choice.id == id)
        }) {
            choice.selected = selected.to_owned();
            cx.notify();
        }
    }
}
