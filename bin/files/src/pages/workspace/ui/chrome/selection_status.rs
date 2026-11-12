//! The current selection, with per-entry deselection.
use super::*;
use super::{file_label::FileLabel, status::*};
use crate::entities::file::ui::FileIcon;
use gds::service::remote::Address;
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{assets::IconName, prelude::*};
use std::{collections::HashMap, rc::Rc};

#[derive(Clone, PartialEq, Eq)]
enum Target {
    File(PathBuf),
    Trash(std::ffi::OsString),
    Remote(Address),
    Place,
    Volume,
}

#[derive(Clone, PartialEq, Eq)]
struct SelectionItem {
    label: SharedString,
    target: Target,
}

pub(in crate::pages::workspace) struct Details {
    browser: WeakEntity<Browser>,
    open: bool,
    tab_id: u64,
    revision: u64,
    items: Vec<SelectionItem>,
    icons: HashMap<PathBuf, FileIcon>,
    scroll: UniformListScrollHandle,
    _changes: Subscription,
}
impl Details {
    pub(in crate::pages::workspace) fn new(
        browser: Entity<Browser>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            browser: browser.downgrade(),
            open: false,
            tab_id: 0,
            revision: 0,
            items: Vec::new(),
            icons: HashMap::new(),
            scroll: UniformListScrollHandle::new(),
            _changes: cx.observe(&browser, |this, browser, cx| {
                if browser.read(cx).clipboard_details.read(cx).is_open() {
                    let clipboard_details = browser.read(cx).clipboard_details.clone();
                    clipboard_details.update(cx, |details, cx| details.sync_from_clipboard(cx));
                }
                this.sync(&browser, cx);
            }),
        }
    }

    pub(in crate::pages::workspace) fn set_open(
        &mut self,
        open: bool,
        browser: &Entity<Browser>,
        cx: &mut Context<Self>,
    ) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.sync(browser, cx);
        }
    }

    fn sync_state(&mut self, browser: &Entity<Browser>, cx: &App) -> bool {
        if !self.open {
            return false;
        }
        let browser = browser.read(cx);
        let tab_id = browser.tabs.active_id;
        let revision = browser
            .active()
            .map_or(0, |slot| slot.navigation.revision());
        let items = browser.status_selection(cx);
        if self.tab_id == tab_id && self.revision == revision && self.items == items {
            return false;
        }
        if self.tab_id != tab_id {
            self.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), px(0.)));
        }
        self.tab_id = tab_id;
        self.revision = revision;
        self.items = items;
        self.icons.clear();

        if let Some(directory) = browser
            .active()
            .and_then(|slot| slot.navigation.directory())
        {
            for item in &self.items {
                if let Target::File(path) = &item.target
                    && let Some(entry) = directory.entry_for_path(path)
                {
                    self.icons.insert(path.clone(), FileIcon::for_entry(entry));
                }
            }
        }
        true
    }

    fn sync(&mut self, browser: &Entity<Browser>, cx: &mut Context<Self>) {
        if self.sync_state(browser, cx) {
            cx.notify();
        }
    }

    fn render_row(
        &self,
        index: usize,
        item: SelectionItem,
        tab: u64,
        browser: &Entity<Browser>,
        cx: &App,
    ) -> StatusRow {
        let SelectionItem { label, target } = item;
        let icon = match &target {
            Target::File(path) => self
                .icons
                .get(path)
                .copied()
                .unwrap_or_else(|| FileIcon::plain(IconName::FileQuestionMark)),
            Target::Place => FileIcon::plain(IconName::Folder),
            Target::Volume => FileIcon::plain(IconName::HardDrive),
            Target::Trash(id) => FileIcon::plain(
                match browser
                    .read(cx)
                    .collections
                    .trash
                    .rows()
                    .iter()
                    .find(|entry| &entry.id == id)
                    .map(|entry| entry.kind)
                {
                    Some(directory::EntryKind::Directory) => IconName::Folder,
                    Some(directory::EntryKind::SymbolicLink) => IconName::Link,
                    _ => IconName::File,
                },
            ),
            Target::Remote(_) => FileIcon::plain(IconName::File),
        };
        let name: SharedString = label
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(&label)
            .to_owned()
            .into();
        let browser = self.browser.clone();
        let on_click = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = browser.update(cx, |browser, cx| {
                if browser.tabs.active_id == tab {
                    browser.remove_status_selection(target.clone(), cx);
                }
            });
        };
        el! {
            .StatusRow::default {
                # format!("selected-{index}"); title: label;
                content: .FileLabel::default { icon: icon; name: name; };
                action: ($("deselect-entry", index)).IconButton::new {
                    icon: IconName::X; label: $(i18n::text("selection-remove-item", cx));
                    on_click: on_click;
                };
            }
        }
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(browser) = self.browser.upgrade() else {
            return el! { .div }.into_any_element();
        };
        if !self.open {
            self.open = true;
            self.sync_state(&browser, cx);
        }
        if self.items.is_empty() {
            return el! { .StatusList::default { empty: $(i18n::text("selection-empty", cx)); } }
                .into_any_element();
        }
        let list = uniform_list(
            "selection-details-list",
            self.items.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let Some(browser) = this.browser.upgrade() else {
                    return Vec::new();
                };
                let tab = browser.read(cx).tabs.active_id;
                range
                    .filter_map(|index| {
                        let item = this.items.get(index)?.clone();
                        Some(this.render_row(index, item, tab, &browser, cx))
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .w_full()
        .max_h(px(LIST_HEIGHT - LIST_PADDING * 2.));
        el! {
            .div {
                # "status-details"; w: LIST_WIDTH.px; max_w_full; max_h: LIST_HEIGHT.px;
                overflow_hidden; flex; flex_col; p_1; text_sm;
            } [list]
        }
        .into_any_element()
    }
}

impl Browser {
    fn status_selection_count(&self, cx: &App) -> usize {
        if !self.pages.is_empty() {
            return 0;
        }
        if let Some(view) = self.remote_view() {
            return view.read(cx).item_counts().selected;
        }
        if self.collection() == Some(Collection::Trash) {
            return self.collections.trash.selected_count();
        }
        if self.collection() == Some(Collection::Volumes) {
            return usize::from(
                self.selected_volume()
                    .and_then(|id| {
                        self.devices
                            .service
                            .read(cx)
                            .snapshot
                            .volumes
                            .iter()
                            .find(|volume| &volume.id == id)
                    })
                    .is_some(),
            );
        }
        if self.selected_place().is_some() {
            return 1;
        }
        let Some(slot) = self
            .active()
            .filter(|slot| slot.files_available() && slot.navigation.directory().is_some())
        else {
            return 0;
        };
        // `refresh_visible` prunes hidden or deleted paths from `selected` and interactive
        // selection only chooses from `table_rows`, while the active tab never holds an
        // unapplied projection.
        slot.navigation.view().selected.len()
    }

    fn status_selection(&self, cx: &App) -> Vec<SelectionItem> {
        if !self.pages.is_empty() {
            return Vec::new();
        }
        if let Some(view) = self.remote_view() {
            return view
                .read(cx)
                .selected_addresses()
                .into_iter()
                .map(|address| SelectionItem {
                    label: address.to_string().into(),
                    target: Target::Remote(address),
                })
                .collect();
        }
        if self.collection() == Some(Collection::Trash) {
            return self
                .collections
                .trash
                .selected_entries()
                .into_iter()
                .map(|entry| SelectionItem {
                    label: entry.original_path.to_string_lossy().into_owned().into(),
                    target: Target::Trash(entry.id),
                })
                .collect();
        }
        if self.collection() == Some(Collection::Volumes) {
            return self
                .selected_volume()
                .and_then(|id| {
                    self.devices
                        .service
                        .read(cx)
                        .snapshot
                        .volumes
                        .iter()
                        .find(|volume| &volume.id == id)
                })
                .map(|volume| SelectionItem {
                    label: volume.name.clone().into(),
                    target: Target::Volume,
                })
                .into_iter()
                .collect();
        }
        if let Some(place) = self.selected_place() {
            return vec![SelectionItem {
                label: place.label(cx).into(),
                target: Target::Place,
            }];
        }
        self.selected_visible_paths()
            .into_iter()
            .map(|path| SelectionItem {
                label: path.to_string_lossy().into_owned().into(),
                target: Target::File(path),
            })
            .collect()
    }

    fn remove_status_selection(&mut self, target: Target, cx: &mut Context<Self>) {
        match target {
            Target::File(path) => {
                if let Some(slot) = self.active_mut() {
                    slot.navigation.view_mut().selected.remove(&path);
                    if slot.anchor.as_ref() == Some(&path) {
                        slot.anchor = None;
                    }
                    if slot.cursor.as_ref() == Some(&path) {
                        slot.cursor = None;
                    }
                }
            }
            Target::Trash(id) => self.collections.trash.deselect(&id),
            Target::Remote(address) => {
                if let Some(view) = self.remote_view() {
                    view.update(cx, |view, cx| view.deselect_address(&address, cx));
                }
            }
            Target::Place => {
                if let Some(slot) = self.current_tab_mut() {
                    slot.navigation.view_mut().set_place(None);
                }
            }
            Target::Volume => self.set_selected_volume(None),
        }
        cx.notify();
    }

    pub(in crate::pages::workspace) fn render_selection_status(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = self.status_selection_count(cx);
        let label = if count == 0 {
            SharedString::default()
        } else {
            i18n::count("selection-count", count, cx)
        };
        let details = self.selection_details.clone();
        let browser = cx.entity();
        let on_open_change: Rc<dyn Fn(bool, &mut Window, &mut App)> = Rc::new({
            let details = details.clone();
            move |open, _, cx| {
                let browser = browser.clone();
                details.update(cx, |details, cx| details.set_open(open, &browser, cx));
            }
        });
        el! {
            .StatusControl::default {
                # "selected-files-status"; title: $(i18n::text("selection-title", cx));
                summary: .StatusSummary::default {
                    compact: $(self.window.layout.compact); icon: IconName::Check;
                    label: label; count: count;
                };
                details: details;
                on_open_change: on_open_change;
            }
        }
        .into_any_element()
    }
}
