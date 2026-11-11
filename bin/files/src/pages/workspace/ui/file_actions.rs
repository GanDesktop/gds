mod menu;

use std::{cell::RefCell, collections::HashSet, path::PathBuf, rc::Rc, sync::Arc};

use fluent_bundle::FluentArgs;
use gds::{i18n, ui::DialogActions, ui::notifications};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, WindowExt, dialog::AlertDialog, menu::PopupMenu};
use gpui_kit::{prelude::*, *};

use super::{Browser, ClipboardNotice};
use crate::entities::file::directory::EntryKind;
use crate::features::file_operations as operations;

const CONFIRMATION_WIDTH: f32 = 480.;
const NAME_PREVIEW_LIMIT: usize = 5;
const CONFIRMATION_GAP: f32 = 8.;

pub(super) struct State {
    pub rename: Entity<super::rename::State>,
    pub in_progress: bool,
    pub history: operations::history::History,
    pub paths: Vec<PathBuf>,
    pub remote_paste_cancel: Option<tokio_util::sync::CancellationToken>,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            rename: cx.new(|cx| super::rename::State::new(window, cx)),
            in_progress: false,
            history: Default::default(),
            paths: Vec::new(),
            remote_paste_cancel: None,
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(cancel) = &self.remote_paste_cancel {
            cancel.cancel();
        }
    }
}

#[derive(Clone)]
struct Selection {
    tab_id: u64,
    directory: PathBuf,
    paths: Vec<PathBuf>,
    directories_only: bool,
}

impl Selection {
    fn directory_entry(entry: &crate::entities::file::directory::Entry) -> bool {
        entry.kind == EntryKind::Directory
            || entry.kind == EntryKind::SymbolicLink
                && entry.details.target.as_ref().is_some_and(|target| {
                    target.status == crate::entities::file::directory::LinkStatus::Resolved
                        && target.kind == EntryKind::Directory
                })
    }

    fn terminal_target(&self) -> Option<PathBuf> {
        let [path] = self.paths.as_slice() else {
            return None;
        };
        if self.directories_only {
            Some(path.clone())
        } else {
            path.parent().map(std::path::Path::to_path_buf)
        }
    }

    fn capture(browser: &Browser) -> Option<Self> {
        let paths = browser.selected_visible_paths();
        if paths.is_empty() {
            return None;
        }
        let slot = browser.active()?;
        let tab = &slot.navigation;
        let selected: HashSet<_> = paths.iter().collect();
        let directories_only = slot
            .table_rows
            .iter()
            .filter(|entry| selected.contains(&entry.path))
            .all(Self::directory_entry);
        Some(Self {
            tab_id: slot.id,
            directory: tab.path()?.to_owned(),
            paths,
            directories_only,
        })
    }

    fn still_selected(&self, browser: &Browser) -> bool {
        let Some(slot) = browser.active() else {
            return false;
        };
        if slot.id != self.tab_id || slot.path() != Some(self.directory.as_path()) {
            return false;
        }
        let current = browser.selected_visible_paths();
        let selected: HashSet<_> = current.iter().collect();
        current.len() == self.paths.len() && self.paths.iter().all(|path| selected.contains(path))
    }
}

struct DirectoryTarget {
    tab_id: u64,
    path: PathBuf,
}

impl DirectoryTarget {
    fn capture(browser: &Browser, path: PathBuf) -> Option<Self> {
        if !browser.pages.is_empty() || browser.collection().is_some() {
            return None;
        }
        let slot = browser.active()?;
        (slot.files_available() && slot.path() == Some(path.as_path())).then_some(Self {
            tab_id: slot.id,
            path,
        })
    }

    fn still_current(&self, browser: &Browser) -> bool {
        browser.pages.is_empty()
            && browser.collection().is_none()
            && browser.active().is_some_and(|slot| {
                slot.files_available()
                    && slot.id == self.tab_id
                    && slot.path() == Some(self.path.as_path())
            })
    }
}

struct Request {
    origin_tab: u64,
    kind: operations::Kind,
    batch: operations::Batch,
}

struct Confirmation {
    browser: WeakEntity<Browser>,
    pending: RefCell<Option<Request>>,
    names: Vec<SharedString>,
    count: usize,
    origin_tab: u64,
}

impl Confirmation {
    fn new(browser: WeakEntity<Browser>, request: Request) -> Rc<Self> {
        let names = request
            .batch
            .paths()
            .iter()
            .take(NAME_PREVIEW_LIMIT)
            .map(|path| {
                path.file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .escape_debug()
                    .to_string()
                    .into()
            })
            .collect();
        let count = request.batch.paths().len();
        let origin_tab = request.origin_tab;
        Rc::new(Self {
            browser,
            pending: RefCell::new(Some(request)),
            names,
            count,
            origin_tab,
        })
    }

    fn open(this: Rc<Self>, window: &mut Window) {
        // Wait for a triggering popup to leave the focus tree. Kit then records
        // the browser, not the disappearing popup, as this dialog's return target.
        window.on_next_frame(move |window, cx| {
            let Some(browser) = this.browser.upgrade() else {
                return;
            };
            let focus = browser.read(cx).window.focus.clone();
            focus.focus(window, cx);
            window.open_alert_dialog(cx, Self::builder(this));
        });
    }

    fn builder(this: Rc<Self>) -> impl Fn(AlertDialog, &mut Window, &mut App) -> AlertDialog {
        move |dialog, _, cx| {
            let on_confirm = Self::confirm_handler(this.clone());
            let on_close = Self::close_handler(this.clone());
            let names = this.names.iter().cloned().map(Self::name_row);
            let remaining = this.count.saturating_sub(NAME_PREVIEW_LIMIT);
            let footer = el! { .DialogActions::default {
                primary_label: $(i18n::count("delete-selected", this.count, cx));
                primary_icon: IconName::Trash;
                cancel_label: $(i18n::text("operation-cancel", cx));
                danger;
            } };
            el! {
                dialog {
                    confirm; width: CONFIRMATION_WIDTH.px;
                    bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                    title: $(i18n::count("delete-confirm-title", this.count, cx));
                    description: $(i18n::text("delete-confirm-message", cx));
                    icon: IconName::Trash.Icon::new;
                    footer: footer;
                    on_ok: on_confirm;
                    on_close: on_close;
                } [
                    .div { flex; flex_col; gap: CONFIRMATION_GAP.px; w_full; min_w_0 } [
                        ..names,
                        @if (remaining > 0)
                            .div { text_sm; text_color: $(cx.theme().muted_foreground) } [$(i18n::count("operation-more-items", remaining, cx))],
                        @end,
                    ],
                ]
            }
        }
    }

    fn name_row(name: SharedString) -> Div {
        el! { .div { w_full; min_w_0; truncate; text_sm } [name] }
    }

    fn confirm_handler(this: Rc<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, _, cx| {
            let request = this.pending.borrow_mut().take();
            let Some(request) = request else {
                return false;
            };
            if let Some(browser) = this.browser.upgrade() {
                browser.update(cx, |browser, cx| browser.start_file_operation(request, cx));
            }
            true
        }
    }

    fn close_handler(this: Rc<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            // Only cancellation releases the pending lock; confirmed work owns it
            // until the background result arrives.
            if this.pending.borrow_mut().take().is_some()
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

impl operations::Kind {
    fn progress_label(self) -> &'static str {
        match self {
            Self::Trash => "trash-working",
            Self::PermanentDelete => "delete-working",
        }
    }

    fn success_label(self) -> &'static str {
        match self {
            Self::Trash => "trash-done",
            Self::PermanentDelete => "delete-done",
        }
    }

    fn partial_label(self) -> &'static str {
        match self {
            Self::Trash => "trash-partial",
            Self::PermanentDelete => "delete-partial",
        }
    }
}

impl Browser {
    pub(super) fn restore_file_focus(view: WeakEntity<Self>, origin_tab: u64, window: &mut Window) {
        window.on_next_frame(move |window, cx| {
            if window.has_active_dialog(cx) {
                return;
            }
            let Some(view) = view.upgrade() else {
                return;
            };
            let has_focused_input = window.focused_input(cx).is_some();
            let browser = view.read(cx);
            let closing_path_input = !browser.toolbar.path.editing
                && browser
                    .toolbar
                    .path
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .contains_focused(window, cx);
            if has_focused_input && !closing_path_input {
                return;
            }
            if !browser.pages.is_empty()
                || browser.collection().is_some()
                || browser.tabs.active_id != origin_tab
            {
                return;
            }
            let focus = browser.window.focus.clone();
            focus.focus(window, cx);
        });
    }

    pub(super) fn selected_visible_paths(&self) -> Vec<PathBuf> {
        if !self.pages.is_empty() || self.collection().is_some() {
            return Vec::new();
        }
        let Some(slot) = self.active().filter(|slot| slot.files_available()) else {
            return Vec::new();
        };
        let tab = &slot.navigation;
        let Some(directory) = tab.directory() else {
            return Vec::new();
        };
        let selected: HashSet<_> = tab.view().selected.iter().collect();
        slot.visible
            .iter()
            .filter_map(|&index| directory.entries.get(index))
            .filter(|entry| selected.contains(&entry.path))
            .map(|entry| entry.path.clone())
            .collect()
    }

    pub(super) fn request_trash_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_file_operation(operations::Kind::Trash, window, cx);
    }

    pub(super) fn request_delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_file_operation(operations::Kind::PermanentDelete, window, cx);
    }

    fn request_file_operation(
        &mut self,
        kind: operations::Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.in_progress {
            return;
        }
        let Some(selection) = Selection::capture(self) else {
            return;
        };
        if selection
            .paths
            .iter()
            .any(|path| !self.device_path_accepts_task(path, cx))
        {
            return;
        }
        let batch = match operations::Batch::new(selection.paths) {
            Ok(batch) => batch,
            Err(error) => {
                self.finish_background(
                    notifications::Report::new(notifications::Level::Error, error.to_string()),
                    cx,
                );
                cx.notify();
                return;
            }
        };
        self.operations.in_progress = true;
        self.operations.paths = batch.paths().to_vec();
        let request = Request {
            origin_tab: selection.tab_id,
            kind,
            batch,
        };
        match kind {
            operations::Kind::Trash => {
                Self::restore_file_focus(cx.entity().downgrade(), request.origin_tab, window);
                self.start_file_operation(request, cx);
            }
            operations::Kind::PermanentDelete => {
                let confirmation = Confirmation::new(cx.entity().downgrade(), request);
                Confirmation::open(confirmation, window);
            }
        }
        cx.notify();
    }

    fn start_file_operation(&mut self, request: Request, cx: &mut Context<Self>) {
        if request
            .batch
            .paths()
            .iter()
            .any(|path| !self.device_path_available(path, cx))
        {
            self.operations.in_progress = false;
            notifications::error(i18n::text("device-path-unavailable", cx), cx);
            cx.notify();
            return;
        }
        let origin_tab = request.origin_tab;
        let kind = request.kind;
        let count = request.batch.paths().len();
        self.start_background(i18n::count(kind.progress_label(), count, cx), cx);
        let cancel = self.background_tasks.read(cx).token("files");
        let work = self.window.tasks.spawn_blocking_named(
            match kind {
                operations::Kind::Trash => "Move files to trash",
                operations::Kind::PermanentDelete => "Delete files permanently",
            },
            move || request.batch.execute_cancellable(kind, &cancel),
        );
        cx.spawn(async move |this, cx| {
            let report = match work.await {
                Ok(report) => report,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.operations.in_progress = false;
                        this.operations.paths.clear();
                        this.finish_background(notifications::Report::new(
                            notifications::Level::Error, error.to_string()), cx);
                    });
                    return;
                }
            };
            for item in report.failed_items() {
                if let Err(error) = &item.result {
                    tracing::warn!("File operation {kind:?} failed for {}: {error}", item.path.display());
                }
            }
            if let Err(error) = this.update(cx, |this, cx| this.finish_file_operation(report, cx)) {
                tracing::warn!(
                    "File operation {kind:?} for tab {origin_tab} arrived after its window closed: {error}"
                );
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_file_operation(&mut self, mut report: operations::Report, cx: &mut Context<Self>) {
        self.operations.in_progress = false;
        let succeeded = report.succeeded_paths();
        if !report.history_errors.is_empty() {
            self.operations.history.clear();
        }
        if report.kind == operations::Kind::PermanentDelete && !succeeded.is_empty() {
            self.operations.history.clear();
        } else {
            self.operations
                .history
                .record(operations::history::Entry::new(
                    operations::history::Kind::Trash,
                    std::mem::take(&mut report.receipts)
                        .into_iter()
                        .map(operations::history::Step::Restore)
                        .collect(),
                ));
        }
        let failed = report.items.len() - succeeded.len();
        let level = if failed == 0 && !report.history_errors.is_empty() {
            notifications::Level::Warning
        } else if failed == 0 {
            notifications::Level::Success
        } else {
            notifications::Level::Error
        };
        let summary = if failed == 0 {
            i18n::count(report.kind.success_label(), succeeded.len(), cx)
        } else {
            let mut args = FluentArgs::new();
            args.set("succeeded", succeeded.len() as i64);
            args.set("failed", failed as i64);
            i18n::format(report.kind.partial_label(), Some(&args), cx)
        };
        let mut notice = notifications::Report::new(level, summary);
        for item in report.failed_items() {
            if let Err(error) = &item.result {
                let path = item
                    .path
                    .as_os_str()
                    .to_string_lossy()
                    .escape_debug()
                    .to_string();
                notice = notice.detail(format!("{path}: {error}"));
            }
        }
        for error in report.history_errors {
            notice = notice.detail(error);
        }
        self.clear_completed_selection(&succeeded);
        self.refresh_after_file_operation(&succeeded, cx);
        self.finish_background(notice, cx);
        cx.notify();
    }

    pub(super) fn clear_completed_selection(&mut self, paths: &[PathBuf]) {
        let completed: HashSet<_> = paths.iter().collect();
        // Directory refresh is asynchronous; key repeat must not resubmit completed targets.
        for slot in &mut self.tabs.items {
            {
                let tab = &mut slot.navigation;
                tab.view_mut()
                    .selected
                    .retain(|path| !completed.contains(path));
            }
            if slot
                .cursor
                .as_ref()
                .is_some_and(|path| completed.contains(path))
            {
                slot.cursor = None;
            }
            if slot
                .anchor
                .as_ref()
                .is_some_and(|path| completed.contains(path))
            {
                slot.anchor = None;
            }
        }
    }

    pub(super) fn directory_context_menu(
        &mut self,
        path: PathBuf,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PopupMenu {
        let menu = gds::ui::menu::popup_in(menu, window);
        let Some(target) = DirectoryTarget::capture(self, path) else {
            return menu;
        };
        self.window.focus.focus(window, cx);
        self.directory_menu(menu, Arc::new(target), cx)
    }

    pub(super) fn entry_context_menu(
        &mut self,
        path: PathBuf,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PopupMenu {
        let menu = gds::ui::menu::popup_in(menu, window);
        if !self.pages.is_empty() || self.collection().is_some() {
            return menu;
        }
        let Some(slot) = self.active() else {
            return menu;
        };
        let tab = &slot.navigation;
        let Some(directory) = tab.directory() else {
            return menu;
        };
        let visible_index = slot.visible.iter().position(|&index| {
            directory
                .entries
                .get(index)
                .is_some_and(|entry| entry.path == path)
        });
        let Some(visible_index) = visible_index else {
            return menu;
        };
        if !tab.view().selected.contains(&path) {
            self.select(visible_index, Modifiers::default(), cx);
        }
        self.window.focus.focus(window, cx);
        let Some(selection) = Selection::capture(self) else {
            return menu;
        };
        let selection = Arc::new(selection);
        if selection.paths.len() == 1 {
            self.single_entry_menu(menu, selection, cx)
        } else {
            self.multi_entry_menu(menu, selection, cx)
        }
    }

    pub(super) fn open_selected_in_new_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = Selection::capture(self) else {
            return;
        };
        self.open_directories(&selection, window, cx);
    }

    fn open_directories(
        &mut self,
        selection: &Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !selection.directories_only {
            notifications::info(i18n::text("open-directories-only", cx), cx);
            cx.notify();
            return;
        }
        for path in &selection.paths {
            self.open_tab(path.clone(), false, window, cx);
        }
    }

    pub(super) fn copy_selected_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let result =
            operations::Batch::new(paths.to_vec()).and_then(|batch| batch.clipboard_text());
        match result {
            Ok(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                // Path text replaces both the file summary and cut markers.
                crate::shared::lib::clipboard::clear(cx);
                notifications::Report::new(
                    notifications::Level::Success,
                    i18n::count("paths-copied", paths.len(), cx),
                )
                .replacing::<ClipboardNotice>()
                .show(cx);
            }
            Err(operations::Error::NonUtf8Clipboard { .. }) => {
                notifications::Report::new(
                    notifications::Level::Error,
                    i18n::text("path-not-text", cx),
                )
                .replacing::<ClipboardNotice>()
                .show(cx);
            }
            Err(error) => {
                notifications::Report::new(notifications::Level::Error, error.to_string())
                    .replacing::<ClipboardNotice>()
                    .show(cx)
            }
        }
        cx.notify();
    }
}
