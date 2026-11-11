mod actions;
mod name_prompt;
mod table;
#[cfg(test)]
mod tests;

use crate::{
    features::{connect_server::Connector, file_operations::remote as operations},
    shared::ui::actions::Command,
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
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub(crate) struct Options {
    pub tasks: Scope,
    pub session: Session,
    pub listing: Listing,
    pub connector: Entity<Connector>,
    pub desktop: desktop::Desktop,
    pub favorite: bool,
    /// Retained for Wave 1 caller source-compatibility; filtering is driven via [`View::set_filter`].
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
    rows: Vec<Entry>,
    selected: Vec<Address>,
    anchor: Option<usize>,
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
            selected: Vec::new(),
            anchor: None,
            filter: String::new(),
            show_hidden,
            compact: false,
            job: None,
            message: None,
        };
        view.sync_table(cx);
        view
    }

    pub(super) fn filter_entries(entries: &[Entry], filter: &str, show_hidden: bool) -> Vec<Entry> {
        let needle = filter.to_lowercase();
        entries
            .iter()
            .filter(|entry| {
                (show_hidden || !entry.name.starts_with('.'))
                    && (needle.is_empty() || entry.name.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect()
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

    /// Source-compatible adapter for `pages/workspace` during Wave 1.
    pub(crate) fn background_task(&self) -> Option<(String, CancellationToken)> {
        self.running_task().map(|task| (task.label, task.cancel))
    }

    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn selected_addresses(&self) -> Vec<Address> {
        self.selected.clone()
    }

    pub(crate) fn deselect_address(&mut self, address: &Address, cx: &mut Context<Self>) {
        self.selected.retain(|selected| selected != address);
        self.sync_table(cx);
        cx.notify();
    }

    pub(crate) fn item_counts(&self) -> Counts {
        Counts {
            visible: self.rows.len(),
            selected: self.selected.len(),
        }
    }

    /// Source-compatible adapter for `pages/workspace` during Wave 1.
    pub(crate) fn counts(&self) -> (usize, usize) {
        let counts = self.item_counts();
        (counts.visible, counts.selected)
    }

    pub(crate) fn address(&self) -> &Address {
        &self.options.listing.address
    }

    pub(super) fn is_filtered(&self) -> bool {
        !self.filter.is_empty()
    }

    pub(crate) fn set_filter(&mut self, filter: String, show_hidden: bool, cx: &mut Context<Self>) {
        if self.filter == filter && self.show_hidden == show_hidden {
            return;
        }
        self.filter = filter;
        self.show_hidden = show_hidden;
        self.anchor = None;
        self.rows = Self::filter_entries(
            &self.options.listing.entries,
            &self.filter,
            self.show_hidden,
        );
        self.selected
            .retain(|address| self.rows.iter().any(|entry| entry.address == *address));
        self.sync_table(cx);
    }

    fn sync_table(&self, cx: &mut Context<Self>) {
        let rows = self.rows.clone();
        let selected = self.selected.clone();
        self.table.update(cx, |state, cx| {
            state.delegate_mut().rows = rows;
            state.delegate_mut().selected = selected;
            state.refresh(cx);
        });
        cx.notify();
    }

    pub(super) fn select(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.options.focus.focus(window, cx);
        let Some(entry) = self.rows.get(index).cloned() else {
            return;
        };
        if window.modifiers().shift {
            let anchor = self.anchor.unwrap_or(index);
            self.selected = self.rows[anchor.min(index)..=anchor.max(index)]
                .iter()
                .map(|entry| entry.address.clone())
                .collect();
        } else if window.modifiers().control || window.modifiers().platform {
            if self.selected.contains(&entry.address) {
                self.selected.retain(|address| address != &entry.address);
            } else {
                self.selected.push(entry.address.clone());
            }
            self.anchor = Some(index);
        } else {
            self.selected = vec![entry.address.clone()];
            self.anchor = Some(index);
        }
        self.sync_table(cx);
        if event.click_count() == 2 {
            self.activate(entry, window, cx);
        }
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
                self.selected = self
                    .rows
                    .iter()
                    .map(|entry| entry.address.clone())
                    .collect();
                self.sync_table(cx);
            }
            Command::DeselectAll => {
                self.selected.clear();
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
        let current = self
            .selected
            .last()
            .and_then(|address| self.rows.iter().position(|entry| entry.address == *address));
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
            self.selected = self.rows[anchor.min(index)..=anchor.max(index)]
                .iter()
                .map(|entry| entry.address.clone())
                .collect();
        } else {
            self.anchor = Some(index);
            self.selected = vec![self.rows[index].address.clone()];
        }
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
            ToolbarAction::Download => self.download(self.selected.clone(), false, window, cx),
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
