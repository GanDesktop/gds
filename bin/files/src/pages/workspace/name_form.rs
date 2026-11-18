//! Native leaf-name field shared by file creation, rename, and paste conflicts.

use std::{ffi::OsString, rc::Rc};

use anti_webui::prelude::*;
use gds::{
    app::Gds,
    i18n,
    ui::{
        Icon,
        dialog::{DecisionActions, Dialog, DialogDecision},
    },
};
use gds_uim::el;

use super::{model::Prompt, ui::Browser};
use crate::features::file_operations::{names, rename};

#[derive(Clone, Copy)]
pub(super) enum NamePurpose {
    NewFolder,
    Rename,
    Conflict,
    Generic,
}

struct Presentation {
    description: &'static str,
    placeholder: &'static str,
    action: &'static str,
    icon: Icon,
    action_icon: Icon,
    width: f32,
}

impl NamePurpose {
    fn presentation(self) -> Option<Presentation> {
        Some(match self {
            Self::NewFolder => Presentation {
                description: "new-folder-message",
                placeholder: "new-folder-placeholder",
                action: "new-folder-confirm",
                icon: Icon::FolderPlus,
                action_icon: Icon::FolderPlus,
                width: 480.0,
            },
            Self::Rename => Presentation {
                description: "rename-message",
                placeholder: "rename-placeholder",
                action: "rename-confirm",
                icon: Icon::TextCursor,
                action_icon: Icon::Edit,
                width: 480.0,
            },
            Self::Conflict => Presentation {
                description: "transfer-conflict-message",
                placeholder: "transfer-conflict-placeholder",
                action: "paste-files",
                icon: Icon::Paste,
                action_icon: Icon::Paste,
                width: 520.0,
            },
            Self::Generic => return None,
        })
    }
}

#[derive(Clone)]
pub(super) struct NameForm {
    purpose: NamePurpose,
    title: String,
    suggestion: OsString,
    display: String,
}

impl NameForm {
    pub(super) fn new(purpose: NamePurpose, title: String, suggestion: OsString) -> Self {
        let display = suggestion.to_string_lossy().into_owned();
        Self {
            purpose,
            title,
            suggestion,
            display,
        }
    }

    fn checked_name(&self, value: &str) -> Result<OsString, &'static str> {
        // The suggestion can contain non-UTF-8 bytes that the native field displays lossily.
        let name = if value == self.display {
            self.suggestion.clone()
        } else {
            value.into()
        };
        match self.purpose {
            NamePurpose::Rename => rename::validate_name(&name, &self.suggestion)
                .map_err(rename::NameError::label_id)?,
            NamePurpose::NewFolder | NamePurpose::Conflict => {
                names::validate(&name).map_err(names::NameError::label_id)?;
            }
            NamePurpose::Generic => {}
        }
        Ok(name)
    }

    pub(super) fn view(self, browser: &Browser, submit: Rc<dyn Fn(OsString)>) -> View {
        let input = signal(self.display.clone());
        let error = signal(if matches!(self.purpose, NamePurpose::Rename) {
            None
        } else {
            self.checked_name(&self.display).err()
        });
        let node = node_ref();
        let edit = self.clone();
        let accept = {
            let browser = browser.downgrade();
            let form = self.clone();
            Rc::new(move |value: &str| match form.checked_name(value) {
                Ok(name) => {
                    if let Some(browser) = browser.upgrade() {
                        browser.0.prompt.set(Prompt::Closed);
                        submit(name);
                    }
                }
                Err(label) => error.set_if_changed(Some(label)),
            })
        };
        let presentation = self.purpose.presentation();
        let mut confirm = DialogDecision::confirm(move || accept(&input.get_untracked()))
            .disabled_when(move || error.get().is_some());
        if let Some(presentation) = &presentation {
            confirm = confirm
                .label(i18n::text(presentation.action))
                .icon(presentation.action_icon);
        }
        let enter = confirm.clone();
        let cancel = browser.downgrade();
        let decisions = el! { .DecisionActions::default.[
            (move () => $(if let Some(browser) = cancel.upgrade() { browser.cancel_prompt(); })).DialogDecision::cancel.{
                label: $(i18n::text("operation-cancel"));
            },
            confirm,
        ] };
        let field = el! { .gds::ui::field::Field::default.{gap: 8.0_f32}.[
            input.text_field.{
                node_ref: node; h: 32.0_f32; w_full; min_w: 0.0_f32;
                if let Some(presentation) = $(self.purpose.presentation()) {
                    placeholder: $(i18n::text(presentation.placeholder));
                };
                on_input: move value => $(error.set_if_changed(edit.checked_name(value).err()));
                on_submit: move _ => &enter.{activate};
            },
            (move () => ${
                error.get().map(|label| el! { $(i18n::text(label)).text.{
                    dyn_style: style => $(style.text_size(Gds::current().appearance().typography().scaled(12.0))
                        .text_color(theme().danger_text));
                } })
            }).dyn_view,
        ] };
        let dialog = if let Some(presentation) = presentation {
            el! { .Dialog::default.{w: presentation->width; p: 16.0_f32; gap: 8.0_f32;
                decisions: decisions;
            }.[
                .div.{flex; items_start; gap: 8.0_f32; min_w: 0.0_f32; mb: -8.0_f32}.[
                    .div.{flex; items_center; h: 20.0_f32}.[ $(presentation.icon.view(16.0)) ],
                    .div.{flex; flex_col; gap: 4.0_f32; min_w: 0.0_f32}.[
                        self->title.text.{font_weight: FontWeight::SEMIBOLD;
                            dyn_style: style => $(style.text_size(Gds::current().appearance().typography().scaled(16.0)));
                        },
                        $(i18n::text(presentation.description)).text.{
                            dyn_style: style => $(style.text_color(theme().text_muted));
                        },
                    ],
                ],
                field,
                ] }
        } else {
            el! { .Dialog::default.{title: self->title; decisions: decisions}.[field] }
        };
        request_frame(move |_| {
            if input.is_alive() {
                let value = input.get_untracked();
                node.focus();
                node.set_text(&value, (0, value.len()));
            }
        });
        dialog.into_view()
    }
}
