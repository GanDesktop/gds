//! Source-independent directory chooser built over reactive signals.

mod commands;
mod model;
mod table;
#[cfg(test)]
mod tests;
mod view;

use std::cell::RefCell;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};

pub use model::{Entry, Location, Page, Request};
pub use view::{
    DirectoryPickerDialog, DirectoryPickerDialogProps, DirectoryPickerView,
    DirectoryPickerViewProps,
};
use zgui::reactive::{LocalStorage, RwSignal, Signal, Task, prelude::*, spawn_local};

use crate::appearance::tokens::DataListStyle;
use model::{History, Navigation};
use table::DirectoryRows;

pub(crate) const MAX_PICKER_HEIGHT: f32 = 440.0;
pub(crate) const MIN_PICKER_HEIGHT: f32 = 200.0;
const SCROLL_PREFETCH_MARGIN: usize = 9;

/// Fixed row height in CSS pixels passed to [`zgui_ui::VirtualList`].
pub const ROW_HEIGHT: f32 = DataListStyle::ROW_HEIGHT;

/// Minimum filtered rows before pagination stops automatic prefetching (`20`).
pub const MIN_SCROLLABLE_ROWS: usize = (MAX_PICKER_HEIGHT as usize)
    .div_ceil(DataListStyle::ROW_HEIGHT as usize)
    + SCROLL_PREFETCH_MARGIN;

/// Remaining-row scroll-prefetch threshold kept strictly smaller than `SCROLL_PREFETCH_MARGIN`.
pub const TABLE_LOAD_MORE_THRESHOLD: usize = 5;

/// Asynchronous directory page loader; dropping the future cancels in-flight work.
pub type Loader = Rc<dyn Fn(Request) -> Pin<Box<dyn Future<Output = Result<Page, String>>>>>;

#[derive(Clone, Copy)]
struct PickerSignals {
    page: RwSignal<Option<Page>, LocalStorage>,
    rows: RwSignal<DirectoryRows, LocalStorage>,
    selected_index: RwSignal<Option<usize>, LocalStorage>,
    selected_path: RwSignal<Option<String>, LocalStorage>,
    editing_location: RwSignal<bool, LocalStorage>,
    address: RwSignal<String, LocalStorage>,
    filter: RwSignal<String, LocalStorage>,
    show_hidden: RwSignal<bool, LocalStorage>,
    pending: RwSignal<bool, LocalStorage>,
    pending_request: RwSignal<Option<Request>, LocalStorage>,
    loading_more: RwSignal<bool, LocalStorage>,
    failed: RwSignal<Option<String>, LocalStorage>,
    can_back: RwSignal<bool, LocalStorage>,
    can_forward: RwSignal<bool, LocalStorage>,
    can_parent: RwSignal<bool, LocalStorage>,
}

impl PickerSignals {
    fn new() -> Self {
        Self {
            page: RwSignal::new_local(None),
            rows: RwSignal::new_local(DirectoryRows::default()),
            selected_index: RwSignal::new_local(None),
            selected_path: RwSignal::new_local(None),
            editing_location: RwSignal::new_local(false),
            address: RwSignal::new_local(String::new()),
            filter: RwSignal::new_local(String::new()),
            show_hidden: RwSignal::new_local(false),
            pending: RwSignal::new_local(false),
            pending_request: RwSignal::new_local(None),
            loading_more: RwSignal::new_local(false),
            failed: RwSignal::new_local(None),
            can_back: RwSignal::new_local(false),
            can_forward: RwSignal::new_local(false),
            can_parent: RwSignal::new_local(false),
        }
    }
}

struct PickerRuntime {
    history: History,
    generation: u64,
    in_flight: Option<Task>,
    failed_request: Option<(Request, Navigation)>,
}

/// Reactive state controller backing [`DirectoryPickerView`].
#[derive(Clone)]
pub struct DirectoryPicker {
    pub(crate) loader: Loader,
    signals: PickerSignals,
    runtime: Rc<RefCell<PickerRuntime>>,
}

impl DirectoryPicker {
    /// Creates a [`DirectoryPicker`] backed by async `loader` without starting an initial load yet.
    #[must_use]
    pub fn new(loader: Loader) -> Self {
        let runtime = PickerRuntime {
            history: History::default(),
            generation: 0,
            in_flight: None,
            failed_request: None,
        };
        Self {
            loader,
            signals: PickerSignals::new(),
            runtime: Rc::new(RefCell::new(runtime)),
        }
    }

    /// Creates a [`DirectoryPicker`] and immediately begins loading `initial_path`.
    #[must_use]
    pub fn with_path(loader: Loader, initial_path: impl Into<String>) -> Self {
        let picker = Self::new(loader);
        picker.load(Navigation::Visit, Some(initial_path.into()), None);
        picker
    }

    /// Starts the initial directory load when no page, error, or active request is present.
    pub fn start(&self) {
        if self.signals.page.with_untracked(Option::is_some)
            || self.signals.pending.get_untracked()
            || self.signals.failed.with_untracked(Option::is_some)
        {
            return;
        }
        self.load(Navigation::Visit, None, None);
    }

    /// Navigates to `path`, clearing any active filter or inline location editor.
    pub fn navigate(&self, path: impl Into<String>) {
        let target = path.into();
        self.signals.editing_location.set(false);
        self.signals.filter.set(String::new());
        self.clear_selection();
        self.load(Navigation::Visit, Some(target), None);
    }

    /// Navigates backward in directory visit history, if available.
    pub fn back(&self) {
        let target = self.runtime.borrow().history.back.last().cloned();
        if let Some(path) = target {
            self.signals.editing_location.set(false);
            self.signals.filter.set(String::new());
            self.clear_selection();
            self.load(Navigation::Back, Some(path), None);
        }
    }

    /// Navigates forward in directory visit history, if available.
    pub fn forward(&self) {
        let target = self.runtime.borrow().history.forward.last().cloned();
        if let Some(path) = target {
            self.signals.editing_location.set(false);
            self.signals.filter.set(String::new());
            self.clear_selection();
            self.load(Navigation::Forward, Some(path), None);
        }
    }

    /// Navigates to the current directory's parent, if one exists.
    pub fn parent(&self) {
        let parent = self
            .signals
            .page
            .with_untracked(|page| page.as_ref().and_then(|p| p.parent.clone()));
        if let Some(path) = parent {
            self.navigate(path);
        }
    }

    /// Retries a failed request or reloads the current location.
    pub fn refresh(&self) {
        let retry = self.runtime.borrow_mut().failed_request.take();
        if let Some((request, kind)) = retry {
            self.load(kind, request.path, request.cursor);
        } else {
            let current = self.current_path();
            self.load(Navigation::Refresh, current, None);
        }
    }

    /// Fetches the next page of directory entries if a continuation cursor is present and no
    /// request or error is currently active.
    pub fn load_more(&self) {
        if self.signals.pending.get_untracked()
            || self.signals.loading_more.get_untracked()
            || self.signals.failed.with_untracked(Option::is_some)
        {
            return;
        }
        let next = self.signals.page.with_untracked(|page| {
            page.as_ref()
                .and_then(|p| p.next_cursor.clone().map(|c| (p.location.path.clone(), c)))
        });
        if let Some((path, cursor)) = next {
            self.load(Navigation::More, Some(path), Some(cursor));
        }
    }

    /// Cancels any active directory fetch, dropping the in-flight future cleanly.
    pub fn cancel_loading(&self) {
        let cancelled = {
            let mut rt = self.runtime.borrow_mut();
            rt.generation = rt.generation.saturating_add(1);
            rt.failed_request = None;
            rt.in_flight.take()
        };
        if let Some(task) = cancelled {
            task.cancel();
        }
        self.signals.pending.set(false);
        self.signals.pending_request.set(None);
        self.signals.loading_more.set(false);
        self.signals.failed.set(None);
        self.signals.editing_location.set(false);
        if let Some(current) = self.current_path() {
            self.signals.address.set(current);
        }
    }

    /// Toggles visibility of hidden (`dot`) directories and prefetches more pages if needed.
    pub fn toggle_hidden(&self) {
        self.signals.show_hidden.update(|show| *show = !*show);
        self.clear_selection();
        self.recompute_rows();
        self.maybe_prefetch_more(true);
    }

    /// Updates the case-insensitive folder filter string and prefetches more pages if fewer than
    /// [`MIN_SCROLLABLE_ROWS`] match.
    pub fn set_filter(&self, query: impl Into<String>) {
        self.signals.filter.set(query.into());
        self.clear_selection();
        self.recompute_rows();
        self.maybe_prefetch_more(true);
    }

    /// Opens the inline editable address bar initialized to the active location path.
    pub fn begin_edit_location(&self) {
        let current = self.current_path().unwrap_or_default();
        self.signals.address.set(current);
        self.signals.editing_location.set(true);
    }

    /// Closes the inline editable address bar without navigating.
    pub fn cancel_edit_location(&self) {
        self.signals.editing_location.set(false);
        if let Some(current) = self.current_path() {
            self.signals.address.set(current);
        }
    }

    /// Updates the editable address string buffer.
    pub fn set_address(&self, address: impl Into<String>) {
        self.signals.address.set(address.into());
    }

    /// Submits `address` and navigates to the trimmed path when non-empty.
    pub fn apply_address(&self, address: impl Into<String>) {
        let trimmed = address.into().trim().to_owned();
        if !trimmed.is_empty() {
            self.navigate(trimmed);
        } else {
            self.cancel_edit_location();
        }
    }

    /// Selects row `index` from the currently visible filtered listing.
    pub fn select_row(&self, index: usize) {
        let path = self
            .signals
            .rows
            .with_untracked(|rows| rows.get(index).map(|entry| entry.location.path.clone()));
        if let Some(path) = path {
            self.signals.selected_index.set(Some(index));
            self.signals.selected_path.set(Some(path));
        }
    }

    /// Clears any explicit child directory selection.
    pub fn clear_selection(&self) {
        self.signals.selected_index.set(None);
        self.signals.selected_path.set(None);
    }

    /// Moves selection up or down within the filtered visible rows.
    pub fn move_selection(&self, delta: isize) {
        let len = self.signals.rows.with_untracked(DirectoryRows::len);
        if len == 0 {
            return;
        }
        let next = match self.signals.selected_index.get_untracked() {
            Some(current) => {
                if delta >= 0 {
                    current.saturating_add(delta.unsigned_abs()).min(len - 1)
                } else {
                    current.saturating_sub(delta.unsigned_abs())
                }
            }
            None if delta >= 0 => 0,
            None => len - 1,
        };
        self.select_row(next);
    }

    /// Selects the first visible directory row, if any.
    pub fn select_first(&self) {
        self.select_row(0);
    }

    /// Selects the last visible directory row, if any.
    pub fn select_last(&self) {
        let len = self.signals.rows.with_untracked(DirectoryRows::len);
        if len > 0 {
            self.select_row(len - 1);
        }
    }

    /// Navigates into the currently selected directory row, if one is selected.
    pub fn open_selected(&self) {
        if let Some(path) = self.signals.selected_path.get_untracked() {
            self.navigate(path);
        }
    }

    /// Path of the currently loaded location, if any.
    #[must_use]
    pub fn current_path(&self) -> Option<String> {
        self.signals
            .page
            .with_untracked(|page| page.as_ref().map(|p| p.location.path.clone()))
    }

    /// Selected child directory path if a row is highlighted, otherwise the currently open
    /// directory path. Pending navigation, failed reads, and unsubmitted location edits return
    /// `None`.
    #[must_use]
    pub fn selected_path(&self) -> Option<String> {
        self.selected_location().map(|loc| loc.path)
    }

    /// Selected child [`Location`] if a row is highlighted, otherwise the currently open page's
    /// [`Location`]. Pending navigation, failed reads, and unsubmitted location edits return
    /// `None`.
    #[must_use]
    pub fn selected_location(&self) -> Option<Location> {
        if self.signals.pending.get_untracked()
            || self.signals.loading_more.get_untracked()
            || self.signals.failed.with_untracked(Option::is_some)
            || self.signals.editing_location.get_untracked()
        {
            return None;
        }
        if let Some(selected) = self.signals.selected_path.get_untracked()
            && let Some(loc) = self.signals.rows.with_untracked(|rows| {
                rows.entries
                    .iter()
                    .find(|entry| entry.location.path == selected)
                    .map(|entry| entry.location.clone())
            })
        {
            return Some(loc);
        }
        self.signals
            .page
            .with_untracked(|page| page.as_ref().map(|p| p.location.clone()))
    }

    /// Reactive signal indicating whether the current choice can be accepted (`selected_path().is_some()`).
    #[must_use]
    pub fn can_accept(&self) -> Signal<bool, LocalStorage> {
        let sigs = self.signals;
        Signal::derive_local(move || {
            !sigs.pending.get()
                && !sigs.loading_more.get()
                && sigs.failed.with(Option::is_none)
                && !sigs.editing_location.get()
                && sigs.page.with(Option::is_some)
        })
    }

    /// Reactive signal carrying the currently chosen path (selected child or loaded page path)
    /// displayed in [`crate::ui::picker::PickerStatus`].
    #[must_use]
    pub fn chosen_path(&self) -> Signal<String, LocalStorage> {
        let sigs = self.signals;
        Signal::derive_local(move || {
            if let Some(selected) = sigs.selected_path.get()
                && sigs.rows.with(|rows| {
                    rows.entries
                        .iter()
                        .any(|entry| entry.location.path == selected)
                })
            {
                return selected;
            }
            sigs.page
                .with(|page| page.as_ref().map(|p| p.location.path.clone()))
                .unwrap_or_default()
        })
    }

    /// Signal carrying the active in-flight navigation [`Request`], if any.
    #[must_use]
    pub fn pending_request(&self) -> Signal<Option<Request>, LocalStorage> {
        self.signals.pending_request.into()
    }

    /// Signal carrying the active directory [`Page`].
    #[must_use]
    pub fn page(&self) -> Signal<Option<Page>, LocalStorage> {
        self.signals.page.into()
    }

    /// Signal carrying the filtered visible rows.
    #[must_use]
    pub fn rows(&self) -> Signal<DirectoryRows, LocalStorage> {
        self.signals.rows.into()
    }

    /// Signal carrying the currently selected row index.
    #[must_use]
    pub fn selected_index(&self) -> Signal<Option<usize>, LocalStorage> {
        self.signals.selected_index.into()
    }

    /// Signal indicating whether an initial/navigation directory load is in progress.
    #[must_use]
    pub fn is_loading(&self) -> Signal<bool, LocalStorage> {
        self.signals.pending.into()
    }

    /// Signal indicating whether pagination append (`load_more`) is currently running.
    #[must_use]
    pub fn is_loading_more(&self) -> Signal<bool, LocalStorage> {
        self.signals.loading_more.into()
    }

    /// Signal carrying the latest load error message, if any.
    #[must_use]
    pub fn error(&self) -> Signal<Option<String>, LocalStorage> {
        self.signals.failed.into()
    }

    /// Signal indicating whether hidden (`dot`) folders are currently shown.
    #[must_use]
    pub fn show_hidden(&self) -> Signal<bool, LocalStorage> {
        self.signals.show_hidden.into()
    }

    /// Signal carrying the active folder filter query string.
    #[must_use]
    pub fn filter_query(&self) -> Signal<String, LocalStorage> {
        self.signals.filter.into()
    }

    /// Signal indicating whether the inline address input is open.
    #[must_use]
    pub fn is_editing_location(&self) -> Signal<bool, LocalStorage> {
        self.signals.editing_location.into()
    }

    /// Signal carrying the editable location bar string.
    #[must_use]
    pub fn address_text(&self) -> Signal<String, LocalStorage> {
        self.signals.address.into()
    }

    /// Signal indicating whether backward navigation history is available.
    #[must_use]
    pub fn can_go_back(&self) -> Signal<bool, LocalStorage> {
        self.signals.can_back.into()
    }

    /// Signal indicating whether forward navigation history is available.
    #[must_use]
    pub fn can_go_forward(&self) -> Signal<bool, LocalStorage> {
        self.signals.can_forward.into()
    }

    /// Signal indicating whether the active directory has a parent directory.
    #[must_use]
    pub fn can_go_parent(&self) -> Signal<bool, LocalStorage> {
        self.signals.can_parent.into()
    }

    fn load(&self, kind: Navigation, path: Option<String>, cursor: Option<String>) {
        let is_more = kind == Navigation::More;
        let loader = Rc::clone(&self.loader);
        let (previous_task, load_gen) = {
            let mut rt = self.runtime.borrow_mut();
            rt.generation = rt.generation.saturating_add(1);
            rt.failed_request = None;
            let load_gen = rt.generation;
            let prev = rt.in_flight.take();
            (prev, load_gen)
        };
        if let Some(prev) = previous_task {
            prev.cancel();
        }

        let request = Request {
            path,
            cursor: cursor.clone(),
        };

        let previous_path = self.current_path();
        if is_more {
            self.signals.loading_more.set(true);
            self.signals.pending_request.set(None);
        } else {
            self.signals.pending.set(true);
            self.signals.pending_request.set(Some(request.clone()));
            self.signals.loading_more.set(false);
        }
        self.signals.failed.set(None);

        let future = loader(request.clone());

        let weak_rt: Weak<RefCell<PickerRuntime>> = Rc::downgrade(&self.runtime);
        let picker_clone = self.clone();

        let task = spawn_local(async move {
            let result = future.await;
            let Some(rt) = weak_rt.upgrade() else {
                return;
            };
            let is_current = {
                let mut guard = rt.borrow_mut();
                if guard.generation != load_gen {
                    false
                } else {
                    let cleared = guard.in_flight.take();
                    drop(cleared);
                    true
                }
            };
            if !is_current {
                return;
            }
            picker_clone.finish_load(kind, previous_path, request, result);
        });

        let stale_task = {
            let mut rt = self.runtime.borrow_mut();
            if rt.generation == load_gen {
                rt.in_flight.replace(task)
            } else {
                Some(task)
            }
        };
        if let Some(stale) = stale_task {
            stale.cancel();
        }
    }

    fn finish_load(
        &self,
        kind: Navigation,
        previous_path: Option<String>,
        request: Request,
        result: Result<Page, String>,
    ) {
        self.signals.pending.set(false);
        self.signals.pending_request.set(None);
        self.signals.loading_more.set(false);

        let mut cursor_progressed = false;
        match result {
            Ok(loaded) => {
                self.signals.failed.set(None);
                if kind == Navigation::More {
                    let mut accepted = false;
                    self.signals.page.update(|slot| {
                        if let Some(existing) = slot
                            && existing.location.path == loaded.location.path
                        {
                            accepted = true;
                            let mut seen: HashSet<String> = existing
                                .entries
                                .iter()
                                .map(|entry| entry.location.path.clone())
                                .collect();
                            for entry in loaded.entries {
                                if seen.insert(entry.location.path.clone()) {
                                    existing.entries.push(entry);
                                }
                            }
                            cursor_progressed = loaded.next_cursor != request.cursor;
                            existing.next_cursor = loaded.next_cursor;
                        }
                    });
                    if !accepted {
                        return;
                    }
                } else {
                    let (can_back, can_forward) = {
                        let mut rt = self.runtime.borrow_mut();
                        rt.history
                            .commit(kind, previous_path.as_deref(), &loaded.location.path);
                        (!rt.history.back.is_empty(), !rt.history.forward.is_empty())
                    };
                    cursor_progressed = loaded.next_cursor.is_some();
                    self.signals.can_back.set(can_back);
                    self.signals.can_forward.set(can_forward);
                    self.signals.can_parent.set(loaded.parent.is_some());
                    self.signals.address.set(loaded.location.path.clone());
                    self.signals.editing_location.set(false);
                    self.signals.page.set(Some(loaded));
                }
                self.recompute_rows();
                self.maybe_prefetch_more(cursor_progressed);
            }
            Err(message) => {
                self.runtime.borrow_mut().failed_request = Some((request, kind));
                self.signals.failed.set(Some(message));
            }
        }
    }

    fn recompute_rows(&self) {
        let show_hidden = self.signals.show_hidden.get_untracked();
        let query = self.signals.filter.get_untracked();
        let next_rows = self
            .signals
            .page
            .with_untracked(|page| DirectoryRows::from_page(page.as_ref(), show_hidden, &query));
        let selected_idx = self
            .signals
            .selected_path
            .get_untracked()
            .and_then(|path| next_rows.index_of_path(&path));
        if selected_idx.is_none() {
            self.signals.selected_path.set(None);
        }
        self.signals.selected_index.set(selected_idx);
        self.signals.rows.set(next_rows);
    }

    fn maybe_prefetch_more(&self, cursor_progressed: bool) {
        if !cursor_progressed {
            return;
        }
        let visible_count = self.signals.rows.with_untracked(DirectoryRows::len);
        let has_cursor = self
            .signals
            .page
            .with_untracked(|page| page.as_ref().and_then(|p| p.next_cursor.as_ref()).is_some());
        if visible_count < MIN_SCROLLABLE_ROWS && has_cursor {
            self.load_more();
        }
    }
}
