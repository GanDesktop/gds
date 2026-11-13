//! Creating one folder inside a captured destination directory.

use crate::{
    features::file_operations::{
        history as operation_history, names,
        new_folder::{Created, Error, Request},
    },
    shared::i18n::t,
};
use std::{
    cell::Cell,
    ffi::{OsStr, OsString},
    path::PathBuf,
    rc::Rc,
};

use gds::{i18n, ui::DialogActions, ui::notifications};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, WindowExt, dialog::AlertDialog};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Browser;
use super::name_form::NameForm;

const DIALOG_WIDTH: f32 = 480.;

struct NewFolderDialog {
    browser: WeakEntity<Browser>,
    origin_tab: u64,
    destination: PathBuf,
    form: Entity<NameForm>,
    started: Cell<bool>,
}

impl NewFolderDialog {
    fn new(
        browser: WeakEntity<Browser>,
        origin_tab: u64,
        destination: PathBuf,
        suggestion: OsString,
        window: &mut Window,
        cx: &mut Context<Browser>,
    ) -> Rc<Self> {
        let form = cx.new(|cx| {
            NameForm::new(
                suggestion,
                i18n::text("new-folder-placeholder", cx),
                window,
                cx,
            )
        });
        Rc::new(Self {
            browser,
            origin_tab,
            destination,
            form,
            started: Cell::new(false),
        })
    }

    fn open(this: Rc<Self>, window: &mut Window) {
        window.on_next_frame(move |window, cx| {
            let Some(browser) = this.browser.upgrade() else {
                return;
            };
            let focus = browser.read(cx).window.focus.clone();
            focus.focus(window, cx);
            let form = this.form.clone();
            window.open_alert_dialog(cx, Self::builder(this.clone()));
            window.on_next_frame(move |window, cx| {
                form.update(cx, |form, cx| form.focus_input(window, cx));
            });
        });
    }

    fn builder(this: Rc<Self>) -> impl Fn(AlertDialog, &mut Window, &mut App) -> AlertDialog {
        move |dialog, _, cx| {
            let on_ok = Self::confirm(this.clone());
            let on_close = Self::close(this.clone());
            let form = this.form.clone();
            let footer = el! { .DialogActions::default {
                primary_label: $(i18n::text("new-folder-confirm", cx));
                primary_icon: IconName::FolderPlus;
                cancel_label: $(i18n::text("operation-cancel", cx));
            } };
            let folder = this.destination.display().to_string();
            el! {
                dialog {
                    confirm; width: DIALOG_WIDTH.px;
                    bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                    title: $(t!(@cx, "new-folder-title", folder));
                    description: $(i18n::text("new-folder-message", cx));
                    icon: IconName::FolderPlus.Icon::new;
                    footer: footer;
                    on_ok: on_ok;
                    on_close: on_close;
                } [$(form.clone())]
            }
        }
    }

    fn confirm(this: Rc<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, _, cx| {
            let valid = this.form.update(cx, |form, cx| form.refresh_error(cx));
            if !valid {
                return false;
            }
            let name = this.form.update(cx, |form, cx| form.chosen_name(cx));
            let Some(browser) = this.browser.upgrade() else {
                return true;
            };
            this.started.set(true);
            let request = Request {
                directory: this.destination.clone(),
                name,
            };
            browser.update(cx, |browser, cx| {
                browser.start_new_folder(request, this.origin_tab, cx);
            });
            true
        }
    }

    fn close(this: Rc<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            if !this.started.get()
                && let Some(browser) = this.browser.upgrade()
            {
                browser.update(cx, |browser, cx| {
                    browser.operations.in_progress = false;
                    cx.notify();
                });
            }
            Browser::restore_file_focus(this.browser.clone(), this.origin_tab, window);
        }
    }
}

impl Browser {
    /// Ask for a name and create one folder inside `destination`.
    ///
    /// `destination` is captured now, so navigating elsewhere while the dialog
    /// is open cannot move the new folder somewhere else.
    pub(in crate::pages::workspace) fn request_new_folder(
        &mut self,
        destination: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.in_progress
            || !self.device_path_accepts_task(&destination, cx)
            || !self.pages.is_empty()
            || self.collection().is_some()
            || window.has_active_dialog(cx)
        {
            return;
        }
        let Some(origin_tab) = self.active().map(|slot| slot.id) else {
            return;
        };
        let default = i18n::text("new-folder-default-name", cx).to_string();
        self.operations.in_progress = true;
        self.operations.paths = vec![destination.clone()];
        // Suggesting a name probes the destination; a slow or network mount
        // must not block the UI while the dialog is being prepared.
        let directory = destination.clone();
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Suggest folder name", move || {
                names::free_name(
                    &directory,
                    OsStr::new(&default),
                    &std::collections::HashSet::new(),
                )
            });
        cx.spawn_in(window, async move |this, cx| {
            let suggested = work
                .await
                .map_err(names::Error::from)
                .and_then(std::convert::identity);
            if let Err(closed) = this.update_in(cx, |browser, window, cx| {
                browser.open_new_folder(origin_tab, destination, suggested, window, cx);
            }) {
                tracing::warn!("New folder suggestion arrived after the view closed: {closed}");
            }
        })
        .detach();
        cx.notify();
    }

    /// Open the naming dialog for a suggestion computed off the UI thread.
    fn open_new_folder(
        &mut self,
        origin_tab: u64,
        destination: PathBuf,
        suggested: Result<OsString, names::Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let suggestion = match suggested {
            Ok(suggestion) => suggestion,
            Err(error) => {
                self.operations.in_progress = false;
                notifications::Report::new(notifications::Level::Error, error.to_string()).show(cx);
                cx.notify();
                return;
            }
        };
        let dialog = NewFolderDialog::new(
            cx.entity().downgrade(),
            origin_tab,
            destination,
            suggestion,
            window,
            cx,
        );
        NewFolderDialog::open(dialog, window);
        cx.notify();
    }

    fn start_new_folder(&mut self, request: Request, origin_tab: u64, cx: &mut Context<Self>) {
        if !self.device_path_available(&request.directory, cx) {
            self.operations.in_progress = false;
            notifications::error(i18n::text("device-path-unavailable", cx), cx);
            cx.notify();
            return;
        }
        self.start_background(i18n::text("new-folder-working", cx), cx);
        let cancel = self.background_tasks.read(cx).token("files");
        let work = request.spawn(&self.window.tasks, cancel);
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(Error::from)
                .and_then(std::convert::identity);
            if let Err(closed) = this.update(cx, |this, cx| {
                this.finish_new_folder(origin_tab, result, cx)
            }) {
                tracing::warn!("Folder creation result arrived after the view closed: {closed}");
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_new_folder(
        &mut self,
        origin_tab: u64,
        result: Result<Created, Error>,
        cx: &mut Context<Self>,
    ) {
        self.operations.in_progress = false;
        let report = match result {
            Ok(Created {
                path: destination,
                snapshot,
            }) => {
                if let Some(expected) = snapshot {
                    self.operations
                        .history
                        .record(operation_history::Entry::new(
                            operation_history::Kind::NewFolder,
                            vec![operation_history::Step::trash_empty_directory(
                                destination.clone(),
                                expected,
                            )],
                        ));
                }
                if let Some(slot) = self.tab_slot_mut(origin_tab)
                    && destination.parent() == slot.path()
                {
                    slot.pending_selection = Some(destination.clone());
                }
                let mut affected = vec![destination.clone()];
                if let Some(parent) = destination.parent() {
                    affected.push(parent.to_path_buf());
                }
                self.refresh_after_file_operation(&affected, cx);
                let name = destination
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                notifications::Report::new(
                    notifications::Level::Success,
                    t!(@cx, "new-folder-done", name),
                )
            }
            Err(Error::Exists { name, .. }) => notifications::Report::new(
                notifications::Level::Error,
                t!(@cx, "new-folder-exists", name),
            ),
            Err(Error::Name(error)) => notifications::Report::new(
                notifications::Level::Error,
                i18n::text(error.label_id(), cx),
            ),
            Err(error) => {
                let error = error.to_string();
                notifications::Report::new(
                    notifications::Level::Error,
                    t!(@cx, "new-folder-failed", error),
                )
            }
        };
        self.finish_background(report, cx);
        cx.notify();
    }
}
