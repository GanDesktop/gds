//! Creating one folder inside a captured destination directory.

use crate::features::file_operations::history as operation_history;
use std::{
    cell::Cell,
    ffi::{OsStr, OsString},
    fs, io,
    path::PathBuf,
    rc::Rc,
};

use fluent_bundle::FluentArgs;
use gds::{i18n, ui::DialogActions, ui::notifications};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, WindowExt,
    dialog::AlertDialog,
    input::{InputEvent, InputState},
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Browser;
use crate::features::file_operations::names;

const DIALOG_WIDTH: f32 = 480.;
const DIALOG_GAP: f32 = 8.;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Name(#[from] names::NameError),
    #[error("an item named {name} already exists in {directory}")]
    Exists { directory: PathBuf, name: String },
    #[error("cannot create {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

struct Request {
    directory: PathBuf,
    name: OsString,
}

impl Request {
    /// Create the directory without ever replacing an existing entry; `mkdir`
    /// itself refuses an occupied name, so a target created after the dialog
    /// was confirmed is reported instead of reused.
    ///
    /// # Errors
    /// Rejects invalid leaf names and reports an occupied name or a filesystem
    /// failure.
    fn execute(self) -> Result<PathBuf, Error> {
        names::validate(&self.name)?;
        let destination = self.directory.join(&self.name);
        match fs::create_dir(&destination) {
            Ok(()) => Ok(destination),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => Err(Error::Exists {
                directory: self.directory,
                name: self.name.to_string_lossy().escape_debug().to_string(),
            }),
            Err(source) => Err(Error::Filesystem {
                path: destination,
                source,
            }),
        }
    }
}

struct Form {
    input: Entity<InputState>,
    suggestion: OsString,
    display: String,
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl Form {
    fn new(suggestion: OsString, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let display = suggestion.to_string_lossy().into_owned();
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(i18n::text("new-folder-placeholder", cx))
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        let mut form = Self {
            input,
            suggestion,
            display,
            error: None,
            _subscription: subscription,
        };
        let value = form.display.clone();
        form.input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.select_all(window, cx);
        });
        form.refresh_error(cx);
        form
    }

    /// The typed text, unless the user left the checked suggestion untouched.
    fn chosen_name(&self, cx: &App) -> OsString {
        let value = self.input.read(cx).value().to_string();
        if value == self.display {
            self.suggestion.clone()
        } else {
            OsString::from(value)
        }
    }

    fn focus_input(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    /// Check the typed leaf itself; the destination is only probed in the
    /// background after the user confirms.
    fn refresh_error(&mut self, cx: &App) -> bool {
        match names::validate(&self.chosen_name(cx)) {
            Ok(()) => {
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(i18n::text(error.label_id(), cx));
                false
            }
        }
    }

    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.refresh_error(cx);
        cx.notify();
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let error = self.error.clone();
        el! {
            .div { flex; flex_col; w_full; min_w_0; gap: DIALOG_GAP.px } [
                .div { w_full; min_w_0 } [($&input, cx).gds::ui::field::input { w_full }],
                @if (let Some(error) = error)
                    .div { text_xs; text_color: $cx.theme().danger } [error],
                @end,
            ]
        }
    }
}

struct NewFolderDialog {
    browser: WeakEntity<Browser>,
    origin_tab: u64,
    destination: PathBuf,
    form: Entity<Form>,
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
        let form = cx.new(|cx| Form::new(suggestion, window, cx));
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
            let footer = DialogActions::default()
                .primary_label(i18n::text("new-folder-confirm", cx))
                .primary_icon(IconName::FolderPlus)
                .cancel_label(i18n::text("operation-cancel", cx));
            let mut arguments = FluentArgs::new();
            arguments.set("folder", this.destination.display().to_string());
            el! {
                dialog {
                    confirm; width: DIALOG_WIDTH.px;
                    bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                    title: $(i18n::format("new-folder-title", Some(&arguments), cx));
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
    pub(super) fn request_new_folder(
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
        let work = cx.background_executor().spawn(async move {
            names::free_name(
                &directory,
                OsStr::new(&default),
                &std::collections::HashSet::new(),
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let suggested = work.await;
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
        notifications::Operation::start(i18n::text("new-folder-working", cx), cx);
        let work = cx
            .background_executor()
            .spawn(async move { request.execute() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
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
        result: Result<PathBuf, Error>,
        cx: &mut Context<Self>,
    ) {
        self.operations.in_progress = false;
        let report = match result {
            Ok(destination) => {
                self.operations
                    .history
                    .record(operation_history::Entry::new(
                        operation_history::Kind::NewFolder,
                        vec![operation_history::Step::trash(destination.clone())],
                    ));
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
                let mut arguments = FluentArgs::new();
                arguments.set(
                    "name",
                    destination
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
                );
                notifications::Report::new(
                    notifications::Level::Success,
                    i18n::format("new-folder-done", Some(&arguments), cx),
                )
            }
            Err(Error::Exists { name, .. }) => {
                let mut arguments = FluentArgs::new();
                arguments.set("name", name);
                notifications::Report::new(
                    notifications::Level::Error,
                    i18n::format("new-folder-exists", Some(&arguments), cx),
                )
            }
            Err(Error::Name(error)) => notifications::Report::new(
                notifications::Level::Error,
                i18n::text(error.label_id(), cx),
            ),
            Err(error) => {
                let mut arguments = FluentArgs::new();
                arguments.set("error", error.to_string());
                notifications::Report::new(
                    notifications::Level::Error,
                    i18n::format("new-folder-failed", Some(&arguments), cx),
                )
            }
        };
        notifications::Operation::finish(report, cx);
        cx.notify();
    }
}
