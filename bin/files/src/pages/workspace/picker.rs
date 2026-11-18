//! Native chooser controls over the existing lossless picker protocol.
use super::{
    model::{Pane, Prompt, Row},
    ui::Browser,
};
use crate::{
    entities::location::Location,
    features::pick_files::{self, SaveDecision},
};
use anti_webui::prelude::*;
use gds::{
    i18n,
    service::desktop::protocol::chooser::Mode,
    ui::{Icon, tooltip::tooltip},
};
use gds_uim::el;
use std::{ffi::OsString, path::PathBuf};

struct Mnemonic {
    text: String,
    key: Option<char>,
}
impl Mnemonic {
    fn parse(label: &str) -> Self {
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

impl Browser {
    fn picker_actions_available(&self) -> bool {
        self.pane() == Pane::Files
            && matches!(self.0.prompt.get_untracked(), Prompt::Closed)
            && !self.0.menus.is_open()
            && !self.0.connector.is_connecting()
            && self.read(|state| {
                state
                    .picker
                    .as_ref()
                    .is_some_and(|picker| !picker.completing)
            })
    }

    pub(super) fn initialize_picker(&self) {
        self.update(|state| {
            let Some(picker) = state.picker.as_mut() else {
                return;
            };
            if let Some(request) = picker.options.portal.as_ref()
                && let Some(current) = request.current_filter.as_ref()
                && let Some(index) = request.filters.iter().position(|filter| filter == current)
            {
                picker.select_filter(index);
            }
        });
        if let Some(name) = self.picker_original_name() {
            self.0.save_name.set(name.to_string_lossy().into_owned());
        }
    }

    fn picker_original_name(&self) -> Option<OsString> {
        self.read(|state| state.picker.as_ref()?.save_name.clone())
    }
    fn picker_save_name(&self) -> OsString {
        let displayed = self.0.save_name.get();
        self.picker_original_name()
            .filter(|original| original.to_string_lossy() == displayed)
            .unwrap_or_else(|| displayed.into())
    }
    fn picker_candidates(&self) -> Option<Vec<PathBuf>> {
        let save_name = self.picker_save_name();
        self.read(|state| {
            let picker = state.picker.as_ref()?;
            let tab = state.tab();
            if picker.completing || picker.submitting || tab.navigation.is_loading() {
                return None;
            }
            if matches!(
                picker.options.portal.as_ref().map(|request| &request.mode),
                Some(Mode::Save { .. })
            ) {
                return pick_files::Options::valid_name(&save_name)
                    .then(|| {
                        tab.navigation
                            .path()
                            .map(|path| vec![path.join(&save_name)])
                    })
                    .flatten();
            }
            let rows = tab.selected();
            if rows.is_empty() && picker.options.directory {
                return tab.navigation.path().map(|path| vec![path.to_owned()]);
            }
            if rows.is_empty() || (!picker.allows_multiple() && rows.len() != 1) {
                return None;
            }
            rows.iter()
                .map(|row| match row {
                    Row::File(entry) if picker.accepts_entry(entry) => Some(entry.path.clone()),
                    _ => None,
                })
                .collect()
        })
    }
    pub(super) fn picker_footer(&self) -> impl IntoView {
        let browser = self.clone();
        dyn_view(move || {
            let Some((options, filters, choices)) = untrack(|| {
                browser.read(|state| {
                    state.picker.as_ref().map(|picker| {
                        (
                            picker.options.clone(),
                            picker
                                .filters()
                                .iter()
                                .enumerate()
                                .map(|(index, filter)| {
                                    (
                                        index,
                                        filter
                                            .label
                                            .clone()
                                            .unwrap_or_else(|| i18n::text("picker-all-files")),
                                    )
                                })
                                .collect::<Vec<_>>(),
                            picker.choices.clone(),
                        )
                    })
                })
            }) else {
                return div().into_view();
            };
            let get = browser.clone();
            let set = browser.clone();
            let filter = select(
                Binding::new(
                    move || {
                        get.read(|state| {
                            state
                                .picker
                                .as_ref()
                                .map_or(0, |picker| picker.active_filter)
                        })
                    },
                    move |index| set.set_picker_filter(index),
                ),
                filters,
            )
            .min_w(96.0_f32)
            .max_w(220.0_f32)
            .flex_1();
            let get = browser.clone();
            let set = browser.clone();
            let read_only = checkbox(
                Binding::new(
                    move || {
                        get.read(|state| {
                            state.picker.as_ref().is_some_and(|picker| picker.read_only)
                        })
                    },
                    move |value| {
                        set.update(|state| {
                            if let Some(picker) = state.picker.as_mut() {
                                picker.read_only = value;
                            }
                        })
                    },
                ),
                i18n::text("picker-read-only"),
            );
            let choices = choices
                .into_iter()
                .map(|choice| {
                    let get = browser.clone();
                    let set = browser.clone();
                    let id = choice.id.clone();
                    let edit_id = id.clone();
                    if choice.options.is_empty() {
                        checkbox(
                            Binding::new(
                                move || {
                                    get.read(|state| {
                                        state
                                            .picker
                                            .as_ref()
                                            .and_then(|picker| {
                                                picker.choices.iter().find(|choice| choice.id == id)
                                            })
                                            .is_some_and(|choice| choice.selected == "true")
                                    })
                                },
                                move |value| {
                                    set.set_picker_choice(
                                        &edit_id,
                                        if value { "true" } else { "false" },
                                    )
                                },
                            ),
                            choice.label,
                        )
                        .into_view()
                    } else {
                        let selection = select(
                            Binding::new(
                                move || {
                                    get.read(|state| {
                                        state
                                            .picker
                                            .as_ref()
                                            .and_then(|picker| {
                                                picker.choices.iter().find(|choice| choice.id == id)
                                            })
                                            .map(|choice| choice.selected.clone())
                                            .unwrap_or_default()
                                    })
                                },
                                move |value: String| set.set_picker_choice(&edit_id, &value),
                            ),
                            choice.options,
                        );
                        el! { .div.{flex; items_center; gap: 6.0_f32}.[choice->label.text, selection] }
                            .into_view()
                    }
                })
                .collect::<Vec<_>>();
            let accept = browser.clone();
            let cancel = browser.clone();
            let enter = browser.clone();
            let label = options
                .portal
                .as_ref()
                .and_then(|request| request.accept_label.as_deref())
                .map(Mnemonic::parse)
                .map(|mnemonic| mnemonic.text)
                .unwrap_or_else(|| {
                    i18n::text(if options.is_save() {
                        "picker-save"
                    } else if options.directory {
                        "picker-confirm-directory"
                    } else {
                        "picker-confirm"
                    })
                });
            let available = browser.clone();
            el! { .div.{
                flex; flex_col; flex_none; min_w: 0.0_f32; gap: 8.0_f32; p: 12.0_f32;
                border_t: 1.0_f32; border_color: $(theme().border);
            }.[
                if matches!(options.portal.as_ref().map(|request| &request.mode), Some(Mode::Save { .. })) [
                    $(browser.0.save_name).text_field.{
                        placeholder: $(i18n::text("picker-file-name"));
                        w_full; min_w: 0.0_f32;
                        on_submit: move _ => $(enter.accept_picker());
                    },
                ],
                .div.{flex; flex_wrap; items_center; justify_end; min_w: 0.0_f32; gap: 8.0_f32}.[
                    if $(!options.directory) [
                        filter,
                        if $(!options.is_save() && !options.allow_mixed) [read_only],
                    ],
                    .div.{flex; flex_none; flex_wrap; max_w: 100.0_f32.pct; ml: AUTO; justify_end; gap: 8.0_f32}.[
                        $(tooltip(el! { $(i18n::text("picker-cancel")).button.{h: 32.0_f32; flex_none; flex_row_reverse; gap: 6.0_f32; on_click: move _ => ${
                                cancel.cancel_picker();
                                cancel.0.window.close();
                            }}.[ $(Icon::Close.view(16.0_f32)) ]
                        }, "Alt+C")),
                        $(tooltip(el! { label.primary_button.{h: 32.0_f32; flex_none; flex_row_reverse; gap: 6.0_f32;
                                disabled: move () => $(available.picker_candidates().is_none());
                                on_click: move _ => $(accept.accept_picker());
                            }.[ $(Icon::Check.view(16.0_f32)) ]
                        }, "Alt+A / Ctrl+Enter")),
                    ],
                ],
                if $(!choices.is_empty()) [
                    .div.{flex; flex_wrap; items_center; gap: 8.0_f32; min_w: 0.0_f32}.[..choices],
                ],
            ] }.into_view()
        })
    }
    fn set_picker_choice(&self, id: &str, value: &str) {
        self.update(|state| {
            if let Some(choice) = state
                .picker
                .as_mut()
                .and_then(|picker| picker.choices.iter_mut().find(|choice| choice.id == id))
            {
                choice.selected = value.to_owned();
            }
        });
    }
    fn set_picker_filter(&self, index: usize) {
        let ids = self.update(|state| {
            if !state
                .picker
                .as_mut()
                .is_some_and(|picker| picker.select_filter(index))
            {
                return Vec::new();
            }
            state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>()
        });
        for id in ids {
            self.reproject(id);
            self.enrich_directory(id);
        }
    }
    pub(super) fn activate_picker_path(&self, path: PathBuf) {
        if !self.picker_actions_available() {
            return;
        }
        let origin = self.origin();
        let target = path.clone();
        self.blocking(
            "Inspect picker activation",
            move || Ok(pick_files::inspect_activated_path(&target)),
            move |browser, result| {
                let current = browser.origin();
                if current.tab != origin.tab
                    || current.location != origin.location
                    || !browser.picker_actions_available()
                {
                    return;
                }
                match result {
                    Ok(Ok(pick_files::ActivatedPath::Directory)) => {
                        browser.open(Location::from(path.as_path()))
                    }
                    Ok(Ok(pick_files::ActivatedPath::File)) => {
                        if browser.picker_original_name().is_some() {
                            if let Some(name) = path.file_name() {
                                browser.update(|state| {
                                    if let Some(picker) = state.picker.as_mut() {
                                        picker.save_name = Some(name.to_owned());
                                    }
                                });
                                browser.0.save_name.set(name.to_string_lossy().into_owned());
                            }
                            return;
                        }
                        let candidates = browser
                            .picker_candidates()
                            .filter(|paths| paths.contains(&path))
                            .unwrap_or_else(|| vec![path]);
                        browser.validate_picker(candidates);
                    }
                    Ok(Err(failure)) => browser.report_notice(match failure.detail {
                        Some(detail) => format!("{}: {detail}", i18n::text(failure.message_id)),
                        None => i18n::text(failure.message_id),
                    }),
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }
    pub(super) fn accept_picker(&self) {
        if !self.picker_actions_available() {
            return;
        }
        let Some(paths) = self.picker_candidates() else {
            return;
        };
        let mode = self.read(|state| {
            state
                .picker
                .as_ref()?
                .options
                .portal
                .as_ref()
                .map(|request| request.mode.clone())
        });
        if let Some(mode @ (Mode::Save { .. } | Mode::SaveFiles { .. })) = mode {
            if !self.start_picker_submission() {
                return;
            }
            let origin = self.origin();
            self.blocking(
                "Prepare picker save targets",
                move || Ok(pick_files::prepare_save_targets(&mode, paths)),
                move |browser, result| {
                    browser.release_picker_submission();
                    let current = browser.origin();
                    if current.tab != origin.tab || current.location != origin.location {
                        return;
                    }
                    match result {
                        Ok(Ok(SaveDecision::Ready(paths))) => browser.validate_picker(paths),
                        Ok(Ok(SaveDecision::ConfirmOverwrite(paths))) => {
                            let approve = browser.clone();
                            let message = i18n::text("picker-overwrite-message");
                            let message = paths
                                .first()
                                .and_then(|path| path.file_name())
                                .map_or(message.clone(), |name| {
                                    format!("{message}\n{}", name.to_string_lossy())
                                });
                            browser.confirm_labelled(
                                i18n::text("picker-overwrite-title"),
                                message,
                                i18n::text("picker-save"),
                                move || approve.validate_picker(paths.clone()),
                            );
                        }
                        Ok(Err(failure)) => browser.report_notice(match failure.detail {
                            Some(detail) => format!("{}: {detail}", i18n::text(failure.message_id)),
                            None => i18n::text(failure.message_id),
                        }),
                        Err(error) => browser.report_notice(error),
                    }
                },
            );
        } else {
            self.validate_picker(paths);
        }
    }
    fn validate_picker(&self, paths: Vec<PathBuf>) {
        if paths
            .iter()
            .any(|path| self.0.volumes.removal_in_progress(path))
        {
            self.report_notice(i18n::text("device-path-unavailable"));
            return;
        }
        let Some(check) = self.read(|state| {
            let picker = state.picker.as_ref().filter(|picker| !picker.completing)?;
            Some(
                picker.selection_check(
                    state
                        .tab()
                        .rows
                        .iter()
                        .filter_map(|row| match row {
                            Row::File(entry) if picker.accepts_entry(entry) => {
                                Some(entry.path.clone())
                            }
                            _ => None,
                        })
                        .collect(),
                ),
            )
        }) else {
            return;
        };
        if !self.start_picker_submission() {
            return;
        }
        let origin = self.origin();
        let targets = paths.clone();
        self.blocking(
            "Validate picker selection",
            move || Ok(check.validate(&targets)),
            move |browser, result| {
                browser.release_picker_submission();
                let current = browser.origin();
                if current.tab != origin.tab || current.location != origin.location {
                    return;
                }
                match result {
                    Ok(Ok(())) => {
                        if let Some(result) = browser.0.picker_result.as_ref() {
                            let accepted = browser.update(|state| {
                                state
                                    .picker
                                    .as_mut()
                                    .filter(|picker| !picker.completing)
                                    .map(|picker| picker.completing = true)
                                    .is_some()
                            });
                            if accepted {
                                result(paths);
                                browser.0.window.close();
                            }
                            return;
                        }
                        let complete = browser.update(|state| {
                            state
                                .picker
                                .as_mut()
                                .filter(|picker| !picker.completing)
                                .map(|picker| picker.complete(&paths))
                        });
                        if let Some(result) = complete {
                            if let Err(error) = result {
                                tracing::warn!(%error,"Cannot report picker selection");
                            }
                            browser.0.window.close();
                        }
                    }
                    Ok(Err(failure)) => browser.report_notice(match failure.detail {
                        Some(detail) => format!("{}: {detail}", i18n::text(failure.message_id)),
                        None => i18n::text(failure.message_id),
                    }),
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }

    fn start_picker_submission(&self) -> bool {
        self.update(|state| {
            let Some(picker) = state
                .picker
                .as_mut()
                .filter(|picker| !picker.completing && !picker.submitting)
            else {
                return false;
            };
            picker.submitting = true;
            true
        })
    }

    fn release_picker_submission(&self) {
        self.update(|state| {
            if let Some(picker) = state.picker.as_mut() {
                picker.submitting = false;
            }
        });
    }
    pub(super) fn cancel_picker(&self) {
        self.update(|state| {
            if let Some(picker) = state.picker.as_mut() {
                picker.cancel();
            }
        });
    }
    pub(super) fn picker_key(&self, event: &KeyboardEvent) -> bool {
        if !self.picker_actions_available() {
            return false;
        }
        let Some(options) = self.read(|state| {
            state
                .picker
                .as_ref()
                .filter(|picker| !picker.completing)
                .map(|picker| picker.options.clone())
        }) else {
            return false;
        };
        if event.modifiers.primary() && event.key == Key::Named(NamedKey::Enter) {
            self.accept_picker();
            return true;
        }
        if !event.modifiers.alt {
            return false;
        }
        let Key::Character(key) = &event.key else {
            return false;
        };
        let mnemonic = options
            .portal
            .as_ref()
            .and_then(|request| request.accept_label.as_deref())
            .and_then(|label| Mnemonic::parse(label).key)
            .map(|key| key.to_lowercase().to_string());
        match key.to_lowercase().as_str() {
            key if key == "a" || mnemonic.as_deref() == Some(key) => self.accept_picker(),
            "c" => {
                self.cancel_picker();
                self.0.window.close();
            }
            "r" if !options.directory && !options.is_save() && !options.allow_mixed => {
                self.update(|state| {
                    if let Some(picker) = state.picker.as_mut() {
                        picker.read_only = !picker.read_only;
                    }
                })
            }
            "t" if !options.directory => {
                let next = self.read(|state| {
                    state
                        .picker
                        .as_ref()
                        .map(|picker| (picker.active_filter + 1) % picker.filters().len())
                });
                if let Some(next) = next {
                    self.set_picker_filter(next);
                }
            }
            _ => return false,
        }
        true
    }
}
