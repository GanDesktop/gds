mod actions;
mod name_prompt;
mod table;

use crate::{
    features::{
        connect_server::Connector,
        file_operations::{commands::Command, remote as operations},
    },
    shared::i18n,
};
use gds::ui::IconButton;
use gds::ui::field::input as input_field;
use gds::{
    service::{
        desktop,
        remote::{Address, Entry, EntryKind, Listing, Session},
    },
    ui::notifications,
};
use gds_uim::el;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        table::{DataTable, TableState},
    },
    *,
};
use std::path::PathBuf;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

pub(crate) struct Options {
    pub runtime: Handle,
    pub session: Session,
    pub listing: Listing,
    pub connector: Entity<Connector>,
    pub desktop: desktop::Desktop,
    pub favorite: bool,
    pub filter: Entity<gpui_kit::component::input::InputState>,
    pub focus: FocusHandle,
}

pub(crate) enum Event {
    Navigate(Address),
    Refresh,
    Favorite(Address),
    Disconnect(Address),
}

pub(crate) struct View {
    options: Options,
    table: Entity<TableState<table::Files>>,
    rows: Vec<Entry>,
    selected: Vec<Address>,
    anchor: Option<usize>,
    filter: String,
    hidden: bool,
    compact: bool,
    task: Option<Task<()>>,
    progress_task: Option<Task<()>>,
    cancel: CancellationToken,
    busy: bool,
    cancellable: bool,
    progress: operations::Progress,
    message: Option<String>,
}

impl EventEmitter<Event> for View {}
impl Drop for View {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl View {
    pub(crate) fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = table::Files::state(cx.entity().downgrade(), window, cx);
        let view = Self {
            rows: options.listing.entries.clone(),
            options,
            table,
            selected: Vec::new(),
            anchor: None,
            filter: String::new(),
            hidden: true,
            compact: false,
            task: None,
            progress_task: None,
            cancel: gds::ui::task_manager::scope(cx).token().child_token(),
            busy: false,
            cancellable: false,
            progress: Default::default(),
            message: None,
        };
        view.sync_table(cx);
        view
    }
    pub(crate) fn background_task(&self) -> Option<(String, CancellationToken)> {
        (self.busy && self.cancellable).then(|| {
            (
                format!(
                    "{} — {}",
                    self.options.listing.address, self.progress.current
                ),
                self.cancel.clone(),
            )
        })
    }
    pub(crate) fn busy(&self) -> bool {
        self.busy
    }
    pub(crate) fn selected_addresses(&self) -> Vec<Address> {
        self.selected.clone()
    }

    pub(crate) fn deselect_address(&mut self, address: &Address, cx: &mut Context<Self>) {
        self.selected.retain(|selected| selected != address);
        self.sync_table(cx);
        cx.notify();
    }

    pub(crate) fn counts(&self) -> (usize, usize) {
        (self.rows.len(), self.selected.len())
    }
    pub(crate) fn address(&self) -> &Address {
        &self.options.listing.address
    }

    pub(crate) fn set_filter(&mut self, filter: String, hidden: bool, cx: &mut Context<Self>) {
        if self.filter == filter && self.hidden == hidden {
            return;
        }
        self.filter = filter;
        self.hidden = hidden;
        self.anchor = None;
        self.rows = self
            .options
            .listing
            .entries
            .iter()
            .filter(|e| {
                (self.hidden || !e.name.starts_with('.'))
                    && e.name.to_lowercase().contains(&self.filter.to_lowercase())
            })
            .cloned()
            .collect();
        self.selected
            .retain(|a| self.rows.iter().any(|e| e.address == *a));
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
    fn select(
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
                .map(|e| e.address.clone())
                .collect();
        } else if window.modifiers().control || window.modifiers().platform {
            if self.selected.contains(&entry.address) {
                self.selected.retain(|a| a != &entry.address);
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
        if self.busy {
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
            .filter(|e| self.selected.contains(&e.address))
            .map(|e| operations::Resource::Remote {
                address: e.address.clone(),
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

    pub(crate) fn command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.busy {
            if command == Command::Cancel && self.cancellable {
                self.cancel.cancel();
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
                    .find(|e| self.selected.contains(&e.address))
                    .cloned()
                {
                    self.activate(entry, window, cx);
                }
            }
            Command::SelectAll => {
                self.selected = self.rows.iter().map(|e| e.address.clone()).collect();
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
                self.message = Some(i18n::text("remote-cut-unavailable", cx).into());
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
            .and_then(|a| self.rows.iter().position(|e| e.address == *a));
        let index = if edge {
            if delta < 0 { 0 } else { self.rows.len() - 1 }
        } else {
            current.map_or(0, |i| {
                if delta < 0 {
                    i.saturating_sub(1)
                } else {
                    (i + 1).min(self.rows.len() - 1)
                }
            })
        };
        if extend {
            let anchor = self.anchor.unwrap_or(index);
            self.selected = self.rows[anchor.min(index)..=anchor.max(index)]
                .iter()
                .map(|e| e.address.clone())
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
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gds::ui::page::*;
        use gpui_kit::assets::IconName;
        let actions = self
            .toolbar_actions()
            .into_iter()
            .map(|(id, icon, enabled)| {
                el! { id.IconButton::new {
                    icon: icon;
                    label: $(i18n::text(id, cx));
                    disabled: $(!enabled);
                    on_click: $(cx.listener(move |this, _, window, cx| this.toolbar_action(id, window, cx)));
                } }
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let notice = self.message.clone();
        let status = format!(
            "{} · {} B · {}",
            self.progress.files, self.progress.bytes, self.progress.current
        );
        el! {
            .ContentPage::default [
                @if (!self.compact)
                .PageHeader::default [
                    .PageTitle::default [$(self.address().name())],
                    .PageActions::default { min_w_0; flex_shrink_1; } [
                        .div { w: 180.0.px; min_w_0 } [($&self.options.filter, cx).input_field { small; cleanable: true }],
                        ..actions,
                    ],
                ],
                @end,
                @if (self.busy)
                    .PageNotice::default [
                        .div { flex; items_center; gap_2; w_full } [
                            .div { flex_1; min_w_0; truncate } [status],
                            @if (self.cancellable)
                            "remote-cancel".IconButton::new { icon: IconName::X; label: $i18n::text("operation-cancel", cx);
                                on_click: $(cx.listener(|this, _, _, _| this.cancel.cancel()));
                            },
                            @end,
                        ],
                    ],
                @end,
                @if (let Some(message) = notice)
                    .PageNotice::default [.div { whitespace_normal } [message]],
                @end,
                .PageContent::default [
                    .PageTableArea::default [
                        ($&self.table).DataTable::new { bordered: false; with_size: $(gpui_kit::component::Size::Size(px(gds::appearance::tokens::DataListStyle::ROW_HEIGHT))); },
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

    fn toolbar_actions(&self) -> [(&'static str, gpui_kit::assets::IconName, bool); 7] {
        use gpui_kit::assets::IconName;
        let selected = !self.selected.is_empty();
        [
            ("remote-upload", IconName::Upload, !self.busy),
            (
                "remote-download",
                IconName::Download,
                !self.busy && selected,
            ),
            ("new-folder", IconName::FolderPlus, !self.busy),
            (
                "rename",
                IconName::Pencil,
                !self.busy && self.selected.len() == 1,
            ),
            (
                "delete-permanently",
                IconName::Trash,
                !self.busy && selected,
            ),
            (
                "remote-bookmark",
                if self.options.favorite {
                    IconName::StarOff
                } else {
                    IconName::Star
                },
                !self.busy,
            ),
            ("remote-disconnect", IconName::Unplug, !self.busy),
        ]
    }

    fn toolbar_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "remote-upload" => self.upload(window, cx),
            "remote-download" => self.download(self.selected.clone(), false, window, cx),
            "new-folder" => self.name_prompt(true, window, cx),
            "rename" => self.name_prompt(false, window, cx),
            "delete-permanently" => self.confirm_delete(window, cx),
            "remote-bookmark" => cx.emit(Event::Favorite(self.address().clone())),
            _ => cx.emit(Event::Disconnect(self.address().clone())),
        };
    }

    pub(crate) fn toolbar_menu(&self, view: WeakEntity<Self>, cx: &App) -> Vec<PopupMenuItem> {
        self.toolbar_actions()
            .into_iter()
            .map(|(id, icon, enabled)| {
                let view = view.clone();
                el! { $(i18n::text(id, cx)).PopupMenuItem::new {
                    icon: icon;
                    disabled: $(!enabled);
                    on_click: $(move |_, window, _| {
                        let view = view.clone();
                        window.on_next_frame(move |window, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.options.focus.focus(window, cx);
                                this.toolbar_action(id, window, cx);
                            });
                        });
                    });
                } }
            })
            .collect()
    }
}
