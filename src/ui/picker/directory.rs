//! A source-independent directory chooser built on Kit's data table and inputs.
mod commands;
mod model;
mod table;
mod view;
pub use model::{Entry, Location, Page, Request};
use model::{History, Navigation};

use gpui_kit::{
    component::{
        input::{InputEvent, InputState},
        table::{TableEvent, TableState},
    },
    *,
};
use std::{future::Future, pin::Pin, rc::Rc};

/// The returned future must release its in-flight work when dropped.
pub type Loader = Rc<dyn Fn(Request) -> Pin<Box<dyn Future<Output = Result<Page, String>>>>>;

pub struct DirectoryPicker {
    loader: Loader,
    page: Option<Page>,
    table: Entity<TableState<table::Directories>>,
    address: Entity<InputState>,
    filter: Entity<InputState>,
    history: History,
    hidden: bool,
    editing: bool,
    pending: Option<(Request, Navigation)>,
    failed: Option<(Request, Navigation, String)>,
    generation: u64,
    task: Option<Task<()>>,
}

impl DirectoryPicker {
    pub fn new(
        loader: Loader,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        commands::register(cx);
        let table = table::Directories::state(cx.entity().downgrade(), window, cx);
        let address = cx.new(|cx| InputState::new(window, cx));
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::text("picker-filter", cx))
        });
        cx.subscribe_in(&address, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let path = this.address.read(cx).value().to_string();
                if !path.is_empty() {
                    this.navigate(Some(path), Navigation::Visit, window, cx);
                }
            }
        })
        .detach();
        cx.subscribe_in(&filter, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_table(true, cx);
                this.load_more(window, cx);
            }
        })
        .detach();
        cx.subscribe_in(&table, window, |this, _, event, window, cx| match event {
            TableEvent::DoubleClickedRow(_) => this.open_selected(window, cx),
            TableEvent::SelectRow(index) => {
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().selected = Some(*index);
                    cx.notify();
                });
                cx.notify();
            }
            TableEvent::ClearSelection => {
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().selected = None;
                    cx.notify();
                });
                cx.notify();
            }
            _ => {}
        })
        .detach();
        let mut picker = Self {
            loader,
            page: None,
            table,
            address,
            filter,
            history: History::default(),
            hidden: false,
            editing: false,
            pending: None,
            failed: None,
            generation: 0,
            task: None,
        };
        picker.navigate(path, Navigation::Visit, window, cx);
        picker
    }

    /// The selected child, or the current directory when nothing is selected.
    /// Pending navigation and failed reads cannot be accepted.
    pub fn selected_path(&self, cx: &App) -> Option<String> {
        if self.pending.is_some() || self.failed.is_some() || self.editing {
            return None;
        }
        let page = self.page.as_ref()?;
        Some(self.selected_entry(cx).map_or_else(
            || page.location.path.clone(),
            |entry| entry.location.path.clone(),
        ))
    }

    pub fn cancel_loading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self.task = None;
        self.pending = None;
        self.failed = None;
        self.editing = false;
        if let Some(page) = &self.page {
            self.address.update(cx, |input, cx| {
                input.set_value(page.location.path.clone(), window, cx)
            });
        }
        cx.notify();
    }

    fn selected_entry(&self, cx: &App) -> Option<Entry> {
        let table = self.table.read(cx);
        table
            .selected_row()
            .and_then(|index| table.delegate().rows.get(index))
            .cloned()
    }

    fn navigate(
        &mut self,
        path: Option<String>,
        kind: Navigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load(Request { path, cursor: None }, kind, window, cx);
    }

    fn load(
        &mut self,
        request: Request,
        kind: Navigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.generation += 1;
        let generation = self.generation;
        self.task = None;
        self.failed = None;
        self.pending = Some((request.clone(), kind));
        let load = (self.loader)(request.clone());
        self.task = Some(cx.spawn_in(window, async move |picker, cx| {
            let result = load.await;
            let _ = picker.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.pending = None;
                match result {
                    Ok(page) => this.apply(page, kind, window, cx),
                    Err(error) => this.failed = Some((request, kind, error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn apply(
        &mut self,
        mut page: Page,
        kind: Navigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if kind == Navigation::More {
            if let Some(previous) = &mut self.page {
                if previous.location.path != page.location.path {
                    return;
                }
                let known: std::collections::HashSet<_> = previous
                    .entries
                    .iter()
                    .map(|e| e.location.path.clone())
                    .collect();
                previous.entries.extend(
                    page.entries
                        .drain(..)
                        .filter(|entry| !known.contains(&entry.location.path)),
                );
                previous.next_cursor = page.next_cursor;
            }
        } else {
            self.history.commit(
                kind,
                self.page.as_ref().map(|page| page.location.path.as_str()),
                &page.location.path,
            );
            self.address.update(cx, |input, cx| {
                input.set_value(page.location.path.clone(), window, cx)
            });
            self.page = Some(page);
            self.editing = false;
        }
        self.sync_table(kind != Navigation::More, cx);
        if kind != Navigation::More {
            self.table.focus_handle(cx).focus(window, cx);
        }
        // A filtered or hidden-only page must not prevent discovering later pages.
        if self.table.read(cx).delegate().rows.len() < 20
            || !self.filter.read(cx).value().is_empty()
        {
            cx.on_next_frame(window, |this, window, cx| this.load_more(window, cx));
        }
    }

    fn sync_table(&self, clear: bool, cx: &mut Context<Self>) {
        let query = self.filter.read(cx).value().to_lowercase();
        let rows: Vec<_> = self
            .page
            .iter()
            .flat_map(|page| &page.entries)
            .filter(|entry| {
                (self.hidden || !entry.hidden)
                    && entry.location.label.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        self.table.update(cx, |table, cx| {
            table.delegate_mut().rows = rows;
            if clear {
                table.clear_selection(cx);
                table.scroll_to_row(0, cx);
            }
            table.refresh(cx);
        });
        cx.notify();
    }

    fn load_more(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.failed.is_some() {
            return;
        }
        if let Some(page) = &self.page
            && let Some(cursor) = &page.next_cursor
        {
            self.load(
                Request {
                    path: Some(page.location.path.clone()),
                    cursor: Some(cursor.clone()),
                },
                Navigation::More,
                window,
                cx,
            );
        }
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.selected_entry(cx) {
            self.navigate(
                Some(entry.location.path.clone()),
                Navigation::Visit,
                window,
                cx,
            );
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.history.back.last() {
            self.navigate(Some(path.clone()), Navigation::Back, window, cx);
        }
    }
    fn forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.history.forward.last() {
            self.navigate(Some(path.clone()), Navigation::Forward, window, cx);
        }
    }
    fn parent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.page.as_ref().and_then(|page| page.parent.clone()) {
            self.navigate(Some(path), Navigation::Visit, window, cx);
        }
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((request, kind, _)) = self.failed.clone() {
            self.load(request, kind, window, cx);
        } else {
            self.navigate(
                self.page.as_ref().map(|page| page.location.path.clone()),
                Navigation::Refresh,
                window,
                cx,
            );
        }
    }
    fn edit_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = true;
        self.address.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }
    fn toggle_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hidden = !self.hidden;
        self.sync_table(true, cx);
        self.load_more(window, cx);
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
