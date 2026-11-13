//! Editable file clipboard summary; removing a row updates the platform offer.
use super::*;
use super::{file_label::FileLabel, status::*};
use crate::entities::file::ui::FileIcon;
use crate::shared::lib::clipboard;
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{assets::IconName, prelude::*};
use std::collections::{HashMap, HashSet};

struct IconBatch {
    expected: Arc<clipboard::Snapshot>,
    icons: HashMap<PathBuf, FileIcon>,
}

pub(in crate::pages::workspace) struct Details {
    open: bool,
    busy: bool,
    tasks: gds::service::task_manager::Scope,
    snapshot: Option<Arc<clipboard::Snapshot>>,
    icons: HashMap<PathBuf, FileIcon>,
    scroll: UniformListScrollHandle,
    loading: Option<Task<()>>,
    _changes: Subscription,
}

impl Details {
    pub(in crate::pages::workspace) fn new(
        tasks: gds::service::task_manager::Scope,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            open: false,
            busy: false,
            tasks,
            snapshot: None,
            icons: HashMap::new(),
            scroll: UniformListScrollHandle::new(),
            loading: None,
            _changes: clipboard::observe(cx, |this, cx| {
                if this.open {
                    this.sync_from_clipboard(cx);
                }
            }),
        }
    }

    pub(in crate::pages::workspace) fn is_open(&self) -> bool {
        self.open
    }

    pub(in crate::pages::workspace) fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.sync_from_clipboard(cx);
        }
    }

    pub(in crate::pages::workspace) fn sync_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.sync_state(cx) {
            cx.notify();
        }
    }

    fn sync_state(&mut self, cx: &mut Context<Self>) -> bool {
        let snapshot = cx.gds().clipboard().current();
        let unchanged = match (&self.snapshot, &snapshot) {
            (Some(previous), Some(next)) => Arc::ptr_eq(previous, next) || previous == next,
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return false;
        }
        let identity_changed = self.snapshot.as_ref().map(|current| &current.identity)
            != snapshot.as_ref().map(|next| &next.identity);
        if identity_changed {
            let count = snapshot.as_ref().map_or(0, |next| next.paths.len());
            let state = self.scroll.0.borrow();
            let row_height = state
                .last_item_size
                .map_or(LIST_HEIGHT, |size| f32::from(size.item.height));
            let viewport_height = f32::from(state.base_handle.bounds().size.height);
            let max_offset = (count as f32 * row_height - viewport_height).max(0.);
            if count == 0 || f32::from(-state.base_handle.offset().y) > max_offset {
                state.base_handle.set_offset(point(px(0.), px(0.)));
            }
        }
        self.snapshot = snapshot.clone();
        let Some(snapshot) = snapshot.filter(|snapshot| !snapshot.paths.is_empty()) else {
            self.icons.clear();
            self.loading = None;
            return true;
        };
        let active: HashSet<&Path> = snapshot.paths.iter().map(PathBuf::as_path).collect();
        self.icons.retain(|path, _| active.contains(path.as_path()));
        let missing: Vec<PathBuf> = snapshot
            .paths
            .iter()
            .filter(|path| !self.icons.contains_key(path.as_path()))
            .cloned()
            .collect();
        if missing.is_empty() {
            self.loading = None;
            return true;
        }
        let expected = Arc::clone(&snapshot);
        let work = self
            .tasks
            .spawn_blocking_named("Inspect clipboard files", move || {
                let icons = missing
                    .into_iter()
                    .map(|path| {
                        let icon = FileIcon::for_path(&path);
                        (path, icon)
                    })
                    .collect();
                IconBatch { expected, icons }
            });
        self.loading = Some(cx.spawn(async move |this, cx| match work.await {
            Ok(batch) => {
                let _ = this.update(cx, |this, cx| {
                    if this.snapshot.as_ref().is_some_and(|current| {
                        Arc::ptr_eq(current, &batch.expected) || current == &batch.expected
                    }) {
                        this.icons.extend(batch.icons);
                        cx.notify();
                    }
                });
            }
            Err(error) => tracing::warn!(%error, "clipboard icon lookup failed"),
        }));
        true
    }

    fn remove_entry(
        &mut self,
        expected: Arc<clipboard::Snapshot>,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        let work = cx.gds_mut().clipboard().remove_path(expected, path);
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if let Err(error) = result {
                    notifications::error(error.to_string(), cx);
                }
                cx.gds_mut().clipboard().refresh();
                this.sync_from_clipboard(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_row(
        &self,
        index: usize,
        expected: &Arc<clipboard::Snapshot>,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> StatusRow {
        let label: SharedString = path.to_string_lossy().into_owned().into();
        let key = label.clone();
        let name: SharedString = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
            .into();
        let icon = self
            .icons
            .get(path)
            .copied()
            .unwrap_or_else(|| FileIcon::plain(IconName::File));
        let expected = Arc::clone(expected);
        let path = path.to_path_buf();
        let on_click = cx.listener(move |this, _, _, cx| {
            this.remove_entry(Arc::clone(&expected), path.clone(), cx);
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
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(snapshot) = self
            .snapshot
            .as_ref()
            .filter(|snapshot| !snapshot.paths.is_empty())
            .cloned()
        else {
            return el! { .StatusList::default { empty: $(i18n::text("clipboard-empty", cx)); } }
                .into_any_element();
        };
        let list = uniform_list(
            "clipboard-details-list",
            snapshot.paths.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        let path = snapshot.paths.get(index)?;
                        Some(this.render_row(index, &snapshot, path, cx))
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
    pub(in crate::pages::workspace) fn render_clipboard_status(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = cx.gds().clipboard().current();
        let count = snapshot.as_ref().map_or(0, |snapshot| snapshot.paths.len());
        let icon = match snapshot.as_ref().map(|snapshot| snapshot.mode) {
            Some(clipboard::Mode::Cut) => IconName::Scissors,
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
        let on_open_change: std::rc::Rc<dyn Fn(bool, &mut Window, &mut App)> = std::rc::Rc::new({
            let details = details.clone();
            move |open, _, cx| {
                details.update(cx, |details, cx| details.set_open(open, cx));
            }
        });
        el! {
            .StatusControl::default {
                # "file-clipboard-status"; title: $(i18n::text("clipboard-title", cx));
                summary: .StatusSummary::default {
                    compact: $(self.window.layout.compact); icon: icon; label: label; count: count;
                };
                details: details;
                on_open_change: on_open_change;
            }
        }
        .into_any_element()
    }
}
