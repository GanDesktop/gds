//! The current selection, with per-entry deselection.
use super::*;
use super::{file_icon::FileIcon, file_label::FileLabel, status::*};
use gds::service::remote::Address;
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{assets::IconName, prelude::*};

#[derive(Clone)]
enum Target {
    File(PathBuf),
    Trash(std::ffi::OsString),
    Remote(Address),
    Place,
    Volume,
}

pub(super) struct Details {
    browser: WeakEntity<Browser>,
    _changes: Subscription,
}
impl Details {
    pub(super) fn new(browser: Entity<Browser>, cx: &mut Context<Self>) -> Self {
        Self {
            browser: browser.downgrade(),
            _changes: cx.observe(&browser, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(browser) = self.browser.upgrade() else {
            return el! { .div }.into_any_element();
        };
        let tab = browser.read(cx).tabs.active_id;
        let entries = browser.read(cx).status_selection(cx);
        let icons: std::collections::HashMap<_, _> = browser
            .read(cx)
            .tabs
            .items
            .iter()
            .filter_map(|slot| slot.navigation.directory())
            .flat_map(|directory| &directory.entries)
            .map(|entry| (entry.path.clone(), FileIcon::for_entry(entry)))
            .collect();
        let rows = entries
            .into_iter()
            .enumerate()
            .map(|(index, (label, target))| {
                let icon = match &target {
                    Target::File(path) => icons
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
                        action: "deselect-entry".IconButton::new {
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
    fn status_selection(&self, cx: &App) -> Vec<(SharedString, Target)> {
        if !self.pages.is_empty() {
            return Vec::new();
        }
        if let Some(view) = self.remote_view() {
            return view
                .read(cx)
                .selected_addresses()
                .into_iter()
                .map(|address| (address.to_string().into(), Target::Remote(address)))
                .collect();
        }
        if self.collection() == Some(Collection::Trash) {
            return self
                .collections
                .trash
                .selected_entries()
                .into_iter()
                .map(|entry| {
                    (
                        entry.original_path.to_string_lossy().into_owned().into(),
                        Target::Trash(entry.id),
                    )
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
                .map(|volume| (volume.name.clone().into(), Target::Volume))
                .into_iter()
                .collect();
        }
        if let Some(place) = self.selected_place() {
            return vec![(place.label(cx).into(), Target::Place)];
        }
        self.selected_visible_paths()
            .into_iter()
            .map(|path| {
                (
                    path.to_string_lossy().into_owned().into(),
                    Target::File(path),
                )
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
        let count = self.status_selection(cx).len();
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
