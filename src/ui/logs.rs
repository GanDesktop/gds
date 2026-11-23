//! Opt-in diagnostic viewer for application tracing logs and live task scopes.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use zgui::geom::{DevicePx, Point};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, Memo, RwSignal, Signal, StoredValue, on_cleanup_local, spawn_local,
};
use zgui::view::install_stylesheet;
use zgui::view::time::{TimeoutHandle, set_timeout};
use zgui::view::{ScrollBehavior, ScrollTarget};
use zgui::vocab::{Key, Modifiers, NamedKey};
use zgui::{component, view};
use zgui_ui::prelude::*;

pub use window::{OpenLogs, Options, init, is_enabled, open, store};

use crate::app::Gds;
use crate::i18n;
#[cfg(test)]
use crate::service::logs::Snapshot;
use crate::service::logs::{Entry, Level, Store};
use crate::ui::button::{ButtonSize, IconButtonProps};
use crate::ui::icon::{IconName, IconProps};
use crate::ui::task_manager::TaskManagerViewProps;
use crate::ui::tooltip::wrap_tokens;
use filters::{ALL_KEY, FilterState, level_to_key};
use navigation::NavigationStep;

mod filters;
mod navigation;
mod window;

#[cfg(test)]
mod tests;

const PREVIEW_CHARS: usize = 240;
const ROW_HEIGHT: f32 = 22.0;
const UPDATE_INTERVAL: Duration = Duration::from_millis(50);
const SHEET: &str = "gds-logs";

const LOGS_CSS: &str = "
.gds-logs-root {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    outline: none;
    background-color: var(--gds-content-bg, var(--gds-content));
    color: var(--gds-foreground);
}
.gds-logs-tabs {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
}
.gds-logs-tabs-header {
    display: flex;
    flex-direction: row;
    align-items: center;
    flex-shrink: 0;
    padding: 0 12px;
    border-bottom: 1px solid var(--gds-border);
    background: transparent;
}
.gds-logs-tabs-list.zui-tabs__list {
    height: 36px;
    padding: 0;
    gap: 16px;
    border: none;
    border-radius: 0;
    background: transparent;
}
.gds-logs-tabs-trigger.zui-tabs__trigger {
    height: 36px;
    padding: 0 4px;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: transparent;
    box-shadow: none;
    color: var(--gds-muted-foreground);
    font-size: var(--gds-text-sm, 14px);
    cursor: pointer;
}
.gds-logs-tabs-trigger.zui-tabs__trigger:hover {
    color: var(--gds-foreground);
}
.gds-logs-tabs-trigger.zui-tabs__trigger[data-state=\"active\"] {
    background: transparent;
    box-shadow: none;
    color: var(--gds-foreground);
    border-bottom-color: var(--gds-primary);
}
.gds-logs-panel {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
}
.gds-logs-toolbar {
    display: flex;
    flex-direction: row;
    align-items: center;
    flex-shrink: 0;
    gap: 8px;
    padding: 8px;
    background: transparent;
}
.gds-logs-select {
    width: 140px;
    flex-shrink: 0;
}
.gds-logs-select-target {
    width: 180px;
    flex-shrink: 0;
}
.gds-logs-search-wrap {
    flex: 1 1 0;
    min-width: 0;
}
.gds-logs-search-group.gds-logs-search-group {
    width: 100%;
    height: 32px;
    border-radius: 6px;
    background-color: var(--gds-content);
    border: 1px solid var(--gds-outline);
    box-shadow: none;
}
.gds-logs-search-group.gds-logs-search-group:focus-within {
    border-color: var(--gds-primary);
    box-shadow: none;
    outline: none;
}
.gds-logs-list-host {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    width: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
}
.gds-logs-list {
    flex: 1 1 0;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow-x: hidden;
}
.gds-logs-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 22px;
    padding: 0 8px;
    font-family: var(--gds-font-mono);
    font-size: var(--gds-text-xs, 12px);
    cursor: pointer;
    box-sizing: border-box;
    white-space: nowrap;
    overflow: hidden;
}
.gds-logs-row:hover {
    background-color: var(--gds-hover);
}
.gds-logs-row--selected,
.gds-logs-row--selected:hover {
    background-color: var(--gds-selection);
    color: var(--gds-selection-foreground);
}
.gds-logs-time {
    width: 100px;
    flex-shrink: 0;
    color: var(--gds-muted-foreground);
    white-space: nowrap;
}
.gds-logs-row--selected .gds-logs-time,
.gds-logs-row--selected .gds-logs-target {
    color: inherit;
}
.gds-logs-level {
    width: 44px;
    flex-shrink: 0;
    white-space: nowrap;
}
.gds-logs-level--default {
    color: var(--gds-foreground);
}
.gds-logs-row--selected .gds-logs-level--default {
    color: inherit;
}
.gds-logs-level--warn {
    color: var(--gds-warning, #d97706);
}
.gds-logs-level--error {
    color: var(--gds-error);
}
.gds-logs-target {
    width: 200px;
    flex-shrink: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--gds-muted-foreground);
}
.gds-logs-message {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.gds-logs-detail {
    max-height: 180px;
    flex-shrink: 0;
    border-top: 1px solid var(--gds-border);
    background: transparent;
    padding: 8px;
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
    min-width: 0;
}
.gds-logs-detail-body {
    flex: 1 1 0;
    min-width: 0;
    max-height: 164px;
    overflow-y: auto;
    font-family: var(--gds-font-mono);
    font-size: var(--gds-text-xs, 12px);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
}
.gds-logs-detail-actions {
    display: flex;
    flex-direction: row;
    align-items: center;
    flex-shrink: 0;
    gap: 4px;
}
.gds-logs-footer {
    padding: 4px 8px;
    background: transparent;
    color: var(--gds-muted-foreground);
    font-size: var(--gds-text-xs, 12px);
}
";

fn format_time(time: std::time::SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.format("%H:%M:%S%.3fZ").to_string()
}

fn preview_text(message: &str) -> String {
    let mut chars = message.chars();
    let mut preview: String = chars
        .by_ref()
        .take(PREVIEW_CHARS)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

fn level_class(level: Level) -> &'static str {
    match level {
        Level::Error => "gds-logs-level--error",
        Level::Warn => "gds-logs-level--warn",
        Level::Trace | Level::Debug | Level::Info => "gds-logs-level--default",
    }
}

#[must_use]
pub(crate) fn toggle_selected_id(current: Option<u64>, clicked: u64) -> Option<u64> {
    (current != Some(clicked)).then_some(clicked)
}

fn active_store() -> Option<Store> {
    Gds::try_current()
        .and_then(|gds| gds.ui().logs().store())
        .or_else(store)
}

#[cfg(test)]
mod test_probe {
    use super::*;
    use std::cell::RefCell;

    pub(crate) struct ViewerProbe {
        pub snapshot: RwSignal<Snapshot, LocalStorage>,
        pub visible: Memo<Vec<usize>>,
        pub selected: RwSignal<Option<u64>, LocalStorage>,
        pub level_key: RwSignal<String, LocalStorage>,
        pub target_key: RwSignal<String, LocalStorage>,
        pub query: RwSignal<String, LocalStorage>,
        pub active_tab: RwSignal<String, LocalStorage>,
        pub list_ref: NodeRef,
        pub root_ref: NodeRef,
    }

    thread_local! {
        pub(crate) static PROBE: RefCell<Option<ViewerProbe>> = const { RefCell::new(None) };
    }
}

/// Secondary diagnostic window root providing log filtering, inspection, and task monitoring.
#[component]
pub fn LogViewer() -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    install_stylesheet(SHEET, LOGS_CSS);

    let store_opt = active_store();
    let Some(active_store) = store_opt else {
        return AnyView::new(view! {
            box(class = "gds-logs-root", attr:data-testid = "logs-disabled") {
                text {{move || i18n::text("logs-open-error")}}
            }
        });
    };

    let has_inspector = Gds::try_current().is_some_and(|gds| gds.tasks().inspector().is_some());
    let active_tab = RwSignal::new_local(String::from("logs"));

    let changes = active_store.subscribe();
    let initial_snapshot = active_store.snapshot();
    let snapshot = RwSignal::new_local(initial_snapshot);
    let level_key = RwSignal::new_local(String::from(ALL_KEY));
    let target_key = RwSignal::new_local(String::from(ALL_KEY));
    let query = RwSignal::new_local(String::new());
    let selected = RwSignal::new_local(None::<u64>);

    let level_open = RwSignal::new_local(false);
    let target_open = RwSignal::new_local(false);

    let root_ref = NodeRef::new();
    let list_ref = NodeRef::new();
    let search_ref = NodeRef::new();
    let clipboard = try_use_clipboard();
    let pending_timer: Rc<std::cell::RefCell<Option<TimeoutHandle>>> =
        Rc::new(std::cell::RefCell::new(None));
    let initial_scroll_done = Rc::new(Cell::new(false));
    let scroll_to_tail_pending = Rc::new(Cell::new(true));

    let filter_state: Memo<FilterState> = Memo::new(move |_| {
        let lvl = level_key.with(|key| filters::key_to_level(key));
        let tgt = target_key.with(|key| (key != ALL_KEY).then(|| key.clone()));
        let q = query.get();
        FilterState {
            level: lvl,
            target: tgt,
            query: q,
        }
    });

    let visible: Memo<Vec<usize>> = Memo::new(move |_| {
        let filter = filter_state.get();
        snapshot.with(|snap| filter.visible_indices(&snap.entries))
    });

    let visible_count: Signal<usize, LocalStorage> =
        Signal::derive_local(move || visible.with(Vec::len));
    let targets: Memo<Vec<String>> =
        Memo::new(move |_| snapshot.with(|snap| filters::targets(&snap.entries)));

    let scroll_to_bottom = move |row_count: usize| {
        let scale = list_ref.scale().max(1.0);
        let pos = list_ref.scroll_position();
        let est_height = row_count as f32 * ROW_HEIGHT * scale;
        let content_h = pos.content_size.height.0.max(est_height);
        let target_y = (content_h - pos.scrollport.height.0).max(0.0);
        list_ref.scroll_to(
            ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(target_y))),
            ScrollBehavior::Instant,
        );
    };

    let refresh_from_store = Rc::new({
        let store = active_store.clone();
        let scroll_to_tail_pending = Rc::clone(&scroll_to_tail_pending);
        move || {
            let next = store.snapshot();
            let prev_rev = snapshot.with_untracked(|snap| snap.revision);
            if next.revision == prev_rev {
                return;
            }
            let pos = list_ref.scroll_position();
            let follow = pos.content_size.height.0 <= 0.0 || pos.is_at_end_vertically();
            snapshot.set(next);
            let still_present = selected.get_untracked().is_some_and(|sel_id| {
                let indices = visible.get_untracked();
                snapshot.with_untracked(|snap| {
                    indices
                        .iter()
                        .any(|&idx| snap.entries.get(idx).is_some_and(|e| e.id == sel_id))
                })
            });
            if !still_present && selected.get_untracked().is_some() {
                selected.set(None);
            }
            if follow {
                scroll_to_tail_pending.set(true);
                scroll_to_bottom(visible.get_untracked().len());
            }
        }
    });

    // Clear selection automatically whenever active filters exclude the selected entry.
    let prune_selected = zgui::reactive::RenderEffect::new(move |_| {
        visible.with(|indices| {
            let Some(sel_id) = selected.get_untracked() else {
                return;
            };
            let still_visible = snapshot.with_untracked(|snap| {
                indices
                    .iter()
                    .any(|&idx| snap.entries.get(idx).is_some_and(|e| e.id == sel_id))
            });
            if !still_visible {
                selected.set(None);
            }
        });
    });
    let _prune_selected_store = StoredValue::new_local(prune_selected);

    // Initial tail scroll once list viewport measures its content size.
    let content_size_sig = list_ref.observe_content_size();
    let tail_effect = zgui::reactive::RenderEffect::new(move |_| {
        let _size = content_size_sig.get();
        if !initial_scroll_done.get() || scroll_to_tail_pending.get() {
            initial_scroll_done.set(true);
            scroll_to_tail_pending.set(false);
            scroll_to_bottom(visible.get_untracked().len());
        }
    });
    let _tail_effect_store = StoredValue::new_local(tail_effect);

    // Coalesce rapid tracing log spikes at 50ms using the reactive UI timer.
    let timer_pending = Rc::new(Cell::new(false));
    let timer_slot = Rc::clone(&pending_timer);
    let timer_flag = Rc::clone(&timer_pending);
    on_cleanup_local(move || {
        *timer_slot.borrow_mut() = None;
    });

    spawn_local(async move {
        let mut drained_rx = changes;
        while drained_rx.recv().await.is_some() {
            if timer_flag.replace(true) {
                continue;
            }
            let timer_flag_inner = Rc::clone(&timer_flag);
            let refresh = Rc::clone(&refresh_from_store);
            let handle = set_timeout(UPDATE_INTERVAL, move || {
                timer_flag_inner.set(false);
                refresh();
            });
            *pending_timer.borrow_mut() = Some(handle);
        }
    });

    let search_placeholder = Signal::derive_local(|| i18n::text("logs-search"));
    let clear_search_label = Signal::derive_local(|| i18n::text("close"));
    let clear_label = Signal::derive_local(|| i18n::text("logs-clear"));
    let copy_label = Signal::derive_local(|| i18n::text("logs-copy-selected"));
    let close_detail_label = Signal::derive_local(|| i18n::text("logs-close-detail"));

    let on_clear = {
        let store = active_store.clone();
        move |_: &mut EventCx<'_, zgui::view::events::Click>| {
            store.clear();
            selected.set(None);
            snapshot.set(store.snapshot());
        }
    };

    let on_key_down = {
        move |ev: &mut EventCx<'_, zgui::view::events::KeyDown>| {
            if ev.modifiers == Modifiers::PRIMARY
                && matches!(&ev.key_without_modifiers, Key::Character(c) if c.eq_ignore_ascii_case("w"))
            {
                window::close_active();
                ev.stop_propagation();
                ev.prevent_default();
                return;
            }

            if active_tab.get_untracked() != "logs"
                || level_open.get_untracked()
                || target_open.get_untracked()
            {
                return;
            }

            let step = match ev.key {
                Key::Named(NamedKey::ArrowUp) if ev.modifiers == Modifiers::NONE => {
                    Some(NavigationStep::Previous)
                }
                Key::Named(NamedKey::ArrowDown) if ev.modifiers == Modifiers::NONE => {
                    Some(NavigationStep::Next)
                }
                Key::Named(NamedKey::PageUp) if ev.modifiers == Modifiers::NONE => {
                    Some(NavigationStep::PageUp)
                }
                Key::Named(NamedKey::PageDown) if ev.modifiers == Modifiers::NONE => {
                    Some(NavigationStep::PageDown)
                }
                _ => None,
            };

            let Some(step) = step else {
                return;
            };

            let pos = list_ref.scroll_position();
            let scale = list_ref.scale().max(1.0);
            let row_px = ROW_HEIGHT * scale;
            let page_rows = ((pos.scrollport.height.0 / row_px).floor() as usize).max(1);

            let chosen = visible.with_untracked(|indices| {
                snapshot.with_untracked(|snap| {
                    navigation::step_selection(
                        indices,
                        &snap.entries,
                        selected.get_untracked(),
                        step,
                        page_rows,
                    )
                })
            });

            if let Some((row_index, entry_id)) = chosen {
                selected.set(Some(entry_id));
                let row_top = row_index as f32 * row_px;
                let row_bottom = row_top + row_px;
                let offset_y = pos.offset.y.0;
                let viewport_h = pos.scrollport.height.0;
                if viewport_h > 0.0 {
                    if row_top < offset_y {
                        list_ref.scroll_to(
                            ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(row_top))),
                            ScrollBehavior::Instant,
                        );
                    } else if row_bottom > offset_y + viewport_h {
                        list_ref.scroll_to(
                            ScrollTarget::Offset(Point::new(
                                DevicePx(0.0),
                                DevicePx((row_bottom - viewport_h).max(0.0)),
                            )),
                            ScrollBehavior::Instant,
                        );
                    }
                }
                ev.stop_propagation();
                ev.prevent_default();
            }
        }
    };

    let selected_entry = Signal::derive_local(move || -> Option<Entry> {
        let sel_id = selected.get()?;
        let is_visible = visible.with(|indices| {
            snapshot.with(|snap| {
                indices
                    .iter()
                    .any(|&idx| snap.entries.get(idx).is_some_and(|e| e.id == sel_id))
            })
        });
        if !is_visible {
            return None;
        }
        snapshot.with(|snap| {
            snap.entries
                .iter()
                .find(|entry| entry.id == sel_id)
                .cloned()
        })
    });

    let target_options_view = move || -> Vec<AnyView> {
        let list = targets.get();
        let mut items = Vec::with_capacity(list.len() + 1);
        items.push(AnyView::new(view! {
            SelectItem(value = ALL_KEY) {{i18n::text("logs-all-targets")}}
        }));
        for target in list {
            let label = target.clone();
            items.push(AnyView::new(view! {
                SelectItem(value = target) {{label}}
            }));
        }
        items
    };

    let detail_panel = move || {
        let entry = selected_entry.get()?;
        let copy_text = entry.text();
        let wrapped = wrap_tokens(&copy_text);
        let clip = clipboard.clone();
        Some(view! {
            box(class = "gds-logs-detail", attr:data-testid = "logs-detail") {
                scroll(class = "gds-logs-detail-body", attr:data-testid = "logs-detail-text") {
                    text {{wrapped}}
                }
                box(class = "gds-logs-detail-actions") {
                    IconButton(
                        icon = IconName::Copy,
                        label = copy_label,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "logs-copy",
                        on:click = move |_| {
                            if let Some(clip) = clip.as_ref() {
                                clip.set_text(ClipboardKind::Standard, copy_text.clone());
                            }
                        },
                    )
                    IconButton(
                        icon = IconName::X,
                        label = close_detail_label,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "logs-close-detail",
                        on:click = move |_| {
                            selected.set(None);
                        },
                    )
                }
            }
        })
    };

    let build_logs_panel = || {
        view! {
            box(class = "gds-logs-panel") {
                box(class = "gds-logs-toolbar", attr:data-testid = "logs-toolbar") {
                    Select(
                        value = level_key,
                        open = level_open,
                    ) {
                        SelectTrigger(class = "gds-logs-select") {
                            SelectValue(placeholder = String::from(" "))
                        }
                        SelectContent {
                            SelectItem(value = level_to_key(None)) {{move || i18n::text("logs-all")}}
                            SelectItem(value = level_to_key(Some(Level::Trace))) {"TRACE"}
                            SelectItem(value = level_to_key(Some(Level::Debug))) {"DEBUG"}
                            SelectItem(value = level_to_key(Some(Level::Info))) {"INFO"}
                            SelectItem(value = level_to_key(Some(Level::Warn))) {"WARN"}
                            SelectItem(value = level_to_key(Some(Level::Error))) {"ERROR"}
                        }
                    }
                    Select(
                        value = target_key,
                        open = target_open,
                    ) {
                        SelectTrigger(class = "gds-logs-select-target") {
                            SelectValue(placeholder = String::from(" "))
                        }
                        SelectContent {
                            {target_options_view}
                        }
                    }
                    box(class = "gds-logs-search-wrap") {
                        InputGroup(class = "gds-logs-search-group") {
                            InputGroupAddon {
                                Icon(name = IconName::Search, size = 16.0)
                            }
                            InputGroupInput(
                                node_ref = search_ref,
                                value = Binding::from(query),
                                placeholder = search_placeholder.get_untracked(),
                                a11y:label = search_placeholder.get_untracked(),
                            ) {}
                            {move || {
                                if query.get().is_empty() {
                                    return None;
                                }
                                let s_ref = search_ref;
                                Some(
                                    view! {
                                        InputGroupAddon(align = InputGroupAddonAlign::InlineEnd) {
                                            InputGroupButton(
                                                size = InputGroupButtonSize::IconXs,
                                                a11y:label = clear_search_label.get_untracked(),
                                                on:click = move |_| {
                                                    query.set(String::new());
                                                    s_ref.focus();
                                                },
                                            ) {
                                                Icon(name = IconName::X, size = 14.0)
                                            }
                                        }
                                    }
                                    .into_view(),
                                )
                            }}
                        }
                    }
                    IconButton(
                        icon = IconName::ListX,
                        label = clear_label,
                        attr:data-testid = "logs-clear",
                        on:click = on_clear,
                    )
                }
                box(class = "gds-logs-list-host") {
                    VirtualList(
                        count = visible_count,
                        row_size = ROW_HEIGHT,
                        overscan = 6,
                        label = "Logs",
                        node_ref = list_ref,
                        class = "gds-logs-list",
                        row = move |index| {
                            let row_view = move || {
                                let entry = visible.with(|indices| {
                                    let entry_idx = *indices.get(index)?;
                                    snapshot.with(|snap| snap.entries.get(entry_idx).cloned())
                                })?;
                                let is_selected = selected.get() == Some(entry.id);
                                let id = entry.id;
                                let time_str = format_time(entry.time);
                                let level_str = entry.level.as_str().to_owned();
                                let lvl_cls = level_class(entry.level);
                                let tgt = entry.target.clone();
                                let preview = preview_text(&entry.message);
                                let r_ref = root_ref;
                                Some(view! {
                                    box(
                                        class = "gds-logs-row",
                                        class = if is_selected {
                                            "gds-logs-row--selected"
                                        } else {
                                            ""
                                        },
                                        attr:data-log-entry = {id.to_string()},
                                        on:click = move |_| {
                                            r_ref.focus();
                                            selected.update(|current| {
                                                *current = toggle_selected_id(*current, id);
                                            });
                                        },
                                    ) {
                                        text(class = "gds-logs-time") {{time_str}}
                                        text(class = "gds-logs-level", class = lvl_cls) {{level_str}}
                                        text(class = "gds-logs-target") {{tgt}}
                                        text(class = "gds-logs-message") {{preview}}
                                    }
                                })
                            };
                            view! { {row_view} }
                        },
                    )
                }
                {detail_panel}
                box(class = "gds-logs-footer", attr:data-testid = "logs-count") {
                    text {{move || {
                        let visible_n = visible_count.get();
                        let (retained, evicted) = snapshot.with(|snap| (snap.entries.len(), snap.evicted));
                        crate::t!(
                            "logs-count",
                            visible = visible_n,
                            retained = retained,
                            evicted = evicted
                        )
                    }}}
                }
            }
        }
    };

    let content = if has_inspector {
        let panel_holder = StoredValue::new_local(Some(AnyView::new(build_logs_panel())));
        AnyView::new(view! {
            Tabs(value = active_tab, class = "gds-logs-tabs") {
                box(class = "gds-logs-tabs-header") {
                    TabsList(class = "gds-logs-tabs-list") {
                        TabsTrigger(value = "logs", class = "gds-logs-tabs-trigger") {
                            {move || i18n::text("logs-tab-logs")}
                        }
                        TabsTrigger(value = "tasks", class = "gds-logs-tabs-trigger") {
                            {move || i18n::text("logs-tab-tasks")}
                        }
                    }
                }
                TabsContent(value = "logs", keep_mounted = true, class = "gds-logs-panel") {
                    {panel_holder.try_update_value(|slot| slot.take()).flatten()}
                }
                TabsContent(value = "tasks", class = "gds-logs-panel") {
                    TaskManagerView()
                }
            }
        })
    } else {
        AnyView::new(build_logs_panel())
    };

    #[cfg(test)]
    test_probe::PROBE.with(|slot| {
        *slot.borrow_mut() = Some(test_probe::ViewerProbe {
            snapshot,
            visible,
            selected,
            level_key,
            target_key,
            query,
            active_tab,
            list_ref,
            root_ref,
        });
    });

    AnyView::new(view! {
        box(
            node_ref = root_ref,
            tabindex = {Focus::Programmatic},
            class = "gds-logs-root",
            attr:data-testid = "logs-window",
            on:key_down = on_key_down,
        ) {
            {content}
        }
    })
}
