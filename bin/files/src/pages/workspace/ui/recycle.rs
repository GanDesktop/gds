//! Trash browsing and recovery over the platform trash store.
//!
//! Every filesystem effect belongs to [`backend`]; this module owns the listing
//! model, the id-based selection and the frozen target sets handed to
//! destructive actions. A trashed record is selected by its opaque trash id,
//! never by its original path: several records may share one original path, and
//! an original path is display data rather than a current, deletable file.

mod backend;
mod table;
mod view;

use std::{collections::HashSet, ffi::OsString, path::PathBuf, sync::Arc};

use fluent_bundle::FluentArgs;
use gds::{i18n, ui::notifications};
use gpui_kit::component::table::TableState;
use gpui_kit::*;

use self::{
    backend::{Action, Entry, Error, Outcome},
    table::TrashTable,
};
use super::{Browser, Collection, selection::Motion};

/// One toast slot for failed listings.
pub(super) struct LoadNotice;

/// What a completed listing did to the visible model.
pub(super) enum Load {
    /// A newer request replaced this one; ignore the result completely.
    Stale,
    Applied,
    Failed(String),
}

#[derive(Clone, Default)]
pub(super) struct ViewState {
    selected: HashSet<OsString>,
    cursor: Option<OsString>,
    anchor: Option<OsString>,
    scroll_offset: f32,
}

pub(super) struct State {
    generation: u64,
    loading: bool,
    error: Option<String>,
    entries: Arc<[Entry]>,
    selected: HashSet<OsString>,
    cursor: Option<OsString>,
    anchor: Option<OsString>,
    busy: bool,
    task_paths: Vec<PathBuf>,
    table: Entity<TableState<TrashTable>>,
    supported: bool,
}

impl State {
    pub(super) fn new(
        browser: WeakEntity<Browser>,
        window: &mut Window,
        cx: &mut Context<Browser>,
    ) -> Self {
        Self {
            generation: 0,
            loading: false,
            error: None,
            entries: Arc::from([]),
            selected: HashSet::new(),
            cursor: None,
            anchor: None,
            busy: false,
            task_paths: Vec::new(),
            table: TrashTable::make_state(browser, window, cx),
            supported: backend::supported(),
        }
    }

    pub(super) fn view(&self, cx: &App) -> ViewState {
        ViewState {
            selected: self.selected.clone(),
            cursor: self.cursor.clone(),
            anchor: self.anchor.clone(),
            scroll_offset: self
                .table
                .read(cx)
                .vertical_scroll_handle
                .0
                .borrow()
                .base_handle
                .offset()
                .y
                .into(),
        }
    }
    pub(super) fn restore_view(&mut self, view: ViewState, cx: &App) {
        self.selected = view.selected;
        self.cursor = view.cursor;
        self.anchor = view.anchor;
        self.table
            .read(cx)
            .vertical_scroll_handle
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(view.scroll_offset)));
    }

    pub(super) fn rows(&self) -> &[Entry] {
        &self.entries
    }

    pub(super) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    pub(super) fn task_paths(&self) -> &[PathBuf] {
        &self.task_paths
    }

    /// Whether retrying a failed listing can help on this platform.
    pub(super) fn supported(&self) -> bool {
        self.supported
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn selected_count(&self) -> usize {
        self.selected.len()
    }

    pub(super) fn selection(&self) -> HashSet<OsString> {
        self.selected.clone()
    }

    pub(super) fn scroll_handle(&self, cx: &App) -> UniformListScrollHandle {
        self.table.read(cx).vertical_scroll_handle.clone()
    }

    /// Frozen entries in list order.
    ///
    /// Two records that share an original path stay two independent targets.
    pub(super) fn selected_entries(&self) -> Vec<Entry> {
        self.entries
            .iter()
            .filter(|entry| self.selected.contains(&entry.id))
            .cloned()
            .collect()
    }

    fn index_of(&self, id: &OsString) -> Option<usize> {
        self.entries.iter().position(|entry| &entry.id == id)
    }

    pub(super) fn cursor_index(&self) -> Option<usize> {
        self.cursor.as_ref().and_then(|id| self.index_of(id))
    }

    fn is_selected_at(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| self.selected.contains(&entry.id))
    }

    fn begin_load(&mut self) -> u64 {
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.generation
    }

    fn apply_load(&mut self, generation: u64, result: Result<Vec<Entry>, backend::Error>) -> Load {
        if generation != self.generation {
            return Load::Stale;
        }
        self.loading = false;
        match result {
            Ok(entries) => {
                self.entries = entries.into();
                self.error = None;
                self.retain_existing_selection();
                Load::Applied
            }
            Err(error) => {
                let message = error.to_string();
                // A failed refresh keeps the previous rows visible; only an
                // empty list turns the failure into the whole content state.
                if self.entries.is_empty() {
                    self.error = Some(message.clone());
                }
                Load::Failed(message)
            }
        }
    }

    fn fail(&mut self, message: String) {
        self.loading = false;
        self.entries = Arc::from([]);
        self.clear_selection();
        self.error = Some(message);
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected.clear();
        self.cursor = None;
        self.anchor = None;
    }

    fn retain_existing_selection(&mut self) {
        let existing: HashSet<&OsString> = self.entries.iter().map(|entry| &entry.id).collect();
        self.selected.retain(|id| existing.contains(id));
        if self
            .cursor
            .as_ref()
            .is_some_and(|id| !existing.contains(id))
        {
            self.cursor = None;
        }
        if self
            .anchor
            .as_ref()
            .is_some_and(|id| !existing.contains(id))
        {
            self.anchor = None;
        }
    }

    fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    fn select(&mut self, index: usize, modifiers: Modifiers) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let id = entry.id.clone();
        if modifiers.shift {
            let anchor = self.anchor.clone().unwrap_or_else(|| id.clone());
            let anchor_index = self.index_of(&anchor).unwrap_or(index);
            let (start, end) = (anchor_index.min(index), anchor_index.max(index));
            self.selected = self.entries[start..=end]
                .iter()
                .map(|entry| entry.id.clone())
                .collect();
            self.anchor = Some(anchor);
        } else if modifiers.control || modifiers.platform {
            if !self.selected.remove(&id) {
                self.selected.insert(id.clone());
            }
            self.anchor = Some(id.clone());
        } else {
            self.selected.clear();
            self.selected.insert(id.clone());
            self.anchor = Some(id.clone());
        }
        self.cursor = Some(id);
    }

    fn select_all(&mut self) {
        self.selected = self.entries.iter().map(|entry| entry.id.clone()).collect();
        if self.cursor.is_none() {
            self.cursor = self.entries.first().map(|entry| entry.id.clone());
        }
        self.anchor = self.cursor.clone();
    }
}

impl Browser {
    pub(super) fn trash_selection_menu(
        &self,
        menu: gpui_kit::component::menu::PopupMenu,
        cx: &Context<Self>,
    ) -> gpui_kit::component::menu::PopupMenu {
        view::entry_menu(self, menu, cx)
    }

    pub(super) fn load_trash(&mut self, cx: &mut Context<Self>) {
        if !backend::supported() {
            self.trash
                .fail(i18n::text("recycle-unsupported", cx).to_string());
            cx.notify();
            return;
        }
        let generation = self.trash.begin_load();
        let work = cx
            .background_executor()
            .spawn(async move { backend::list() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                match this.trash.apply_load(generation, result) {
                    Load::Stale => {}
                    Load::Applied => cx.notify(),
                    Load::Failed(message) => {
                        notifications::Report::new(
                            notifications::Level::Error,
                            i18n::text("recycle-load-failed", cx),
                        )
                        .detail(message)
                        .replacing::<LoadNotice>()
                        .show(cx);
                        cx.notify();
                    }
                }
            }) {
                tracing::warn!("Trash listing arrived after the window closed: {error}");
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn refresh_trash(&mut self, cx: &mut Context<Self>) {
        self.load_trash(cx);
    }

    pub(super) fn select_trash(
        &mut self,
        index: usize,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        self.trash.select(index, modifiers);
        cx.notify();
    }

    pub(super) fn trash_select_all(&mut self, cx: &mut Context<Self>) {
        if self.collection() != Some(Collection::Trash) {
            return;
        }
        self.trash.select_all();
        cx.notify();
    }

    pub(super) fn select_trash_row_for_menu(&mut self, index: usize) {
        if !self.trash.is_selected_at(index) {
            self.trash.select(index, Modifiers::default());
        }
    }

    pub(super) fn restore_selected_trash(&mut self, cx: &mut Context<Self>) {
        let entries = self.trash.selected_entries();
        self.start_trash_operation(Action::Restore, entries, cx);
    }

    pub(super) fn request_trash_purge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.trash.selected_entries();
        self.confirm_purge_entries(entries, window, cx);
    }

    pub(super) fn confirm_purge_entries(
        &mut self,
        entries: Vec<Entry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.trash.is_busy() || entries.is_empty() {
            return;
        }
        view::Confirmation::open(
            cx.entity().downgrade(),
            view::Intent::PurgeSelection,
            entries,
            window,
        );
        cx.notify();
    }

    pub(super) fn request_empty_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.trash.is_busy() {
            return;
        }
        // Freeze exactly the rows the user is looking at; anything deleted into
        // Trash after this point is not part of the confirmed request.
        let entries = self.trash.rows().to_vec();
        if entries.is_empty() {
            return;
        }
        view::Confirmation::open(
            cx.entity().downgrade(),
            view::Intent::EmptyTrash,
            entries,
            window,
        );
        cx.notify();
    }

    pub(super) fn start_trash_operation(
        &mut self,
        action: Action,
        entries: Vec<Entry>,
        cx: &mut Context<Self>,
    ) {
        if entries.is_empty() || self.trash.is_busy() {
            return;
        }
        if matches!(action, Action::Restore)
            && entries
                .iter()
                .any(|entry| !self.device_path_accepts_task(&entry.original_path, cx))
        {
            notifications::error(i18n::text("device-path-unavailable", cx), cx);
            return;
        }
        self.trash.set_busy(true);
        self.trash.task_paths = entries
            .iter()
            .map(|entry| entry.original_path.clone())
            .collect();
        let count = entries.len();
        notifications::Operation::start(i18n::count(action_labels(action).working, count, cx), cx);
        let work = cx
            .background_executor()
            .spawn(async move { backend::perform(action, entries) });
        cx.spawn(async move |this, cx| {
            let outcomes = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                this.finish_trash_operation(action, outcomes, cx)
            }) {
                tracing::warn!("Trash operation arrived after the window closed: {error}");
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_trash_operation(
        &mut self,
        action: Action,
        outcomes: Vec<Outcome>,
        cx: &mut Context<Self>,
    ) {
        self.trash.set_busy(false);
        self.trash.task_paths.clear();
        if matches!(action, Action::Restore) {
            // A payload that already moved back belongs to its folder again
            // even when the trash record cleanup failed afterwards.
            let restored: Vec<PathBuf> = outcomes
                .iter()
                .filter(|outcome| Effect::of(outcome).moved())
                .map(|outcome| outcome.original_path.clone())
                .collect();
            if !restored.is_empty() {
                self.refresh_after_file_operation(&restored, cx);
            }
        }
        let report = outcome_report(action, &outcomes, cx);
        // Re-read the store instead of patching the list optimistically: both
        // an untouched record and one whose cleanup failed must show up as the
        // store really reports it.
        self.load_trash(cx);
        notifications::Operation::finish(report, cx);
        cx.notify();
    }

    /// Move the Trash cursor one list motion.
    ///
    /// Returns whether the motion applied, so a caller can tell whether it
    /// really moved the selection and should stop the key from travelling on.
    pub(super) fn move_trash_cursor(
        &mut self,
        motion: Motion,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.collection() != Some(Collection::Trash) {
            return false;
        }
        let current = self.trash.cursor_index();
        let Some(index) = motion.target(current, self.trash.rows().len()) else {
            return false;
        };
        self.trash.select(
            index,
            Modifiers {
                shift: extend,
                ..Default::default()
            },
        );
        self.trash
            .table
            .update(cx, |table, cx| table.scroll_to_row(index, cx));
        cx.notify();
        true
    }

    pub(super) fn on_trash_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.collection() != Some(Collection::Trash) || !self.list_keys_available(window, cx) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        let Some(motion) = Motion::for_key(&event.keystroke.key) else {
            return;
        };
        if self.move_trash_cursor(motion, modifiers.shift, cx) {
            self.focus.focus(window, cx);
            cx.stop_propagation();
            window.prevent_default();
        }
    }

    pub(super) fn on_trash_restore(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.restore_selected_trash(cx);
    }

    pub(super) fn on_trash_purge(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_trash_purge(window, cx);
    }

    pub(super) fn on_trash_empty(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_empty_trash(window, cx);
    }

    pub(super) fn on_trash_refresh(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_trash(cx);
    }

    pub(super) fn render_trash(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        view::render_trash(self, window, cx)
    }
}

/// What one attempted record did to its payload on disk.
enum Effect {
    /// The payload moved to its target and the record was cleaned up.
    Done,
    /// The payload moved, but its trash record was left behind.
    RecordLeft,
    /// The payload was untouched; the record stays in Trash.
    Untouched,
}

impl Effect {
    /// Classify one outcome. `RestoreIncomplete` and `PurgeIncomplete` mean the
    /// payload already moved or was removed and only the record cleanup failed,
    /// so they must not be reported as if the record were still intact.
    fn of(outcome: &Outcome) -> Self {
        match &outcome.result {
            Ok(()) => Self::Done,
            Err(Error::RestoreIncomplete { .. } | Error::PurgeIncomplete { .. }) => {
                Self::RecordLeft
            }
            Err(_) => Self::Untouched,
        }
    }

    /// Whether the payload already reached its target, cleanup aside.
    fn moved(self) -> bool {
        !matches!(self, Self::Untouched)
    }
}

/// The result notice for one batch: a summary that only claims what the
/// outcomes actually did, plus one detail line per record that was not clean.
fn outcome_report(action: Action, outcomes: &[Outcome], cx: &App) -> notifications::Report {
    let moved = outcomes
        .iter()
        .filter(|outcome| Effect::of(outcome).moved())
        .count();
    let record_left = outcomes
        .iter()
        .filter(|outcome| matches!(Effect::of(outcome), Effect::RecordLeft))
        .count();
    let untouched = outcomes.len() - moved;
    let labels = action_labels(action);
    let (level, summary) = match (untouched, record_left) {
        (0, 0) => (
            notifications::Level::Success,
            i18n::count(labels.done, moved, cx),
        ),
        // Every payload moved; only record cleanup is outstanding. Details name
        // the records that were left behind.
        (0, left) => {
            let mut arguments = FluentArgs::new();
            arguments.set("count", moved as i64);
            arguments.set("records_left", left as i64);
            (
                notifications::Level::Error,
                i18n::format(labels.records_left, Some(&arguments), cx),
            )
        }
        // Mixed batches lead with the untouched records; the details still note
        // any record whose cleanup failed.
        _ => {
            let mut arguments = FluentArgs::new();
            arguments.set("succeeded", moved as i64);
            arguments.set("failed", untouched as i64);
            (
                notifications::Level::Error,
                i18n::format(labels.partial, Some(&arguments), cx),
            )
        }
    };
    let mut report = notifications::Report::new(level, summary);
    for outcome in outcomes.iter().filter(|outcome| outcome.result.is_err()) {
        if let Err(error) = &outcome.result {
            let path = outcome
                .original_path
                .as_os_str()
                .to_string_lossy()
                .escape_debug()
                .to_string();
            report = report.detail(format!("{path}: {error}"));
        }
    }
    report
}

/// Fluent keys for one action's progress and result notices.
struct Labels {
    working: &'static str,
    done: &'static str,
    /// Some payloads were left untouched in Trash.
    partial: &'static str,
    /// Every payload moved; some trash records could not be cleaned up.
    records_left: &'static str,
}

fn action_labels(action: Action) -> Labels {
    match action {
        Action::Restore => Labels {
            working: "recycle-restore-working",
            done: "recycle-restore-done",
            partial: "recycle-restore-partial",
            records_left: "recycle-restore-records-left",
        },
        Action::Purge => Labels {
            working: "recycle-purge-working",
            done: "recycle-purge-done",
            partial: "recycle-purge-partial",
            records_left: "recycle-purge-records-left",
        },
    }
}
