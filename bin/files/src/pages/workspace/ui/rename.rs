//! Safe single-item renaming and its dialog input state.

use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

use gds::{i18n, ui::DialogActions, ui::notifications};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{Browser, RenameNotice};

const DIALOG_GAP: f32 = 8.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum NameError {
    #[error("the new name is empty")]
    Empty,
    #[error("the new name contains a path separator")]
    Separator,
    #[error("the new name is a reserved directory name")]
    Reserved,
    #[error("the new name is unchanged")]
    Unchanged,
    #[error("the new name contains an invalid character")]
    Invalid,
}

impl NameError {
    fn label_id(self) -> &'static str {
        match self {
            Self::Empty => "rename-error-empty",
            Self::Separator => "rename-error-separator",
            Self::Reserved => "rename-error-reserved",
            Self::Unchanged => "rename-error-unchanged",
            Self::Invalid => "rename-error-invalid",
        }
    }
}

/// Check a typed leaf name without touching the filesystem.
///
/// The comparison is byte-exact: leading and trailing spaces are legal names
/// and are never trimmed implicitly.
pub(super) fn validate_name(name: &str, current: &OsStr) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
        return Err(NameError::Separator);
    }
    if name == "." || name == ".." {
        return Err(NameError::Reserved);
    }
    if name.contains('\0') {
        return Err(NameError::Invalid);
    }
    if OsStr::new(name) == current {
        return Err(NameError::Unchanged);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error(transparent)]
    Name(#[from] NameError),
    #[error("cannot rename a path without a parent and file name: {path}")]
    InvalidTarget { path: PathBuf },
    #[error("an item named {name} already exists in {parent}")]
    Exists { parent: PathBuf, name: String },
    #[error("cannot rename {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[cfg(not(target_os = "linux"))]
    #[error("safe no-replace rename is not available on this platform: {path}")]
    Unsupported { path: PathBuf },
}

#[derive(Debug, Clone)]
pub(super) struct Request {
    source: PathBuf,
    name: OsString,
}

impl Request {
    /// Resolve the destination and rename without replacing an existing entry.
    ///
    /// # Errors
    /// Rejects invalid leaf names, a missing destination parent, an existing
    /// target, and failures reported by the filesystem.
    pub(super) fn execute(&self) -> Result<PathBuf, Error> {
        let current = self
            .source
            .file_name()
            .ok_or_else(|| Error::InvalidTarget {
                path: self.source.clone(),
            })?;
        validate_name(&self.name.to_string_lossy(), current)?;
        let parent = self.source.parent().ok_or_else(|| Error::InvalidTarget {
            path: self.source.clone(),
        })?;
        let destination = parent.join(&self.name);
        if destination == self.source {
            return Err(Error::Name(NameError::Unchanged));
        }
        ensure_absent(&destination)?;
        rename_no_replace(&self.source, &destination)?;
        Ok(destination)
    }
}

fn ensure_absent(destination: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Err(exists(destination)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Filesystem {
            path: destination.to_path_buf(),
            source,
        }),
    }
}

fn exists(destination: &Path) -> Error {
    Error::Exists {
        parent: destination
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf),
        name: destination
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
    }
}

#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), Error> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    match renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::EXIST => Err(exists(destination)),
        // Refuse rather than fall back to a check-then-rename sequence: that
        // sequence can replace a destination created between the two calls.
        Err(error) => Err(Error::Filesystem {
            path: source.to_path_buf(),
            source: error.into(),
        }),
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_no_replace(source: &Path, _destination: &Path) -> Result<(), Error> {
    // No portable no-replace rename is wired on this target. A plain rename
    // could overwrite an existing entry, so refuse explicitly until a native
    // API is added here.
    Err(Error::Unsupported {
        path: source.to_path_buf(),
    })
}

#[derive(Clone, Debug)]
struct Target {
    path: PathBuf,
    original: OsString,
}

/// Input and validation state shared by the rename dialog and its owner.
pub(super) struct State {
    input: Entity<InputState>,
    target: Option<Target>,
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(i18n::text("rename-placeholder", cx))
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        Self {
            input,
            target: None,
            error: None,
            _subscription: subscription,
        }
    }

    pub(super) fn begin(
        &mut self,
        path: PathBuf,
        original: OsString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = original.to_string_lossy().into_owned();
        // The input caches its placeholder from construction, so a language
        // switch would otherwise leave the old locale in the empty field.
        let placeholder = i18n::text("rename-placeholder", cx);
        self.target = Some(Target { path, original });
        self.error = None;
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
            input.set_value(value, window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    pub(super) fn is_open(&self) -> bool {
        self.target.is_some()
    }

    pub(super) fn focus_input(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.target = None;
        self.error = None;
        cx.notify();
    }

    /// Consume a validated request. Invalid input leaves the dialog state intact.
    pub(super) fn take(&mut self, cx: &App) -> Option<Request> {
        if self.error.is_some() {
            return None;
        }
        let target = self.target.take()?;
        let name = self.input.read(cx).value().to_string();
        Some(Request {
            source: target.path,
            name: name.into(),
        })
    }

    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.refresh_error(cx),
            _ => return,
        }
        cx.notify();
    }

    pub(super) fn refresh_error(&mut self, cx: &App) {
        let Some(target) = &self.target else {
            self.error = None;
            return;
        };
        let value = self.input.read(cx).value();
        self.error = validate_name(value.as_ref(), &target.original)
            .err()
            .map(|error| i18n::text(error.label_id(), cx));
    }
}

impl Render for State {
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

impl Browser {
    pub(super) fn request_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn begin_rename(
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
                let footer = DialogActions::default()
                    .primary_label(i18n::text("rename-confirm", cx))
                    .primary_icon(IconName::Pencil)
                    .cancel_label(i18n::text("operation-cancel", cx));
                let mut arguments = fluent_bundle::FluentArgs::new();
                arguments.set("name", title_name.clone());
                el! {
                    dialog {
                        confirm; width: 480.0.px;
                        bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                        title: $(i18n::format("rename-title", Some(&arguments), cx));
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
        self.operations.rename.update(cx, |state, cx| {
            state.refresh_error(cx);
            cx.notify();
        });
        if self.operations.in_progress {
            return false;
        }
        let valid = self.operations.rename.read(cx).error.is_none();
        if !valid {
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
        notifications::Operation::start(i18n::text("rename-working", cx), cx);
        let source = request.source.clone();
        let origin_tab = self.tabs.active_id;
        let work = cx
            .background_executor()
            .spawn(async move { request.execute() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
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
        result: Result<PathBuf, Error>,
        cx: &mut Context<Self>,
    ) {
        self.operations.in_progress = false;
        let report = match result {
            Ok(destination) => {
                let name = destination
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                let mut arguments = fluent_bundle::FluentArgs::new();
                arguments.set("name", name);
                let summary = i18n::format("rename-done", Some(&arguments), cx);
                if let Some(slot) = self.tab_slot_mut(origin_tab) {
                    slot.pending_selection = Some(destination.clone());
                }
                self.clear_completed_selection(&[source.clone()]);
                self.refresh_after_file_operation(&[source, destination], cx);
                notifications::Report::new(notifications::Level::Success, summary)
            }
            Err(Error::Exists { name, .. }) => {
                let mut arguments = fluent_bundle::FluentArgs::new();
                arguments.set("name", name);
                notifications::Report::new(
                    notifications::Level::Error,
                    i18n::format("rename-target-exists", Some(&arguments), cx),
                )
            }
            Err(Error::Name(error)) => notifications::Report::new(
                notifications::Level::Error,
                i18n::text(error.label_id(), cx),
            ),
            Err(error) => {
                let mut arguments = fluent_bundle::FluentArgs::new();
                arguments.set("error", error.to_string());
                notifications::Report::new(
                    notifications::Level::Error,
                    i18n::format("rename-failed", Some(&arguments), cx),
                )
            }
        };
        notifications::Operation::finish(report, cx);
    }
}

#[cfg(test)]
mod tests;
