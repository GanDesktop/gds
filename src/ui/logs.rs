//! Optional diagnostics window, with bounded logs and the existing task inspector.
use crate::{
    app::Gds,
    i18n,
    service::logs::{Entry, Level, Snapshot, Store},
    ui::{Frame, Icon, IconButton, window::workspace_options},
};
use anti_webui::{
    prelude::*,
    widgets::{scroll_row_into_view, tabs_with},
};
use gds_uim::el;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
    time::Duration,
};

const ROW_HEIGHT: f32 = 22.0;
const DETAIL_HEIGHT: f32 = 180.0;
const PREVIEW_CHARS: usize = 240;

struct OpenWindow {
    window: WindowHandle,
    focus: Rc<Cell<Option<NodeRef>>>,
}

thread_local! {
    static OPEN: RefCell<Option<OpenWindow>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct Viewer {
    store: Store,
    snapshot: Signal<Snapshot>,
    query: Signal<String>,
    level: Signal<Option<Level>>,
    target: Signal<Option<String>>,
    selected: Signal<Option<u64>>,
    tab: Signal<usize>,
    search: NodeRef,
    list: NodeRef,
}

/// Opens or refocuses diagnostics when collection was enabled at launch.
pub fn open() -> bool {
    let Some(store) = Gds::current().ui().logs().store() else {
        return false;
    };
    OPEN.with_borrow_mut(|slot| {
        if let Some(existing) = slot.as_ref().filter(|entry| entry.window.is_open()) {
            if let Some(node) = existing.focus.get() {
                node.focus();
            }
            if existing.window.id() != current_window().id() {
                existing.window.request_activation().then(|result| {
                    if let Err(error) = result {
                        tracing::debug!(%error, "could not request diagnostics activation");
                    }
                });
            }
            return;
        }
        let focus = Rc::new(Cell::new(None));
        let initial = focus.clone();
        let window = open_window(
            workspace_options()
                .top_level()
                .title(i18n::text("logs-title"))
                .size(1000, 720)
                .min_size(780, 480),
            move || Viewer::new(store).window(initial),
        );
        *slot = Some(OpenWindow { window, focus });
    });
    true
}

impl Viewer {
    pub fn new(store: Store) -> Self {
        let mut updates = store.subscribe();
        let snapshot = signal(store.snapshot());
        let source = store.clone();
        let sender = ui_channel(move |()| snapshot.set(source.snapshot()));
        let scope = Gds::current().tasks().owner_scope("Logs window");
        scope
            .spawn_cancellable_named("Observe logs", async move {
                while updates.recv().await.is_some() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    while updates.try_recv().is_ok() {}
                    sender.send(());
                }
            })
            .on_complete(&scope, |result: Result<(), _>| {
                if let Err(error) = result {
                    tracing::debug!(%error, "log observer stopped");
                }
            });
        Self {
            store,
            snapshot,
            query: signal(String::new()),
            level: signal(None),
            target: signal(None),
            selected: signal(None),
            tab: signal(0),
            search: node_ref(),
            list: node_ref(),
        }
    }

    fn visible(&self) -> Vec<usize> {
        let query = self.query.get().to_lowercase();
        let level = self.level.get();
        let target = self.target.get();
        self.snapshot.with(|snapshot| {
            snapshot
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    level.is_none_or(|level| level == entry.level)
                        && target.as_ref().is_none_or(|target| target == &entry.target)
                        && (query.is_empty()
                            || entry.message.to_lowercase().contains(&query)
                            || entry.target.to_lowercase().contains(&query))
                })
                .map(|(index, _)| index)
                .collect()
        })
    }

    fn clear(&self) {
        self.store.clear();
        self.snapshot.set(self.store.snapshot());
        self.selected.set(None);
    }

    fn level_filter(&self) -> Div {
        let choices = [
            None,
            Some(Level::Trace),
            Some(Level::Debug),
            Some(Level::Info),
            Some(Level::Warn),
            Some(Level::Error),
        ]
        .into_iter()
        .map(|level| {
            (
                level,
                level.map_or_else(|| i18n::text("logs-all"), |level| level.as_str().into()),
            )
        })
        .collect();
        el! { (self->level, choices).select.{ w: 120.0_f32; h: 32.0_f32; flex_none;
            access: a => $(a.label(i18n::text("logs-level")));
        } }
    }

    fn target_options(&self) -> Vec<(Option<String>, String)> {
        let mut targets = self.snapshot.with(|snapshot| {
            snapshot
                .entries
                .iter()
                .map(|entry| entry.target.clone())
                .collect::<BTreeSet<_>>()
        });
        targets.extend(self.target.get());
        std::iter::once((None, i18n::text("logs-all-targets")))
            .chain(
                targets
                    .into_iter()
                    .map(|target| (Some(target.clone()), target)),
            )
            .collect()
    }

    fn detail(&self) -> Div {
        let selected = self.selected;
        let id = selected.get();
        let message = self.snapshot.with(|snapshot| {
            snapshot
                .entries
                .iter()
                .find(|entry| Some(entry.id) == id)
                .map(Entry::text)
        });
        let Some(message) = message else {
            return div();
        };
        let copy = message.clone();
        el! { .div.{
            flex; flex_none; min_w: 0.0_f32; gap: 8.0_f32; p: 8.0_f32; max_h: DETAIL_HEIGHT;
            border_t: 1.0_f32; dyn_style: s => $(s.border_color(theme().border));
        }.[
            .div.{flex_1; min_w: 0.0_f32; max_h: $(DETAIL_HEIGHT - 16.0); overflow_y_scroll}.[
                message.text.{font_family: "monospace"; text_size: 12.0_f32; select_text},
            ],
            "copy-log".IconButton::new.{
                icon: Icon::Copy; label_fn: () => "logs-copy-selected".i18n::text;
                on_click: move _ => $(set_clipboard(ClipboardKind::Clipboard, copy.clone()));
            },
            "close-log".IconButton::new.{
                icon: Icon::Close; label_fn: () => "logs-close-detail".i18n::text;
                on_click: move _ => selected.{set: None};
            },
        ] }
    }

    fn row(&self, entry: Entry) -> Div {
        let id = entry.id;
        let selected = self.selected;
        let list = self.list;
        let time: chrono::DateTime<chrono::Utc> = entry.time.into();
        let time = time.format("%H:%M:%S%.3fZ").to_string();
        let level = entry.level;
        let mut message = entry.message.chars();
        let mut preview: String = message
            .by_ref()
            .take(PREVIEW_CHARS)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        if message.next().is_some() {
            preview.push('…');
        }
        el! { .div.{
            h: ROW_HEIGHT; flex; items_center; gap: 8.0_f32; px: 8.0_f32; overflow_hidden;
            cursor: CursorIcon::Pointer; font_family: "monospace"; text_size: 12.0_f32; whitespace_nowrap;
            dyn_style: move s => $(if selected.get() == Some(id) {s.bg(theme().selected)} else {s});
            on_click: move _ => ${
                selected.set((selected.get_untracked() != Some(id)).then_some(id));
                list.focus();
            };
        }.[
            time.text.{w: 100.0_f32; flex_none; dyn_style: s => $(s.text_color(theme().text_muted))},
            $(level.as_str()).text.{w: 40.0_f32; flex_none;
                dyn_style: move s => $(s.text_color(match level {
                    Level::Error => theme().danger_text,
                    Level::Warn => theme().warning_text,
                    _ => theme().text,
                }));
            },
            entry->target.text.{w: 200.0_f32; flex_none; ellipsis; dyn_style: s => $(s.text_color(theme().text_muted))},
            preview.text.{flex_1; min_w: 0.0_f32; ellipsis},
        ] }
    }

    fn move_selection(&self, event: &KeyboardEvent, visible: Memo<Vec<usize>>) {
        let (forward, page) = match event.key {
            Key::Named(NamedKey::ArrowDown) => (true, false),
            Key::Named(NamedKey::ArrowUp) => (false, false),
            Key::Named(NamedKey::PageDown) => (true, true),
            Key::Named(NamedKey::PageUp) => (false, true),
            _ => return,
        };
        if event.modifiers != Modifiers::default() {
            return;
        }
        untrack(|| {
            let indices = visible.get();
            let Some(last) = indices.len().checked_sub(1) else {
                return;
            };
            let height = self
                .list
                .logical_rect()
                .map_or(ROW_HEIGHT, |rect| rect.height);
            let top = self.list.scroll_offset().y;
            let first_visible = ((top / ROW_HEIGHT).floor() as usize).min(last);
            let last_visible = (((top + height) / ROW_HEIGHT).ceil() as usize)
                .saturating_sub(1)
                .min(last);
            let selected = self.selected.get_untracked();
            let step = if page {
                (height / ROW_HEIGHT).floor().max(1.0) as usize
            } else {
                1
            };
            let (next, id) = self.snapshot.with_untracked(|snapshot| {
                let current = indices
                    .iter()
                    .position(|index| Some(snapshot.entries[*index].id) == selected);
                let next = if !page && current.is_none() {
                    if forward { first_visible } else { last_visible }
                } else {
                    let current = current.unwrap_or(first_visible);
                    if forward {
                        current.saturating_add(step).min(last)
                    } else {
                        current.saturating_sub(step)
                    }
                };
                (next, snapshot.entries[indices[next]].id)
            });
            self.selected.set(Some(id));
            let list = self.list;
            if page {
                let distance = step as f32 * ROW_HEIGHT * if forward { 1.0 } else { -1.0 };
                list.scroll_to(0.0, (top + distance).max(0.0));
            }
            request_frame(move |_| scroll_row_into_view(list, next, ROW_HEIGHT));
            event.prevent_default();
            event.stop_propagation();
        });
    }

    fn content(&self) -> Div {
        let viewer = self.clone();
        let visible = memo(move || viewer.visible());
        let rows = self.clone();
        let list_node = self.list;
        let selected = self.selected;
        let snapshot = self.snapshot;
        let count = Cell::new(None);
        effect(move || {
            let revision = snapshot.with(|snapshot| snapshot.revision);
            let visible = visible.get();
            let previous = count.replace(Some((revision, visible.len())));
            if let Some(id) = selected.get_untracked()
                && !snapshot.with_untracked(|snapshot| {
                    visible
                        .iter()
                        .any(|index| snapshot.entries[*index].id == id)
                })
            {
                selected.set(None);
            }
            if let Some((previous_revision, previous_count)) = previous {
                let previous_bottom = previous_count as f32 * ROW_HEIGHT;
                let height = list_node.logical_rect().map_or(0.0, |rect| rect.height);
                let bottom = list_node.scroll_offset().y + height;
                if previous_revision != revision && bottom + ROW_HEIGHT >= previous_bottom {
                    list_node.scroll_to_after_layout(0, i32::MAX);
                }
            }
        });
        request_frame(move |_| list_node.scroll_to_after_layout(0, i32::MAX));

        let list = virtual_list(
            move || visible.with(Vec::len),
            ROW_HEIGHT,
            move |index| {
                let viewer = rows.clone();
                dyn_view(move || {
                    visible
                        .with(|visible| visible.get(index).copied())
                        .and_then(|index| {
                            viewer
                                .snapshot
                                .with(|snapshot| snapshot.entries.get(index).cloned())
                        })
                        .map_or_else(div, |entry| viewer.row(entry))
                })
            },
        );
        let clear = self.clone();
        let detail = self.clone();
        let targets = self.clone();
        let target = self.target;
        let options = memo(move || targets.target_options());
        let levels = self.clone();
        let list_keys = self.clone();
        let search_keys = self.clone();
        let search = self.search;
        let query = self.query;
        el! { .div.{
            flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32;
        }.[
            .div.{flex; items_center; gap: 8.0_f32; p: 8.0_f32; flex_none}.[
                (move () => &levels.{level_filter}).dyn_view,
                (move () => ${
                    el! { (target, options.{get}).select.{w: 176.0_f32; h: 32.0_f32; flex_none;
                        access: a => $(a.label(i18n::text("logs-target")));
                    } }
                }).dyn_view,
                .div.{relative; flex_1; min_w: 0.0_f32; h: 32.0_f32}.[
                    query.text_field.{
                        node_ref: search; placeholder: "logs-search".i18n::text;
                        size_full; pr: 32.0_f32;
                        on_key_down: move event => $(search_keys.move_selection(event, visible));
                    },
                    (move () => $(!query.with(String::is_empty)), move () => ${
                        el! { .div.{absolute; right: 0.0_f32; top: 0.0_f32}.[
                            "clear-log-search".IconButton::new.{
                                icon: Icon::Close; label_fn: () => "logs-search-clear".i18n::text;
                                on_click: move _ => ${query.set(String::new()); search.focus();};
                            },
                        ] }
                    }).show,
                ],
                "logs-clear".IconButton::new.{
                    icon: Icon::Trash; label_fn: () => "logs-clear".i18n::text;
                    on_click: move _ => &clear.{clear};
                },
            ],
            list.{node_ref: list_node; focusable; flex_1; min_h: 0.0_f32;
                on_key_down: move event => $(list_keys.move_selection(event, visible));
            },
            (move () => &detail.{detail}).dyn_view,
            .div.{px: 8.0_f32; text_size: 12.0_f32; dyn_style: s => $(s.text_color(theme().text_muted))}.[
                (move () => ${
                    snapshot.with(|snapshot| i18n::t!("logs-count", visible=visible.with(Vec::len), retained=snapshot.entries.len(), evicted=snapshot.evicted))
                }).text,
            ],
        ] }
    }

    fn window(self, focus: Rc<Cell<Option<NodeRef>>>) -> View {
        let selected = self.tab;
        let search = self.search;
        let tasks_focus = node_ref();
        let tasks = Gds::current().tasks().inspector().map(|inspector| {
            crate::ui::task_manager::View::new(inspector, move || selected.get() == 1, tasks_focus)
        });
        let count = if tasks.is_some() { 2 } else { 1 };
        let tab_row = tabs_with(selected, count, |tabs| {
            let heads = ["logs-tab-logs", "logs-tab-tasks"].into_iter().take(count).enumerate().map(|(index, label)| {
                let head = el! { .div.{
                    flex_none; px: 12.0_f32; py: 6.0_f32; border_b: 2.0_f32; select_none; cursor: CursorIcon::Pointer;
                    dyn_style: move s => $(if tabs.is_selected(index) {
                        s.text_color(theme().text).border_color(theme().accent)
                    } else {
                        s.text_color(theme().text_muted).border_color(Color::TRANSPARENT)
                    });
                }.[(move () => label.i18n::text).text] };
                tabs.tab(index, head)
            });
            el! { .div.{flex; flex_none; px: 12.0_f32; border_b: 1.0_f32; dyn_style: s => $(s.border_color(theme().border))}.[..heads] }
        });
        effect(move || {
            let node = if selected.get() == 0 {
                search
            } else {
                tasks_focus
            };
            focus.set(Some(node));
            request_frame(move |_| node.focus());
        });
        let content = self.content();
        let content = el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32;
            on_key_down: event => ${
                if event.modifiers.primary() && !event.modifiers.alt
                    && !event.modifiers.shift && event.key.is_char('w')
                {
                    event.prevent_default();
                    event.stop_propagation();
                    request_close_window();
                }
            };
        }.[
            tab_row,
            .div.{flex; flex_col; flex_1; min_h: 0.0_f32;
                dyn_style: move s => $(if selected.get() == 0 {s} else {s.hidden()});
            }.[content],
            if let Some(tasks) = tasks [
                .div.{flex; flex_col; flex_1; min_h: 0.0_f32;
                    dyn_style: move s => $(if selected.get() == 1 {s} else {s.hidden()});
                }.[tasks],
            ],
        ] };
        let frame = Frame::without_sidebar();
        let toolbar = frame.toolbar(div(), text(|| i18n::text("logs-title")));
        let workspace = frame.workspace(div(), content);
        el! { frame.[toolbar, workspace] }.into_view()
    }
}

impl IntoView for Viewer {
    fn into_view(self) -> View {
        self.window(Rc::new(Cell::new(None)))
    }
}
