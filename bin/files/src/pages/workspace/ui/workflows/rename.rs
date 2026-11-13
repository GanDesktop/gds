//! Safe single-item renaming and its dialog input state.

use super::name_form::NameForm;
use crate::{
    features::file_operations::{
        history as operation_history,
        rename::{Error, Renamed, Request},
    },
    shared::i18n::t,
};
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

use gds::{i18n, ui::DialogActions, ui::notifications};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt;
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{Browser, RenameNotice};

struct Active {
    path: PathBuf,
    form: Entity<NameForm>,
}

/// Input and validation state shared by the rename dialog and its owner.
pub(in crate::pages::workspace) struct State {
    active: Option<Active>,
}

impl State {
    pub(in crate::pages::workspace) fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self { active: None }
    }

    pub(in crate::pages::workspace) fn begin(
        &mut self,
        path: PathBuf,
        original: OsString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = i18n::text("rename-placeholder", cx);
        let form = cx.new(|cx| NameForm::for_rename(original, placeholder, window, cx));
        self.active = Some(Active { path, form });
        cx.notify();
    }

    pub(in crate::pages::workspace) fn is_open(&self) -> bool {
        self.active.is_some()
    }

    pub(in crate::pages::workspace) fn focus_input(&self, window: &mut Window, cx: &mut App) {
        if let Some(active) = &self.active {
            active
                .form
                .update(cx, |form, cx| form.focus_input(window, cx));
        }
    }

    pub(in crate::pages::workspace) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.active = None;
        cx.notify();
    }

    /// Consume a validated request. Invalid input leaves the dialog state intact.
    pub(in crate::pages::workspace) fn take(&mut self, cx: &mut Context<Self>) -> Option<Request> {
        let active = self.active.as_ref()?;
        let valid = active.form.update(cx, |form, cx| {
            let valid = form.refresh_error(cx);
            cx.notify();
            valid
        });
        if !valid {
            return None;
        }
        let active = self.active.take()?;
        let name = active.form.read(cx).chosen_name(cx);
        Some(Request {
            source: active.path,
            name,
        })
    }
}

impl Render for State {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match &self.active {
            Some(active) => el! { $(active.form.clone()) }.into_any_element(),
            None => el! { .div }.into_any_element(),
        }
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn request_rename(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pages.is_empty()
            || self.collection().is_some()
            || self.operations.in_progress
            || window.has_active_dialog(cx)
            || window.focused_input(cx).is_some()
            || !self.file_selection_focused(window, cx)
        {
            return;
        }
        let paths = self.selected_visible_paths();
        let [path] = paths.as_slice() else {
            return;
        };
        self.begin_rename(path.clone(), window, cx);
    }

    /// Open the rename dialog for an already validated single target.
    pub(in crate::pages::workspace) fn begin_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.in_progress || self.operations.rename.read(cx).is_open() {
            return;
        }
        let Some(original) = path.file_name().map(OsStr::to_os_string) else {
            return;
        };
        let Some(display_name) = original.to_str().map(str::to_owned) else {
            notifications::Report::new(
                notifications::Level::Error,
                i18n::text("rename-not-text", cx),
            )
            .replacing::<RenameNotice>()
            .show(cx);
            cx.notify();
            return;
        };
        self.operations
            .rename
            .update(cx, |state, cx| state.begin(path, original, window, cx));
        let browser = cx.entity().downgrade();
        let rename = self.operations.rename.clone();
        // Let a closing context menu leave the focus tree before the dialog is
        // recorded as its own return target.
        window.on_next_frame(move |window, cx| {
            let Some(browser) = browser.upgrade() else {
                return;
            };
            let browser = browser.downgrade();
            let title_name = display_name.clone();
            let focus_rename = rename.clone();
            window.open_alert_dialog(cx, move |dialog, _, cx| {
                let on_ok = Self::rename_confirm(browser.clone());
                let on_close = Self::rename_close(browser.clone(), rename.clone());
                let dialog_state = rename.clone();
                let footer = el! { .DialogActions::default {
                    primary_label: $(i18n::text("rename-confirm", cx));
                    primary_icon: IconName::Pencil;
                    cancel_label: $(i18n::text("operation-cancel", cx));
                } };
                let name = title_name.clone();
                el! {
                    dialog {
                        confirm; width: 480.0.px;
                        bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                        title: $(t!(@cx, "rename-title", name));
                        description: $(i18n::text("rename-message", cx));
                        icon: IconName::TextCursor.Icon::new;
                        footer: footer;
                        on_ok: on_ok;
                        on_close: on_close;
                    } [$(dialog_state.clone())]
                }
            });
            window.on_next_frame(move |window, cx| {
                focus_rename.update(cx, |state, cx| state.focus_input(window, cx));
            });
        });
        cx.notify();
    }

    fn rename_confirm(
        browser: WeakEntity<Self>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, _, cx| {
            let Some(browser) = browser.upgrade() else {
                return true;
            };
            browser.update(cx, |browser, cx| browser.confirm_rename(cx))
        }
    }

    fn rename_close(
        browser: WeakEntity<Self>,
        rename: Entity<State>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            rename.update(cx, |state, cx| state.cancel(cx));
            let Some(browser) = browser.upgrade() else {
                return;
            };
            let origin_tab = browser.read(cx).tabs.active_id;
            browser.update(cx, |_, cx| cx.notify());
            Self::restore_file_focus(browser.downgrade(), origin_tab, window);
        }
    }

    fn confirm_rename(&mut self, cx: &mut Context<Self>) -> bool {
        if self.operations.in_progress {
            return false;
        }
        let Some(request) = self
            .operations
            .rename
            .update(cx, |state, cx| state.take(cx))
        else {
            return false;
        };
        if !self.device_path_accepts_task(&request.source, cx) {
            notifications::error(i18n::text("device-path-unavailable", cx), cx);
            return true;
        }
        self.operations.in_progress = true;
        self.operations.paths = vec![request.source.clone()];
        self.start_background(i18n::text("rename-working", cx), cx);
        let source = request.source.clone();
        let origin_tab = self.tabs.active_id;
        let cancel = self.background_tasks.read(cx).token("files");
        let work = request.spawn(&self.window.tasks, cancel);
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(Error::from)
                .and_then(std::convert::identity);
            if let Err(error) = this.update(cx, |this, cx| {
                this.finish_rename(origin_tab, source, result, cx);
                cx.notify();
            }) {
                tracing::warn!("Rename result arrived after the view closed: {error}");
            }
        })
        .detach();
        cx.notify();
        true
    }

    fn finish_rename(
        &mut self,
        origin_tab: u64,
        source: PathBuf,
        result: Result<Renamed, Error>,
        cx: &mut Context<Self>,
    ) {
        self.operations.in_progress = false;
        let report = match result {
            Ok(Renamed {
                destination,
                snapshot,
            }) => {
                if let Some(expected) = snapshot {
                    self.operations
                        .history
                        .record(operation_history::Entry::new(
                            operation_history::Kind::Rename,
                            vec![operation_history::Step::Move {
                                source: destination.clone(),
                                destination: source.clone(),
                                expected,
                            }],
                        ));
                }
                let name = destination
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                let summary = t!(@cx, "rename-done", name);
                if let Some(slot) = self.tab_slot_mut(origin_tab) {
                    slot.pending_selection = Some(destination.clone());
                }
                self.clear_completed_selection(&[source.clone()]);
                self.refresh_after_file_operation(&[source, destination], cx);
                notifications::Report::new(notifications::Level::Success, summary)
            }
            Err(Error::Exists { name, .. }) => notifications::Report::new(
                notifications::Level::Error,
                t!(@cx, "rename-target-exists", name),
            ),
            Err(Error::Name(error)) => notifications::Report::new(
                notifications::Level::Error,
                i18n::text(error.label_id(), cx),
            ),
            Err(error) => {
                let error = error.to_string();
                notifications::Report::new(
                    notifications::Level::Error,
                    t!(@cx, "rename-failed", error),
                )
            }
        };
        self.finish_background(report, cx);
    }
}
