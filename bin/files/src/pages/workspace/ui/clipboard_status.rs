//! Editable file clipboard summary; removing a row updates the platform offer.
use super::*;
use super::{file_label::FileLabel, status::*};
use crate::entities::file::ui::FileIcon;
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{assets::IconName, prelude::*};

struct IconBatch {
    paths: Vec<PathBuf>,
    icons: std::collections::HashMap<PathBuf, FileIcon>,
}

pub(super) struct Details {
    busy: bool,
    tasks: gds::service::task_manager::Scope,
    paths: Vec<PathBuf>,
    icons: std::collections::HashMap<PathBuf, FileIcon>,
    loading: Option<Task<()>>,
}

impl Details {
    pub(super) fn new(tasks: gds::service::task_manager::Scope) -> Self {
        Self {
            busy: false,
            tasks,
            paths: Vec::new(),
            icons: Default::default(),
            loading: None,
        }
    }

    pub(super) fn sync_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let paths = file_clipboard::current(cx)
            .as_ref()
            .map(|snapshot| snapshot.paths.clone())
            .unwrap_or_default();
        self.sync_icons(paths, cx);
    }

    fn sync_icons(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths == self.paths {
            return;
        }
        self.paths = paths.clone();
        self.icons.retain(|path, _| paths.contains(path));
        if paths.is_empty() {
            self.loading = None;
            cx.notify();
            return;
        }
        let work = self
            .tasks
            .spawn_blocking_named("Inspect clipboard files", move || {
                let icons = paths
                    .iter()
                    .map(|path| {
                        let icon = match directory::Entry::inspect(path) {
                            Ok(entry) => FileIcon::for_entry(&entry),
                            // Clipboard entries may disappear after a move or external deletion.
                            Err(_) => FileIcon::plain(IconName::FileQuestionMark),
                        };
                        (path.clone(), icon)
                    })
                    .collect();
                IconBatch { paths, icons }
            });
        self.loading = Some(cx.spawn(async move |this, cx| match work.await {
            Ok(batch) => {
                let _ = this.update(cx, |this, cx| {
                    if this.paths == batch.paths {
                        this.icons = batch.icons;
                        cx.notify();
                    }
                });
            }
            Err(error) => tracing::warn!(%error, "clipboard icon lookup failed"),
        }));
    }
}
impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = file_clipboard::current(cx);
        let rows = snapshot
            .map(|snapshot| {
                snapshot.paths.iter().enumerate().map(|(index, path)| {
                    let expected = snapshot.as_ref().clone();
                    let path = path.clone();
                    let label: SharedString = path.to_string_lossy().into_owned().into();
                    let key = label.clone();
                    let name: SharedString = path
                        .file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                        .into();
                    let icon = self.icons.get(&path).copied()
                        .unwrap_or_else(|| FileIcon::plain(IconName::File));
                    let on_click = cx.listener(move |this, _, _, cx| {
                        if this.busy {
                            return;
                        }
                        this.busy = true;
                        let work = file_clipboard::Bridge::new(cx)
                            .remove_path(expected.clone(), path.clone(), cx);
                        cx.spawn(async move |this, cx| {
                            let result = work.await;
                            let _ = this.update(cx, |this, cx| {
                                this.busy = false;
                                if let Err(error) = result {
                                    notifications::error(error.to_string(), cx);
                                }
                                file_clipboard::refresh(cx);
                                this.sync_from_clipboard(cx);
                                cx.notify();
                            });
                        }).detach();
                        cx.notify();
                    });
                    el! {
                        .StatusRow::default {
                            # key; title: label;
                            content: .FileLabel::default { icon: icon; name: name; };
                            action: ($("remove-clipboard-entry", index)).IconButton::new {
                                icon: IconName::X; label: $(i18n::text("clipboard-remove-item", cx));
                                disabled: $(self.busy); on_click: on_click;
                            };
                        }
                    }
                }).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        el! { .StatusList::default { empty: $(i18n::text("clipboard-empty", cx)); } [..rows] }
    }
}

impl Browser {
    pub(super) fn render_clipboard_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = file_clipboard::current(cx);
        let count = snapshot.as_ref().map_or(0, |snapshot| snapshot.paths.len());
        let icon = match snapshot.as_ref().map(|snapshot| snapshot.mode) {
            Some(file_clipboard::Mode::Cut) => IconName::Scissors,
            _ => IconName::Copy,
        };
        let label = snapshot
            .as_ref()
            .map(|snapshot| {
                if let [path] = snapshot.paths.as_slice() {
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                        .into()
                } else {
                    i18n::count("item-count", count, cx)
                }
            })
            .unwrap_or_default();
        let details = self.clipboard_details.clone();
        let on_open: std::rc::Rc<dyn Fn(&mut Window, &mut App)> = std::rc::Rc::new({
            let details = details.clone();
            move |_, cx| {
                details.update(cx, |details, cx| details.sync_from_clipboard(cx));
            }
        });
        el! {
            .StatusControl::default {
                # "file-clipboard-status"; title: $(i18n::text("clipboard-title", cx));
                summary: .StatusSummary::default {
                    compact: $(self.window.layout.compact); icon: icon; label: label; count: count;
                };
                details: details;
                on_open: on_open;
            }
        }
        .into_any_element()
    }
}
