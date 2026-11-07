//! Editable file clipboard summary; removing a row updates the platform offer.
use super::*;
use super::{file_icon::FileIcon, file_label::FileLabel, status::*};
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{assets::IconName, prelude::*};

pub(super) struct Details {
    busy: bool,
    runtime: tokio::runtime::Handle,
    paths: Vec<PathBuf>,
    icons: std::collections::HashMap<PathBuf, FileIcon>,
    loading: Option<Task<()>>,
}

impl Details {
    pub(super) fn new(runtime: tokio::runtime::Handle) -> Self {
        Self {
            busy: false,
            runtime,
            paths: Vec::new(),
            icons: Default::default(),
            loading: None,
        }
    }

    fn sync_icons(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths == self.paths {
            return;
        }
        self.paths = paths.clone();
        self.icons.retain(|path, _| paths.contains(path));
        let work = self.runtime.spawn_blocking(move || {
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
            (paths, icons)
        });
        self.loading = Some(cx.spawn(async move |this, cx| match work.await {
            Ok((paths, icons)) => {
                let _ = this.update(cx, |this, cx| {
                    if this.paths == paths {
                        this.icons = icons;
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
        self.sync_icons(
            snapshot
                .as_ref()
                .map(|snapshot| snapshot.paths.clone())
                .unwrap_or_default(),
            cx,
        );
        let rows = snapshot
            .map(|snapshot| {
                snapshot.paths.iter().map(|path| {
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
                                cx.notify();
                            });
                        }).detach();
                        cx.notify();
                    });
                    el! {
                        .StatusRow::default {
                            # key; title: label;
                            content: .FileLabel::default { icon: icon; name: name; };
                            action: "remove-clipboard-entry".IconButton::new {
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
        el! {
            .StatusControl::default {
                # "file-clipboard-status"; title: $(i18n::text("clipboard-title", cx));
                summary: .StatusSummary::default {
                    compact: $(self.window.layout.compact); icon: icon; label: label; count: count;
                };
                details: $(self.clipboard_details.clone());
            }
        }
        .into_any_element()
    }
}
