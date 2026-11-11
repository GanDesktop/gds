//! The current selection, with per-entry deselection.
use super::*;
use super::{file_label::FileLabel, status::*};
use crate::entities::file::ui::FileIcon;
use gds::service::remote::Address;
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{assets::IconName, prelude::*};
use std::collections::{HashMap, HashSet};

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

pub(super) struct Details {
    browser: WeakEntity<Browser>,
    tab_id: u64,
    revision: u64,
    items: Vec<SelectionItem>,
    icons: HashMap<PathBuf, FileIcon>,
    _changes: Subscription,
}
impl Details {
    pub(super) fn new(browser: Entity<Browser>, cx: &mut Context<Self>) -> Self {
        Self {
            browser: browser.downgrade(),
            tab_id: 0,
            revision: 0,
            items: Vec::new(),
            icons: HashMap::new(),
            _changes: cx.observe(&browser, |this, browser, cx| {
                this.sync(&browser, cx);
            }),
        }
    }

    fn sync(&mut self, browser: &Entity<Browser>, cx: &mut Context<Self>) {
        let clipboard_details = browser.read(cx).clipboard_details.clone();
        clipboard_details.update(cx, |details, cx| details.sync_from_clipboard(cx));
        browser.update(cx, |browser, cx| browser.refresh_preview(cx));

        let (tab_id, revision, items) = {
            let browser = browser.read(cx);
            let tab_id = browser.tabs.active_id;
            let revision = browser
                .active()
                .map_or(0, |slot| slot.navigation.revision());
            let items = browser.status_selection(cx);
            (tab_id, revision, items)
        };
        if self.tab_id == tab_id && self.revision == revision && self.items == items {
            return;
        }
        self.tab_id = tab_id;
        self.revision = revision;
        self.items = items;
        self.icons.clear();

        let selected_files: HashSet<&Path> = self
            .items
            .iter()
            .filter_map(|item| match &item.target {
                Target::File(path) => Some(path.as_path()),
                _ => None,
            })
            .collect();
        if !selected_files.is_empty()
            && let Some(directory) = browser
                .read(cx)
                .active()
                .and_then(|slot| slot.navigation.directory())
        {
            for entry in &directory.entries {
                if selected_files.contains(entry.path.as_path()) {
                    self.icons
                        .insert(entry.path.clone(), FileIcon::for_entry(entry));
                }
            }
        }
        cx.notify();
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(browser) = self.browser.upgrade() else {
            return el! { .div }.into_any_element();
        };
        let tab = browser.read(cx).tabs.active_id;
        let rows = self.items.iter().cloned().enumerate().map(|(index, item)| {
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
        });
        el! { .StatusList::default { empty: $(i18n::text("selection-empty", cx)); } [..rows] }
            .into_any_element()
    }
}

impl Browser {
    fn status_selection_count(&self, cx: &App) -> usize {
        if !self.pages.is_empty() {
            return 0;
        }
        if let Some(view) = self.remote_view() {
            return view.read(cx).selected_addresses().len();
        }
        if self.collection() == Some(Collection::Trash) {
            return self.collections.trash.selected_entries().len();
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
        let Some(slot) = self.active() else {
            return 0;
        };
        let selected = &slot.navigation.view().selected;
        if selected.is_empty() {
            return 0;
        }
        slot.table_rows
            .iter()
            .filter(|entry| selected.contains(&entry.path))
            .count()
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
                    slot.navigation
                        .view_mut()
                        .selected
                        .retain(|selected| selected != &path);
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
                    slot.navigation.view_mut().place = None;
                }
            }
            Target::Volume => self.set_selected_volume(None),
        }
        cx.notify();
    }

    pub(super) fn render_selection_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.status_selection_count(cx);
        let label = if count == 0 {
            SharedString::default()
        } else {
            i18n::count("selection-count", count, cx)
        };
        el! {
            .StatusControl::default {
                # "selected-files-status"; title: $(i18n::text("selection-title", cx));
                summary: .StatusSummary::default {
                    compact: $(self.window.layout.compact); icon: IconName::Check;
                    label: label; count: count;
                };
                details: $(self.selection_details.clone());
            }
        }
        .into_any_element()
    }
}
