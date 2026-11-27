//! The footer's conditional task, selection, and editable clipboard summary popovers.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gds::appearance::motion::{WORKSPACE_DURATION_MS, use_transition};
use gds::appearance::tokens::ICON_SIZE;
use gds::i18n;
use gds::ui::button::{ButtonSize, IconButtonProps};
use gds::ui::feedback::tooltip::{HintTooltipProps, wrap_tokens};
use gds::ui::{IconName, IconProps};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, on_cleanup_local};
use zgui::task::spawn_local;
use zgui::view::{Scoped, current_host, install_stylesheet};
use zgui::vocab::{Role, SharedString};
use zgui::{css, view};
use zgui_ui::overlay::OverlayState;
use zgui_ui::{PopoverContentProps, VirtualListProps};
use zgui_ui_primitives::{Align, Binding, Placement, Side};

pub(in crate::pages::workspace) use super::super::state::ui_flags::StatusKind as Kind;
use super::super::{
    projection::Geometry,
    state::{
        context::{Context, local_memo},
        menus::Surface,
        model::{Pane, Row},
    },
};
use crate::entities::file::{directory, ui::FileIcon};
use crate::shared::lib::clipboard::{Clipboard, Mode, Snapshot};

const CONTROL_SIZE: f32 = 24.0;
const TRIGGER_ICON_SIZE: f32 = 14.0;
const LABEL_MAX_WIDTH: f32 = 240.0;
const LABEL_TRAILING_GAP: f32 = 8.0;
const POPOVER_ROW_HEIGHT: f32 = 32.0;
const POPOVER_MAX_LIST_HEIGHT: f32 = 312.0;
const ARTWORK_BADGE_RATIO: f32 = 0.55;

const STATUS_SHEET: &str = "gds-files-status";

const STATUS_CSS: &str = css!(
    r#"
    .gds-files-status-controls.gds-files-status-controls {
        display: flex;
        align-items: center;
        flex-shrink: 0;
        min-width: 0px;
    }

    .gds-files-status-slot.gds-files-status-slot {
        display: flex;
        align-items: center;
        min-width: 0px;
        flex-shrink: 0;
    }

    .gds-files-status-button.gds-files-status-button {
        display: inline-flex;
        align-items: center;
        min-width: 24px;
        height: 24px;
        min-height: 24px;
        padding: 0px;
        border: none;
        border-radius: var(--gds-radius-control, 6px);
        background: transparent;
        color: var(--gds-foreground);
        cursor: pointer;
        user-select: none;
        box-sizing: border-box;
    }

    .gds-files-status-button.gds-files-status-button:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-status-button.gds-files-status-button:active {
        background-color: var(--gds-pressed);
    }

    .gds-files-status-button.gds-files-status-button:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-files-status-button.gds-files-status-button:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-status-button-inner.gds-files-status-button-inner {
        display: flex;
        align-items: center;
        gap: 0px;
        min-width: 0px;
        height: 24px;
    }

    .gds-files-status-icon-wrap.gds-files-status-icon-wrap {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 24px;
        height: 24px;
        flex: none;
    }

    .gds-files-status-badge.gds-files-status-badge {
        position: absolute;
        top: 0px;
        right: 0px;
        min-width: 12px;
        height: 12px;
        border-radius: 999px;
        padding: 0px 3px;
        background-color: var(--gds-primary);
        color: var(--gds-primary-foreground, var(--gds-selection-foreground));
        display: flex;
        align-items: center;
        justify-content: center;
        box-sizing: border-box;
        pointer-events: none;
    }

    .gds-files-status-badge-text.gds-files-status-badge-text {
        font-size: var(--gds-text-2xs, 10px);
        line-height: 1;
        font-variant-numeric: tabular-nums;
    }

    .gds-files-status-label-clip {
        display: flex;
        align-items: center;
        overflow: hidden;
        flex: none;
        min-width: 0px;
        height: 18px;
    }

    .gds-files-status-label.gds-files-status-label {
        max-width: 240px;
        min-width: 0px;
        flex-shrink: 0;
        font-size: var(--gds-text-xs, 12px);
        line-height: 18px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-status-popover.gds-files-status-popover {
        width: 320px;
        max-height: 320px;
        padding: 4px;
    }

    .gds-files-status-empty.gds-files-status-empty {
        padding: 12px;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-status-list-wrap.gds-files-status-list-wrap {
        width: 100%;
        min-width: 0px;
    }

    .gds-files-status-list.gds-files-status-list {
        width: 100%;
        height: 100%;
        overflow-x: hidden;
    }

    .gds-files-status-row.gds-files-status-row {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        min-width: 0px;
        height: 32px;
        padding: 0px 8px;
        border-radius: 6px;
        box-sizing: border-box;
        overflow: hidden;
    }

    .gds-files-status-row-body {
        display: flex;
        align-items: center;
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
    }

    .gds-files-status-row-body > * {
        display: flex;
        align-items: center;
        width: 100%;
        min-width: 0px;
        overflow: hidden;
    }

    .gds-files-status-row.gds-files-status-row:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-status-row__artwork {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 16px;
        height: 16px;
        flex: none;
        overflow: visible;
    }

    .gds-files-status-row__artwork-badge {
        position: absolute;
        right: -3px;
        bottom: -3px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 999px;
        background-color: var(--gds-popover, var(--gds-panel));
        pointer-events: none;
    }

    .gds-files-status-row-label.gds-files-status-row-label {
        flex: 1 1 0%;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-status-row__action {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        opacity: 0;
        pointer-events: none;
        transition: opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-status-row.gds-files-status-row:hover .gds-files-status-row__action,
    .gds-files-status-row.gds-files-status-row:focus-within .gds-files-status-row__action {
        opacity: 1;
        pointer-events: auto;
    }
    "#
);

fn install_status_stylesheets() {
    gds::ui::styles::install_ui_stylesheets();
    if current_host().is_some() {
        install_stylesheet(STATUS_SHEET, STATUS_CSS);
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::pages::workspace) struct Summary {
    pub icon: IconName,
    pub count: usize,
    pub label: String,
}

const fn should_collapse_status_badge(window_compact: bool, active_controls: usize) -> bool {
    window_compact || active_controls > 1
}

enum Entry {
    Task {
        key: String,
        label: String,
        cancelled: bool,
    },
    Selection {
        tab: u64,
        row: Row,
    },
    Clipboard {
        expected: Arc<Snapshot>,
        path: PathBuf,
        icon: FileIcon,
    },
}

impl Entry {
    fn title(&self) -> String {
        match self {
            Self::Task { label, .. } => label.clone(),
            Self::Selection { row, .. } => match row {
                Row::File(entry) => entry.path.to_string_lossy().into_owned(),
                Row::Trash(entry) => entry.original_path.to_string_lossy().into_owned(),
                Row::Place(location) => location.display_address(),
                Row::Remote(entry) => entry.address.to_string(),
                Row::Tag(tag) => tag.clone(),
                Row::Volume(volume) => volume.name.clone(),
            },
            Self::Clipboard { path, .. } => path.to_string_lossy().into_owned(),
        }
    }
}

#[derive(Clone, Copy)]
struct ClipboardDetailState {
    icons: RwSignal<HashMap<PathBuf, FileIcon>, LocalStorage>,
    removing: RwSignal<bool, LocalStorage>,
}

pub(in crate::pages::workspace) struct StatusBar<'a>(pub &'a Context);

impl StatusBar<'_> {
    pub(in crate::pages::workspace) fn status_controls(&self) -> AnyView {
        let ctx = self.0.clone();
        AnyView::new(Scoped::new(move || {
            StatusBar(&ctx).status_controls_scoped()
        }))
    }

    fn status_controls_scoped(&self) -> AnyView {
        install_status_stylesheets();

        let is_files = local_memo({
            let ctx = self.0.clone();
            move || ctx.pane() == Pane::Files
        });

        let tasks_summary = local_memo({
            let ctx = self.0.clone();
            move || StatusBar(&ctx).status_summary(Kind::Tasks)
        });
        let selection_summary = local_memo({
            let ctx = self.0.clone();
            move || StatusBar(&ctx).status_summary(Kind::Selection)
        });
        let clipboard_summary = local_memo({
            let ctx = self.0.clone();
            move || StatusBar(&ctx).status_summary(Kind::Clipboard)
        });

        let active_count = Signal::derive_local(move || {
            usize::from(tasks_summary.get().count > 0)
                + usize::from(selection_summary.get().count > 0)
                + usize::from(clipboard_summary.get().count > 0)
        });
        let compact_badge = Signal::derive_local({
            let ctx = self.0.clone();
            move || {
                let geometry = Geometry(&ctx);
                let narrow = geometry.width_tier() != gds::ui::window::WidthTier::Wide
                    || geometry.narrow_collection()
                    || (geometry.height_tier() == gds::ui::window::HeightTier::Micro
                        && (ctx.0.footer_filter_open.get() || !ctx.0.filter.get().is_empty()));
                should_collapse_status_badge(narrow, active_count.get())
            }
        });
        let controls_gap = Signal::derive_local({
            let ctx = self.0.clone();
            move || {
                if Geometry(&ctx).width_tier() != gds::ui::window::WidthTier::Wide {
                    0.0
                } else {
                    4.0
                }
            }
        });

        let clipboard_state = ClipboardDetailState {
            icons: RwSignal::new_local(HashMap::new()),
            removing: RwSignal::new_local(false),
        };
        self.observe_clipboard_icons(clipboard_state);

        let any_visible = Signal::derive_local({
            let ctx = self.0.clone();
            move || {
                is_files.get()
                    && (active_count.get() > 0
                        || matches!(ctx.0.menus.surface.get(), Surface::Status(_)))
            }
        });

        let render_ctx = self.0.clone();
        AnyView::new(view! {
            Show(when = move || any_visible.get()) {
                {{
                    let status = StatusBar(&render_ctx);
                    view! {
                        row(
                            class = "gds-files-status-controls",
                            style:gap = move || Some(format!("{:.1}px", controls_gap.get()))
                        ) {
                            {status.status_control(Kind::Tasks, tasks_summary, compact_badge, clipboard_state)}
                            {status.status_control(Kind::Selection, selection_summary, compact_badge, clipboard_state)}
                            {status.status_control(Kind::Clipboard, clipboard_summary, compact_badge, clipboard_state)}
                        }
                    }
                }}
            }
        })
    }

    fn observe_clipboard_icons(&self, state: ClipboardDetailState) {
        let ctx = self.0.clone();
        let icons_sig = state.icons;
        let fx = RenderEffect::new(move |_| {
            let snapshot = Clipboard::current().snapshot();
            let Some(snapshot) = snapshot.filter(|snap| !snap.paths.is_empty()) else {
                if !icons_sig.with_untracked(HashMap::is_empty) {
                    icons_sig.set(HashMap::new());
                }
                return;
            };

            let active_set: HashSet<&Path> = snapshot.paths.iter().map(PathBuf::as_path).collect();
            let mut local_hits: Vec<(PathBuf, FileIcon)> = Vec::new();
            let mut to_probe: Vec<PathBuf> = Vec::new();

            icons_sig.with_untracked(|cached| {
                ctx.read_untracked(|ws| {
                    let tab_rows = &ws.tab().rows;
                    for path in &snapshot.paths {
                        if cached.contains_key(path.as_path()) {
                            continue;
                        }
                        if let Some(entry) = tab_rows.iter().find_map(|row| match row {
                            Row::File(item) if item.path == *path => Some(item),
                            _ => None,
                        }) {
                            local_hits.push((path.clone(), FileIcon::for_entry(entry)));
                        } else {
                            to_probe.push(path.clone());
                        }
                    }
                });
            });

            let needs_prune = icons_sig
                .with_untracked(|cached| cached.keys().any(|p| !active_set.contains(p.as_path())));
            if needs_prune || !local_hits.is_empty() {
                icons_sig.update(|cached| {
                    cached.retain(|p, _| active_set.contains(p.as_path()));
                    for (path, icon) in local_hits {
                        cached.insert(path, icon);
                    }
                });
            }

            if to_probe.is_empty() {
                return;
            }
            let expected = Arc::clone(&snapshot);
            ctx.blocking(
                "Inspect clipboard files",
                move || {
                    let probed = to_probe
                        .into_iter()
                        .map(|path| {
                            let icon = directory::Entry::inspect(&path).map_or_else(
                                |_| FileIcon::plain(IconName::FileQuestionMark),
                                |entry| FileIcon::for_entry(&entry),
                            );
                            (path, icon)
                        })
                        .collect::<Vec<_>>();
                    Ok((expected, probed))
                },
                move |_ctx, result| {
                    let Ok((expected, probed)) = result else {
                        return;
                    };
                    let still_matches = Clipboard::current().snapshot().is_some_and(|current| {
                        Arc::ptr_eq(&current, &expected) || current == expected
                    });
                    if still_matches && !probed.is_empty() {
                        icons_sig.update(|cached| {
                            for (path, icon) in probed {
                                cached.insert(path, icon);
                            }
                        });
                    }
                },
            );
        });
        on_cleanup_local(move || drop(fx));
    }

    pub(in crate::pages::workspace) fn status_summary(&self, kind: Kind) -> Summary {
        if let Some(gds) = gds::app::Gds::try_current() {
            let _ = gds.i18n().language();
        }
        match kind {
            Kind::Tasks => {
                let tasks = self.0.background_task_list();
                let count = tasks.len();
                let label = match count {
                    0 => String::new(),
                    1 => tasks[0].display_label(),
                    _ => i18n::count("tasks-count", count),
                };
                Summary {
                    icon: IconName::ListTodo,
                    count,
                    label,
                }
            }
            Kind::Selection => self.0.read(|state| {
                let count = state.tab().navigation.view().selection.len();
                Summary {
                    icon: IconName::Check,
                    count,
                    label: if count == 0 {
                        String::new()
                    } else {
                        i18n::count("selection-count", count)
                    },
                }
            }),
            Kind::Clipboard => {
                let snapshot = Clipboard::current().snapshot();
                let count = snapshot.as_ref().map_or(0, |snap| snap.paths.len());
                let icon = if snapshot.as_ref().is_some_and(|snap| snap.mode == Mode::Cut) {
                    IconName::Scissors
                } else {
                    IconName::Copy
                };
                let label = snapshot.as_ref().map_or_else(String::new, |snap| {
                    if count == 0 {
                        String::new()
                    } else if let [path] = snap.paths.as_slice() {
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        i18n::count("item-count", count)
                    }
                });
                Summary { icon, count, label }
            }
        }
    }

    fn status_control(
        &self,
        kind: Kind,
        summary: Signal<Summary, LocalStorage>,
        compact_badge: Signal<bool, LocalStorage>,
        clipboard_state: ClipboardDetailState,
    ) -> AnyView {
        let open = Signal::derive_local({
            let ctx = self.0.clone();
            move || matches!(ctx.0.menus.surface.get(), Surface::Status(current) if current == kind)
        });
        let visible = Signal::derive_local(move || summary.get().count > 0 || open.get());

        let expanded_target =
            Signal::derive_local(move || if compact_badge.get() { 0.0 } else { 1.0 });
        let expanded = use_transition(expanded_target, WORKSPACE_DURATION_MS);

        let control_ctx = self.0.clone();
        AnyView::new(view! {
            Show(when = move || visible.get()) {
                {{
                    StatusBar(&control_ctx).status_control_mounted(
                        kind,
                        summary,
                        expanded,
                        open,
                        clipboard_state,
                    )
                }}
            }
        })
    }

    fn status_control_mounted(
        &self,
        kind: Kind,
        summary: Signal<Summary, LocalStorage>,
        expanded: Signal<f32, LocalStorage>,
        open: Signal<bool, LocalStorage>,
        clipboard_state: ClipboardDetailState,
    ) -> AnyView {
        let ctx = self.0.clone();
        AnyView::new(Scoped::new(move || {
            let anchor = NodeRef::new();
            let label_inner_ref = NodeRef::new();
            let label_size = label_inner_ref.observe_border_size();

            let toggle_ctx = ctx.clone();
            let close_ctx = ctx.clone();
            let details_ctx = ctx.clone();

            let _overlay = OverlayState::new(
                Binding::controlled(open, move |next: bool| {
                    if !next {
                        close_ctx.0.menus.close();
                    }
                }),
                false,
                None,
            )
            .anchored_to(anchor)
            .provide();

            let title = Signal::derive_local(move || i18n::text(kind.title()));

            let show_badge = Signal::derive_local(move || {
                summary.get().count > 0 && expanded.get() < 1.0 - 1e-4
            });
            let show_label = Signal::derive_local(move || {
                !summary.get().label.is_empty() && expanded.get() > 1e-4
            });

            let button = view! {
                HintTooltip(text = title) {
                    control(
                        node_ref = anchor,
                        class = "gds-files-status-button",
                        style:height = format!("{CONTROL_SIZE}px"),
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        a11y:label = move || SharedString::from(title.get()),
                        on:click = move |_| {
                            toggle_ctx.0.menus.surface.update(|surface| {
                                *surface = if matches!(surface, Surface::Status(current) if *current == kind) {
                                    Surface::Closed
                                } else {
                                    Surface::Status(kind)
                                };
                            });
                            if kind == Kind::Clipboard {
                                Clipboard::current().refresh();
                            }
                        }
                    ) {
                        row(class = "gds-files-status-button-inner") {
                            box(class = "gds-files-status-icon-wrap") {
                                Icon(
                                    name = Signal::derive_local(move || summary.get().icon),
                                    size = TRIGGER_ICON_SIZE
                                ) {}
                                Show(when = move || show_badge.get()) {
                                    box(
                                        class = "gds-files-status-badge",
                                        style:opacity = move || {
                                            Some(format!("{:.3}", (1.0 - expanded.get()).clamp(0.0, 1.0)))
                                        }
                                    ) {
                                        text(class = "gds-files-status-badge-text") {
                                            {move || summary.get().count.to_string()}
                                        }
                                    }
                                }
                            }
                            Show(when = move || show_label.get()) {
                                box(
                                    class = "gds-files-status-label-clip",
                                    style:opacity = move || {
                                        Some(format!("{:.3}", expanded.get().clamp(0.0, 1.0)))
                                    },
                                    style:margin-right = move || {
                                        Some(format!(
                                            "{:.2}px",
                                            LABEL_TRAILING_GAP * expanded.get().clamp(0.0, 1.0)
                                        ))
                                    },
                                    style:width = move || {
                                        let exp = expanded.get().clamp(0.0, 1.0);
                                        if exp >= 1.0 - 1e-4 {
                                            return None;
                                        }
                                        let scale = label_inner_ref.scale().max(1.0);
                                        let natural = label_size
                                            .get()
                                            .map(|sz| (sz.width.0 / scale).min(LABEL_MAX_WIDTH))
                                            .filter(|w| *w > 0.0)
                                            .unwrap_or_else(|| {
                                                let chars = summary.get().label.chars().count() as f32;
                                                (chars * 7.2).clamp(24.0, LABEL_MAX_WIDTH)
                                            });
                                        Some(format!("{:.2}px", natural * exp))
                                    }
                                ) {
                                    text(node_ref = label_inner_ref, class = "gds-files-status-label") {
                                        {move || summary.get().label}
                                    }
                                }
                            }
                        }
                    }
                }
            };

            view! {
                box(class = "gds-files-status-slot") {
                    {button}
                    PopoverContent(
                        placement = Placement::new(Side::Top, Align::Start),
                        class = "gds-files-status-popover"
                    ) {
                        {{
                            let c = details_ctx.clone();
                            move || StatusBar(&c).status_details(kind, clipboard_state)
                        }}
                    }
                }
            }
        }))
    }

    fn status_details(&self, kind: Kind, clipboard_state: ClipboardDetailState) -> AnyView {
        let rows: Vec<Entry> = match kind {
            Kind::Tasks => self
                .0
                .background_task_list()
                .into_iter()
                .map(|task| Entry::Task {
                    key: task.key.clone(),
                    label: task.display_label(),
                    cancelled: task.is_cancelled(),
                })
                .collect(),
            Kind::Selection => {
                let (tab, rows) = self
                    .0
                    .read(|state| (state.tab().id, state.tab().selected()));
                rows.into_iter()
                    .map(|row| Entry::Selection { tab, row })
                    .collect()
            }
            Kind::Clipboard => {
                let icons = clipboard_state.icons.get();
                Clipboard::current()
                    .snapshot()
                    .map_or_else(Vec::new, |snapshot| {
                        snapshot
                            .paths
                            .iter()
                            .cloned()
                            .map(|path| {
                                let icon = icons
                                    .get(&path)
                                    .copied()
                                    .unwrap_or_else(|| FileIcon::plain(IconName::File));
                                Entry::Clipboard {
                                    expected: Arc::clone(&snapshot),
                                    path,
                                    icon,
                                }
                            })
                            .collect()
                    })
            }
        };
        if rows.is_empty() {
            return AnyView::new(view! {
                text(class = "gds-files-status-empty") {{move || i18n::text(kind.empty())}}
            });
        }
        let rows = Rc::new(rows);
        let count = rows.len();
        let ctx = self.0.clone();
        let height = (count as f32 * POPOVER_ROW_HEIGHT).min(POPOVER_MAX_LIST_HEIGHT);
        AnyView::new(view! {
            box(
                class = "gds-files-status-list-wrap",
                style:height = format!("{height:.1}px")
            ) {
                VirtualList(
                    count = count,
                    row_size = POPOVER_ROW_HEIGHT,
                    label = i18n::text(kind.title()),
                    class = "gds-files-status-list",
                    row = move |index: usize| {
                        rows.get(index)
                            .map(|entry| StatusBar(&ctx).status_entry(entry, clipboard_state.removing))
                            .unwrap_or_else(|| AnyView::new(view! { box() }))
                    }
                )
            }
        })
    }

    fn status_entry(
        &self,
        entry: &Entry,
        clipboard_removing: RwSignal<bool, LocalStorage>,
    ) -> AnyView {
        let title = entry.title();
        match entry {
            Entry::Task {
                key,
                label,
                cancelled,
            } => {
                let ctx = self.0.clone();
                let task_key = key.clone();
                let is_cancelled = *cancelled;
                let display_label = if is_cancelled {
                    format!("{label} — {}", i18n::text_untracked("tasks-cancelling"))
                } else {
                    label.clone()
                };
                self.status_row(
                    FileIcon::plain(IconName::ListTodo),
                    display_label.clone(),
                    display_label,
                    AnyView::new(view! {
                        IconButton(
                            icon = IconName::X,
                            label = Signal::derive_local(|| i18n::text("operation-cancel")),
                            size = ButtonSize::IconXs,
                            disabled = Signal::stored_local(is_cancelled),
                            on:click = move |_| {
                                ctx.cancel_background_task(&task_key);
                            }
                        )
                    }),
                )
            }
            Entry::Selection { tab, row } => {
                let tab = *tab;
                let ctx = self.0.clone();
                let key = row.key();
                let icon = match row {
                    Row::File(file_entry) => FileIcon::for_entry(file_entry),
                    _ => FileIcon::plain(row.icon()),
                };
                self.status_row(
                    icon,
                    row.label(),
                    title,
                    AnyView::new(view! {
                        IconButton(
                            icon = IconName::X,
                            label = Signal::derive_local(|| i18n::text("selection-remove-item")),
                            size = ButtonSize::IconXs,
                            on:click = move |_| {
                                ctx.update_selection(|state| {
                                    if state.tab().id != tab {
                                        return;
                                    }
                                    let visit = state.tab_mut().navigation.view_mut();
                                    visit.selection.remove(&key);
                                    if visit.anchor.as_ref() == Some(&key) {
                                        visit.anchor = None;
                                    }
                                    if visit.cursor.as_ref() == Some(&key) {
                                        visit.cursor = None;
                                    }
                                });
                            }
                        )
                    }),
                )
            }
            Entry::Clipboard {
                expected,
                path,
                icon,
            } => {
                let path = path.clone();
                let label = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                let expected = Arc::clone(expected);
                let ctx = self.0.clone();
                self.status_row(
                    *icon,
                    label,
                    title,
                    AnyView::new(view! {
                        IconButton(
                            icon = IconName::X,
                            label = Signal::derive_local(|| i18n::text("clipboard-remove-item")),
                            size = ButtonSize::IconXs,
                            disabled = Signal::from(clipboard_removing.read_only()),
                            on:click = move |_| {
                                if clipboard_removing.get_untracked() {
                                    return;
                                }
                                clipboard_removing.set(true);
                                let notice = ctx.clone();
                                let expected = Arc::clone(&expected);
                                let path = path.clone();
                                ctx.0.owner.with(|| {
                                    drop(spawn_local(async move {
                                        let outcome =
                                            Clipboard::current().remove(expected, vec![path]).await;
                                        clipboard_removing.set(false);
                                        if let Err(error) = outcome {
                                            notice.report_notice(error.to_string());
                                        }
                                        Clipboard::current().refresh();
                                    }));
                                });
                            }
                        )
                    }),
                )
            }
        }
    }

    fn status_row(&self, icon: FileIcon, label: String, title: String, action: AnyView) -> AnyView {
        let tooltip = wrap_tokens(&title);
        AnyView::new(view! {
            row(class = "gds-files-status-row") {
                {status_artwork(icon, ICON_SIZE)}
                box(class = "gds-files-status-row-body") {
                    HintTooltip(text = tooltip) {
                        text(class = "gds-files-status-row-label") {{label}}
                    }
                }
                box(class = "gds-files-status-row__action") {
                    {action}
                }
            }
        })
    }
}

fn status_artwork(icon: FileIcon, size: f32) -> AnyView {
    let badge = icon.badge().map(|badge_icon| {
        let badge_px = (size * ARTWORK_BADGE_RATIO).max(9.0);
        view! {
            box(class = "gds-files-status-row__artwork-badge") {
                Icon(name = badge_icon, size = badge_px)
            }
        }
        .into_view()
    });
    AnyView::new(view! {
        box(class = "gds-files-status-row__artwork") {
            Icon(name = icon.base(), size = size) {}
            {badge}
        }
    })
}

#[cfg(test)]
mod tests;
