mod actions;
mod name_prompt;
mod table;
#[cfg(test)]
mod tests;

use crate::{
    features::{connect_server::Connector, file_operations::remote as operations},
    shared::{
        lib::selection::{Selection, SelectionBacking},
        ui::actions::Command,
    },
};
use gds::{
    i18n::{t, text},
    service::{
        desktop,
        remote::{Address, Entry, EntryKind, Listing, Session},
        task_manager::{Owner, Scope},
    },
    ui::{IconButton, notifications},
};
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{
        Sizable,
        menu::PopupMenuItem,
        table::{DataTable, TableState},
    },
    *,
};
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

pub(crate) struct Options {
    pub tasks: Scope,
    pub session: Session,
    pub listing: Listing,
    pub connector: Entity<Connector>,
    pub desktop: desktop::Desktop,
    pub favorite: bool,
    /// Filter input state owned by the workspace toolbar; filtering is driven via [`View::set_filter`].
    #[allow(dead_code)]
    pub filter: Entity<gpui_kit::component::input::InputState>,
    pub focus: FocusHandle,
}

pub(crate) enum Event {
    Navigate(Address),
    Refresh,
    Favorite(Address),
    Disconnect(Address),
}

#[derive(Clone, Debug)]
pub(crate) struct RunningTask {
    pub label: String,
    pub cancel: CancellationToken,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub visible: usize,
    pub selected: usize,
}

pub(super) enum Job {
    Mutating {
        label: SharedString,
        _task: Task<()>,
    },
    Transferring {
        cancel: CancellationToken,
        progress: operations::Progress,
        _progress_task: Task<()>,
        _task: Task<()>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToolbarAction {
    Upload,
    Download,
    NewFolder,
    Rename,
    Delete,
    Bookmark,
    Disconnect,
}

impl ToolbarAction {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Upload => "remote-upload",
            Self::Download => "remote-download",
            Self::NewFolder => "new-folder",
            Self::Rename => "rename",
            Self::Delete => "delete-permanently",
            Self::Bookmark => "remote-bookmark",
            Self::Disconnect => "remote-disconnect",
        }
    }

    pub(super) fn label(self, cx: &App) -> SharedString {
        text(self.id(), cx)
    }
}

struct ToolbarItem {
    action: ToolbarAction,
    icon: IconName,
    enabled: bool,
}

pub(crate) struct View {
    options: Options,
    tasks: Owner,
    table: Entity<TableState<table::Files>>,
    rows: Arc<[Entry]>,
    row_backing: Option<Arc<SelectionBacking<Address>>>,
    selected: Selection<Address>,
    anchor: Option<usize>,
    cursor: Option<usize>,
    filter: String,
    show_hidden: bool,
    compact: bool,
    job: Option<Job>,
    message: Option<String>,
}

impl EventEmitter<Event> for View {}

impl View {
    pub(crate) fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tasks = options.tasks.named_child("Remote browser");
        let show_hidden = false;
        let rows = Self::filter_entries(&options.listing.entries, "", show_hidden);
        let table = table::Files::state(cx.entity().downgrade(), window, cx);
        let view = Self {
            options,
            tasks,
            table,
            rows,
            row_backing: None,
            selected: Selection::new(),
            anchor: None,
            cursor: None,
            filter: String::new(),
            show_hidden,
            compact: false,
            job: None,
            message: None,
        };
        view.sync_table(cx);
        view
    }

    fn row_backing(&mut self) -> Arc<SelectionBacking<Address>> {
        if let Some(backing) = &self.row_backing {
            return Arc::clone(backing);
        }
        let backing =
            SelectionBacking::from_items(self.rows.iter().map(|entry| entry.address.clone()));
        self.row_backing = Some(Arc::clone(&backing));
        backing
    }

    pub(super) fn filter_entries(
        entries: &[Entry],
        filter: &str,
        show_hidden: bool,
    ) -> Arc<[Entry]> {
        let trimmed = filter.trim();
        if trimmed.is_empty() {
            return entries
                .iter()
                .filter(|entry| show_hidden || !entry.name.starts_with('.'))
                .cloned()
                .collect::<Vec<_>>()
                .into();
        }
        let needle = trimmed.to_lowercase();
        entries
            .iter()
            .filter(|entry| {
                (show_hidden || !entry.name.starts_with('.'))
                    && if entry.name.is_ascii() && needle.is_ascii() {
                        entry
                            .name
                            .as_bytes()
                            .windows(needle.len())
                            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
                    } else {
                        entry.name.to_lowercase().contains(&needle)
                    }
            })
            .cloned()
            .collect::<Vec<_>>()
            .into()
    }

    pub(crate) fn running_task(&self) -> Option<RunningTask> {
        let Some(Job::Transferring {
            cancel, progress, ..
        }) = &self.job
        else {
            return None;
        };
        Some(RunningTask {
            label: format!("{} — {}", self.options.listing.address, progress.current),
            cancel: cancel.clone(),
        })
    }

    /// Active transfer task summary and cancellation handle for the workspace status bar.
    pub(crate) fn background_task(&self) -> Option<(String, CancellationToken)> {
        self.running_task().map(|task| (task.label, task.cancel))
    }

    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn selected_addresses(&self) -> Vec<Address> {
        self.selected.to_vec()
    }

    pub(crate) fn deselect_address(&mut self, address: &Address, cx: &mut Context<Self>) {
        if self.selected.remove(address) {
            self.sync_table(cx);
        }
    }

    pub(crate) fn item_counts(&self) -> Counts {
        Counts {
            visible: self.rows.len(),
            selected: self.selected.len(),
        }
    }

    /// Visible and selected entry counts as a tuple for the workspace status bar.
    pub(crate) fn counts(&self) -> (usize, usize) {
        let counts = self.item_counts();
        (counts.visible, counts.selected)
    }

    pub(crate) fn address(&self) -> &Address {
        &self.options.listing.address
    }

    pub(super) fn is_filtered(&self) -> bool {
        !self.filter.trim().is_empty()
    }

    pub(crate) fn set_filter(&mut self, filter: String, show_hidden: bool, cx: &mut Context<Self>) {
        if self.filter == filter && self.show_hidden == show_hidden {
            return;
        }
        self.filter = filter;
        self.show_hidden = show_hidden;
        self.anchor = None;
        self.cursor = None;
        self.row_backing = None;
        self.rows = Self::filter_entries(
            &self.options.listing.entries,
            &self.filter,
            self.show_hidden,
        );
        Self::retain_visible_selection(&self.rows, &mut self.selected);
        self.sync_table(cx);
    }

    pub(super) fn retain_visible_selection(rows: &[Entry], selected: &mut Selection<Address>) {
        let visible: HashSet<&Address> = rows.iter().map(|entry| &entry.address).collect();
        // Hidden or filtered-out entries must not remain implicit targets of remote actions.
        selected.retain(|address| visible.contains(address));
    }

    fn sync_table(&self, cx: &mut Context<Self>) {
        let rows = Arc::clone(&self.rows);
        let selected = self.selected.clone();
        self.table.update(cx, |state, cx| {
            Self::sync_table_state(state, rows, selected, cx);
        });
        cx.notify();
    }

    fn sync_table_state(
        state: &mut TableState<table::Files>,
        rows: Arc<[Entry]>,
        selected: Selection<Address>,
        cx: &mut Context<TableState<table::Files>>,
    ) {
        let delegate = state.delegate_mut();
        let rows_changed = !Arc::ptr_eq(&delegate.rows, &rows);
        let selection_changed = delegate.selected != selected;
        if rows_changed {
            delegate.rows = rows;
        }
        if selection_changed {
            delegate.selected = selected;
        }
        // Refresh table state only when the row slice changes; selection-only updates
        // notify the table in place without resetting scroll state or copying rows.
        if rows_changed {
            state.refresh(cx);
        } else if selection_changed {
            cx.notify();
        }
    }

    pub(super) fn select(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.options.focus.focus(window, cx);
        let modifiers = window.modifiers();
        let entry = if modifiers.shift {
            let Some(entry) = self.rows.get(index).cloned() else {
                return;
            };
            let start = self.anchor.unwrap_or(index);
            let backing = self.row_backing();
            self.selected
                .set_range(&backing, start.min(index), start.max(index));
            entry
        } else {
            let Some(entry) = Self::apply_selection(
                &self.rows,
                &mut self.selected,
                &mut self.anchor,
                index,
                false,
                modifiers.control || modifiers.platform,
            ) else {
                return;
            };
            entry
        };
        self.cursor = Some(index);
        self.sync_table(cx);
        if event.click_count() == 2 {
            self.activate(entry, window, cx);
        }
    }

    pub(super) fn apply_selection(
        rows: &[Entry],
        selected: &mut Selection<Address>,
        anchor: &mut Option<usize>,
        index: usize,
        shift: bool,
        toggle: bool,
    ) -> Option<Entry> {
        let entry = rows.get(index)?.clone();
        if shift {
            let start = anchor.unwrap_or(index);
            selected.replace(
                rows[start.min(index)..=start.max(index)]
                    .iter()
                    .map(|item| item.address.clone()),
            );
        } else if toggle {
            selected.toggle(entry.address.clone());
            *anchor = Some(index);
        } else {
            selected.set_single(entry.address.clone());
            *anchor = Some(index);
        }
        Some(entry)
    }

    fn activate(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        if entry.kind == EntryKind::Directory {
            cx.emit(Event::Navigate(entry.address));
        } else {
            self.download(vec![entry.address], true, window, cx);
        }
    }

    fn resources(&self) -> Vec<operations::Resource> {
        self.rows
            .iter()
            .filter(|entry| self.selected.contains(&entry.address))
            .map(|entry| operations::Resource::Remote {
                address: entry.address.clone(),
                session: self.options.session.clone(),
            })
            .collect()
    }

    fn target(&self) -> operations::Resource {
        operations::Resource::Remote {
            address: self.address().clone(),
            session: self.options.session.clone(),
        }
    }

    fn cancel_transfer(&mut self) {
        if let Some(Job::Transferring { cancel, .. }) = &self.job {
            cancel.cancel();
        }
    }

    pub(crate) fn command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(job) = &self.job {
            if command == Command::Cancel
                && let Job::Transferring { cancel, .. } = job
            {
                cancel.cancel();
                return true;
            }
            return matches!(
                command,
                Command::Open
                    | Command::NewFolder
                    | Command::Rename
                    | Command::Trash
                    | Command::DeletePermanent
                    | Command::Paste
                    | Command::Cut
            );
        }
        match command {
            Command::Open => {
                if let Some(entry) = self
                    .rows
                    .iter()
                    .find(|entry| self.selected.contains(&entry.address))
                    .cloned()
                {
                    self.activate(entry, window, cx);
                }
            }
            Command::SelectAll => {
                if self.rows.is_empty() {
                    self.selected.clear();
                } else {
                    let last = self.rows.len() - 1;
                    let backing = self.row_backing();
                    self.selected.set_range(&backing, 0, last);
                }
                self.sync_table(cx);
            }
            Command::DeselectAll => {
                self.selected.clear();
                self.cursor = None;
                self.anchor = None;
                self.sync_table(cx);
            }
            Command::CopyPaths => {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    self.selected
                        .iter()
                        .map(Address::uri)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ));
            }
            Command::Copy => self.copy_uris(cx),
            Command::Cut => {
                self.message = Some(t!(@cx, "remote-cut-unavailable").into());
                cx.notify();
            }
            Command::Paste => self.paste(cx),
            Command::NewFolder => self.name_prompt(true, window, cx),
            Command::Rename => self.name_prompt(false, window, cx),
            Command::Trash | Command::DeletePermanent => self.confirm_delete(window, cx),
            Command::Favorite => cx.emit(Event::Favorite(self.address().clone())),
            _ => return false,
        }
        true
    }

    pub(crate) fn move_cursor(
        &mut self,
        delta: i8,
        edge: bool,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        if self.rows.is_empty() {
            return;
        }
        let current = if let Some(index) = self.cursor
            && self
                .rows
                .get(index)
                .is_some_and(|entry| self.selected.contains(&entry.address))
        {
            Some(index)
        } else {
            let last = self.selected.last().cloned();
            last.and_then(|address| self.row_backing().position(&address))
        };
        let index = if edge {
            if delta < 0 { 0 } else { self.rows.len() - 1 }
        } else {
            current.map_or(0, |index| {
                if delta < 0 {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(self.rows.len() - 1)
                }
            })
        };
        if extend {
            let anchor = self.anchor.unwrap_or(index);
            self.anchor = Some(anchor);
            let backing = self.row_backing();
            self.selected
                .set_range(&backing, anchor.min(index), anchor.max(index));
        } else {
            self.anchor = Some(index);
            self.selected.set_single(self.rows[index].address.clone());
        }
        self.cursor = Some(index);
        self.table.update(cx, |state, cx| {
            state.scroll_to_row(index, cx);
        });
        self.sync_table(cx);
    }

    fn on_cancel_click(&mut self, _: &ClickEvent, _: &mut Window, _: &mut Context<Self>) {
        self.cancel_transfer();
    }

    fn render_job_notice(&self, job: &Job, cx: &mut Context<Self>) -> Div {
        match job {
            Job::Mutating { label, .. } => {
                el! { .div { flex; items_center; gap_2; w_full } [
                    .div { flex_1; min_w_0; truncate } [$(label.clone())],
                ] }
            }
            Job::Transferring { progress, .. } => {
                let status = format!(
                    "{} · {} · {}",
                    progress.files,
                    table::Files::format_size(progress.bytes),
                    progress.current
                );
                let on_cancel = cx.listener(Self::on_cancel_click);
                el! { .div { flex; items_center; gap_2; w_full } [
                    .div { flex_1; min_w_0; truncate } [status],
                    "remote-cancel".IconButton::new {
                        icon: IconName::X;
                        label: $(t!(@cx, "operation-cancel"));
                        on_click: on_cancel;
                    },
                ] }
            }
        }
    }
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gds::ui::page::*;
        let actions = self
            .toolbar_items()
            .into_iter()
            .map(|item| {
                let id = item.action.id();
                let label = item.action.label(cx);
                let on_click = cx.listener(Self::on_toolbar_action(item.action));
                el! { id.IconButton::new {
                    icon: $(item.icon);
                    label: label;
                    disabled: $(!item.enabled);
                    on_click: on_click;
                } }
                .into_any_element()
            })
            .collect::<Vec<_>>();
        let notice = self.message.clone();
        let job_notice = self.job.as_ref().map(|job| self.render_job_notice(job, cx));
        el! {
            .ContentPage::default [
                @if (!self.compact)
                .PageHeader::default [
                    .PageTitle::default [$(self.address().name())],
                    .PageActions::default { min_w_0; flex_shrink_1; } [
                        ..actions,
                    ],
                ],
                @end,
                @if (let Some(job_notice) = job_notice)
                    .PageNotice::default [job_notice],
                @end,
                @if (let Some(message) = notice)
                    .PageNotice::default [.div { whitespace_normal } [message]],
                @end,
                .PageContent::default [
                    .PageTableArea::default [
                        ($&self.table).DataTable::new {
                            bordered: false;
                            with_size: $(gpui_kit::component::Size::Size(px(gds::appearance::tokens::DataListStyle::ROW_HEIGHT)));
                        },
                    ],
                ],
            ]
        }
    }
}

impl View {
    pub(crate) fn set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
        if self.compact != compact {
            self.compact = compact;
            cx.notify();
        }
    }

    fn toolbar_items(&self) -> [ToolbarItem; 7] {
        let busy = self.busy();
        let selected = !self.selected.is_empty();
        [
            ToolbarItem {
                action: ToolbarAction::Upload,
                icon: IconName::Upload,
                enabled: !busy,
            },
            ToolbarItem {
                action: ToolbarAction::Download,
                icon: IconName::Download,
                enabled: !busy && selected,
            },
            ToolbarItem {
                action: ToolbarAction::NewFolder,
                icon: IconName::FolderPlus,
                enabled: !busy,
            },
            ToolbarItem {
                action: ToolbarAction::Rename,
                icon: IconName::Pencil,
                enabled: !busy && self.selected.len() == 1,
            },
            ToolbarItem {
                action: ToolbarAction::Delete,
                icon: IconName::Trash,
                enabled: !busy && selected,
            },
            ToolbarItem {
                action: ToolbarAction::Bookmark,
                icon: if self.options.favorite {
                    IconName::StarOff
                } else {
                    IconName::Star
                },
                enabled: !busy,
            },
            ToolbarItem {
                action: ToolbarAction::Disconnect,
                icon: IconName::Unplug,
                enabled: !busy,
            },
        ]
    }

    fn on_toolbar_action(
        action: ToolbarAction,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |this, _, window, cx| this.perform_toolbar_action(action, window, cx)
    }

    fn perform_toolbar_action(
        &mut self,
        action: ToolbarAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ToolbarAction::Upload => self.upload(window, cx),
            ToolbarAction::Download => self.download(self.selected.to_vec(), false, window, cx),
            ToolbarAction::NewFolder => self.name_prompt(true, window, cx),
            ToolbarAction::Rename => self.name_prompt(false, window, cx),
            ToolbarAction::Delete => self.confirm_delete(window, cx),
            ToolbarAction::Bookmark => cx.emit(Event::Favorite(self.address().clone())),
            ToolbarAction::Disconnect => cx.emit(Event::Disconnect(self.address().clone())),
        }
    }

    fn menu_click_handler(
        view: WeakEntity<Self>,
        action: ToolbarAction,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, _| {
            let view = view.clone();
            window.on_next_frame(move |window, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.options.focus.focus(window, cx);
                    this.perform_toolbar_action(action, window, cx);
                });
            });
        }
    }

    pub(crate) fn toolbar_menu(&self, view: WeakEntity<Self>, cx: &App) -> Vec<PopupMenuItem> {
        self.toolbar_items()
            .into_iter()
            .map(|item| {
                let label = item.action.label(cx);
                let on_click = Self::menu_click_handler(view.clone(), item.action);
                el! { label.PopupMenuItem::new {
                    icon: $(item.icon);
                    disabled: $(!item.enabled);
                    on_click: on_click;
                } }
            })
            .collect()
    }
}
