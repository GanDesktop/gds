//! Native tab reordering, pointer/OS drag-and-drop, spring-loading, and lazy file drag snapshots.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, OnceLock},
    time::Duration,
};

use gds::appearance::tokens::ICON_SIZE;
use gds::ui::{
    IconName, IconProps,
    drag::{DRAG_THRESHOLD, DragCardProps, DragOverlayState, drop_hit_test, drop_target_attrs},
};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::{EventCx, ListenerGuard, ListenerOptions, TimeoutHandle, Timers, events};
use zgui::vocab::{Key, NamedKey, PointerButton};
use zgui::{css, view};

use super::{
    geometry::Grid,
    model::{Contents, Row},
    ui::Browser,
};
use crate::{
    entities::file::{
        directory::{Entry, EntryKind},
        ui::FileIcon,
    },
    features::file_operations::{paste::PasteJob, transfer},
    shared::{i18n, lib::selection::Selection},
};

const PREVIEW_VISIBLE_ITEMS: usize = 3;
const SPRING_LOAD_DELAY: Duration = Duration::from_millis(600);
const DRAG_PREVIEW_SHEET: &str = "gds-files-drag-preview";
const DRAG_PREVIEW_CSS: &str = css!(
    r#"
    .gds-files-drag-item {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-drag-item__icon {
        flex-shrink: 0;
        color: var(--gds-muted-foreground);
    }

    .gds-files-drag-header__icon {
        flex-shrink: 0;
        color: var(--gds-primary);
    }

    .gds-files-drag-item__name {
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-drag-row-host {
        display: flex;
        flex-direction: column;
        min-width: 0px;
    }

    .gds-files-drag-row-host.gds-drop-target.gds-drop-target[data-drop-active="true"] {
        background: transparent;
        outline: none;
    }

    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-row-content,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-row-content,
    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-row-content.gds-files-row-content,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-row-content.gds-files-row-content,
    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-tile,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-tile,
    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-tile.gds-files-tile,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-tile.gds-files-tile {
        background-color: var(--gds-drag-target);
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }
    "#
);

#[derive(Clone, Debug)]
struct DragItem {
    path: PathBuf,
    name: String,
    icon: FileIcon,
}

impl DragItem {
    fn new(entry: &Entry) -> Self {
        Self {
            path: entry.path.clone(),
            name: entry.display_name.clone(),
            icon: FileIcon::for_entry(entry),
        }
    }
}

#[derive(Debug)]
struct DragSnapshotSource {
    rows: Arc<[Entry]>,
    selected: Selection<PathBuf>,
}

#[derive(Debug)]
pub(in crate::pages::workspace) struct DragSnapshotInner {
    source: std::sync::Mutex<Option<DragSnapshotSource>>,
    entries: OnceLock<Arc<[DragItem]>>,
}

impl DragSnapshotInner {
    fn entries(&self) -> &[DragItem] {
        self.entries
            .get_or_init(|| {
                let Some(source) = self
                    .source
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                else {
                    return Arc::from([]);
                };
                source
                    .rows
                    .iter()
                    .filter(|row| source.selected.contains(&row.path))
                    .map(DragItem::new)
                    .collect::<Vec<_>>()
                    .into()
            })
            .as_ref()
    }
}

#[derive(Clone, Debug)]
enum DragPayload {
    Single(Entry),
    Selection(Arc<DragSnapshotInner>),
}

/// Files carried by a drag that started in the file table.
#[derive(Clone, Debug)]
pub(super) struct FileDrag {
    payload: DragPayload,
}

impl FileDrag {
    fn selection(inner: Arc<DragSnapshotInner>) -> Self {
        Self {
            payload: DragPayload::Selection(inner),
        }
    }

    fn single(entry: &Entry) -> Self {
        Self {
            payload: DragPayload::Single(entry.clone()),
        }
    }

    pub(super) fn is_source(path: &Path) -> bool {
        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow()
                .values()
                .any(|sources| !sources.is_empty() && sources.contains(path))
        })
    }

    pub(super) fn paths(&self) -> Vec<PathBuf> {
        match &self.payload {
            DragPayload::Single(entry) => vec![entry.path.clone()],
            DragPayload::Selection(inner) => inner
                .entries()
                .iter()
                .map(|item| item.path.clone())
                .collect(),
        }
    }

    pub(super) fn count(&self) -> usize {
        match &self.payload {
            DragPayload::Single(_) => 1,
            DragPayload::Selection(inner) => inner.entries().len(),
        }
    }

    fn preview_items(&self) -> Vec<DragItem> {
        match &self.payload {
            DragPayload::Single(entry) => vec![DragItem::new(entry)],
            DragPayload::Selection(inner) => inner
                .entries()
                .iter()
                .take(PREVIEW_VISIBLE_ITEMS)
                .cloned()
                .collect(),
        }
    }
}

/// Frozen selection for drag payloads whose item slice materializes lazily on drag start.
#[derive(Clone, Default)]
pub(super) struct DragSnapshot {
    inner: Option<Arc<DragSnapshotInner>>,
}

impl DragSnapshot {
    pub(super) fn new(rows: &Arc<[Entry]>, selected: &Selection<PathBuf>) -> Self {
        if rows.is_empty() || selected.len() <= 1 {
            return Self { inner: None };
        }
        Self {
            inner: Some(Arc::new(DragSnapshotInner {
                source: std::sync::Mutex::new(Some(DragSnapshotSource {
                    rows: Arc::clone(rows),
                    selected: selected.clone(),
                })),
                entries: OnceLock::new(),
            })),
        }
    }

    #[cfg(test)]
    pub(super) fn drag_for(&self, entry: &Entry, selected: &Selection<PathBuf>) -> FileDrag {
        self.drag_for_row(entry, selected.contains(&entry.path))
    }

    pub(super) fn drag_for_row(&self, entry: &Entry, selected: bool) -> FileDrag {
        match (&self.inner, selected) {
            (Some(inner), true) => FileDrag::selection(Arc::clone(inner)),
            _ => FileDrag::single(entry),
        }
    }

    #[cfg(test)]
    pub(super) fn is_materialized(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.entries.get().is_some())
    }
}

#[derive(Clone)]
enum WindowDragPayload {
    Files(FileDrag),
    Tab { id: u64, title: String },
}

#[derive(Clone)]
enum ArmedDrag {
    File {
        entry: Entry,
        tab_id: u64,
        start: (f32, f32),
    },
    Tab {
        id: u64,
        pinned: bool,
        start: (f32, f32),
    },
}

#[derive(Clone)]
enum TargetKind {
    Folder(PathBuf),
    Tab(u64),
}

#[derive(Clone)]
struct RegisteredTarget {
    token: u64,
    key: String,
    node: NodeRef,
    kind: TargetKind,
}

#[derive(Clone)]
struct RegisteredTab {
    token: u64,
    id: u64,
    pinned: bool,
    node: NodeRef,
}

struct SpringLoad {
    _timer: TimeoutHandle,
}

struct DragCoordinator {
    overlay: DragOverlayState<WindowDragPayload>,
    armed: RefCell<Option<ArmedDrag>>,
    targets: RefCell<Vec<RegisteredTarget>>,
    tabs: RefCell<Vec<RegisteredTab>>,
    spring: RefCell<Option<SpringLoad>>,
    copy_modifier: Cell<bool>,
    next_token: Cell<u64>,
}

impl DragCoordinator {
    fn new() -> Self {
        Self {
            overlay: DragOverlayState::new(),
            armed: RefCell::new(None),
            targets: RefCell::new(Vec::new()),
            tabs: RefCell::new(Vec::new()),
            spring: RefCell::new(None),
            copy_modifier: Cell::new(false),
            next_token: Cell::new(1),
        }
    }

    fn alloc_token(&self) -> u64 {
        let token = self.next_token.get();
        self.next_token.set(token.saturating_add(1));
        token
    }

    fn register_target(&self, key: String, node: NodeRef, kind: TargetKind) -> u64 {
        let token = self.alloc_token();
        let mut targets = self.targets.borrow_mut();
        targets.retain(|item| item.key != key);
        targets.push(RegisteredTarget {
            token,
            key,
            node,
            kind,
        });
        token
    }

    fn unregister_target(&self, token: u64) {
        self.targets.borrow_mut().retain(|item| item.token != token);
    }

    fn register_tab(&self, id: u64, pinned: bool, node: NodeRef) -> u64 {
        let token = self.alloc_token();
        let mut tabs = self.tabs.borrow_mut();
        tabs.retain(|item| item.id != id);
        tabs.push(RegisteredTab {
            token,
            id,
            pinned,
            node,
        });
        token
    }

    fn unregister_tab(&self, token: u64) {
        self.tabs.borrow_mut().retain(|item| item.token != token);
    }

    fn hit_target_excluding(
        &self,
        pos: (f32, f32),
        excluded_sources: &HashSet<PathBuf>,
    ) -> Option<RegisteredTarget> {
        self.targets
            .borrow()
            .iter()
            .rev()
            .find(|item| {
                if let TargetKind::Folder(dest) = &item.kind
                    && excluded_sources.contains(dest)
                {
                    return false;
                }
                drop_hit_test(&item.node, pos)
            })
            .cloned()
    }

    fn hit_tab(&self, pos: (f32, f32)) -> Option<RegisteredTab> {
        self.tabs
            .borrow()
            .iter()
            .find(|item| drop_hit_test(&item.node, pos))
            .cloned()
    }

    fn clear_spring(&self) {
        *self.spring.borrow_mut() = None;
    }

    fn cancel_all(&self) {
        *self.armed.borrow_mut() = None;
        self.clear_spring();
        self.overlay.cancel();
    }
}

thread_local! {
    static COORDINATORS: RefCell<HashMap<usize, Rc<DragCoordinator>>> = RefCell::new(HashMap::new());
    static CLEANUP_REGISTERED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    static ACTIVE_DRAG_SOURCES: RefCell<HashMap<usize, HashSet<PathBuf>>> = RefCell::new(HashMap::new());
    static ROW_PRESSED_WINDOW: Cell<usize> = const { Cell::new(0) };
}

fn set_active_drag_sources(browser: &Browser, paths: Vec<PathBuf>) {
    let window_key = Rc::as_ptr(&browser.0) as usize;
    let set: HashSet<PathBuf> = paths.iter().cloned().collect();
    ACTIVE_DRAG_SOURCES.with(|slot| {
        slot.borrow_mut().insert(window_key, set);
    });
    browser.set_drag_sources(paths);
}

fn clear_active_drag_sources(browser: &Browser) {
    let window_key = Rc::as_ptr(&browser.0) as usize;
    ACTIVE_DRAG_SOURCES.with(|slot| {
        slot.borrow_mut().remove(&window_key);
    });
    browser.clear_drag_sources();
}

pub(super) fn mark_row_pointer_down(browser: &Browser) {
    ROW_PRESSED_WINDOW.with(|slot| slot.set(Rc::as_ptr(&browser.0) as usize));
}

pub(super) fn take_row_pointer_down(browser: &Browser) -> bool {
    let key = Rc::as_ptr(&browser.0) as usize;
    ROW_PRESSED_WINDOW.with(|slot| {
        let matched = slot.get() == key;
        slot.set(0);
        matched
    })
}

fn coordinator_for(browser: &Browser) -> Rc<DragCoordinator> {
    let window_key = Rc::as_ptr(&browser.0) as usize;
    if let Some(coord) = COORDINATORS.with(|slot| slot.borrow().get(&window_key).cloned()) {
        return coord;
    }
    // DragCoordinator allocates RwSignal overlay state; anchor it to the window owner so
    // unmounting the first list/row branch does not dispose signals cached per window.
    let coord = browser.0.owner.with(|| Rc::new(DragCoordinator::new()));
    let needs_cleanup = CLEANUP_REGISTERED.with(|slot| slot.borrow_mut().insert(window_key));
    if needs_cleanup {
        let root_node = browser.0.root_focus;
        let weak = browser.downgrade();
        let esc_coord = Rc::clone(&coord);
        browser.0.owner.with(|| {
            let key_guard: Rc<RefCell<Option<ListenerGuard>>> = Rc::new(RefCell::new(None));
            let key_guard_watch = Rc::clone(&key_guard);
            let attach_fx = RenderEffect::new(move |_| {
                if root_node.get().is_none() || key_guard_watch.borrow().is_some() {
                    return;
                }
                let weak_inner = weak.clone();
                let coord_inner = Rc::clone(&esc_coord);
                let guard = root_node.listen(
                    events::KEY_DOWN,
                    ListenerOptions::CAPTURE,
                    move |ev: &mut EventCx<'_, events::KeyDown>| {
                        if ev.key != Key::Named(NamedKey::Escape) {
                            return;
                        }
                        if coord_inner.overlay.payload().get_untracked().is_none() {
                            return;
                        }
                        coord_inner.cancel_all();
                        if let Some(browser) = weak_inner.upgrade() {
                            clear_active_drag_sources(&browser);
                        }
                        ev.prevent_default();
                        ev.stop_propagation();
                    },
                );
                *key_guard_watch.borrow_mut() = guard;
            });

            on_cleanup_local(move || {
                drop(attach_fx);
                drop(key_guard.borrow_mut().take());
                COORDINATORS.with(|slot| {
                    slot.borrow_mut().remove(&window_key);
                });
                CLEANUP_REGISTERED.with(|slot| {
                    slot.borrow_mut().remove(&window_key);
                });
                ACTIVE_DRAG_SOURCES.with(|slot| {
                    slot.borrow_mut().remove(&window_key);
                });
            });
        });
    }
    COORDINATORS.with(|slot| {
        Rc::clone(
            slot.borrow_mut()
                .entry(window_key)
                .or_insert_with(|| Rc::clone(&coord)),
        )
    })
}

impl Browser {
    pub(super) fn background_drop_target(&self, view: AnyView) -> AnyView {
        if zgui::view::current_host().is_some() {
            zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
        }
        let coord = coordinator_for(self);
        let overlay = coord.overlay;
        let browser = self.clone();
        let on_external_drop =
            UnsyncCallback::new(move |(paths, _pos): (Vec<PathBuf>, (f32, f32))| {
                let destination = browser.read_untracked(|state| {
                    let tab = state.tab();
                    matches!(tab.contents, Contents::Directory)
                        .then(|| tab.navigation.path().map(Path::to_path_buf))
                        .flatten()
                });
                if let Some(destination) = destination
                    && !paths.is_empty()
                {
                    let job = PasteJob::new(destination, transfer::Mode::Copy, paths);
                    browser.inspect_paste(job, false, true, None);
                }
            });
        let inactive = Signal::stored_local(false);
        let drop_attrs = drop_target_attrs(inactive, Some(on_external_drop));
        let copy_mode = Signal::derive_local(move || {
            let _ = overlay.position().get();
            coord.copy_modifier.get()
        });

        AnyView::new(view! {
            box(class = "gds-files-drop-background", {..drop_attrs}) {
                {view}
                {move || overlay.payload().get().map(|payload| {
                    match payload {
                        WindowDragPayload::Files(drag) => {
                            let count = drag.count();
                            let items = drag.preview_items();
                            let copy_hint = copy_mode.get().then(|| i18n::text("copy-files"));
                            if count <= 1 {
                                let (icon, label) = items
                                    .into_iter()
                                    .next()
                                    .map(|item| (item.icon.base(), item.name))
                                    .unwrap_or_else(|| (IconName::File, gds::i18n::count("item-count", 1)));
                                AnyView::new(view! {
                                    DragCard(position = overlay.position()) {
                                        column(class = "gds-drag-card__content") {
                                            row(class = "gds-files-drag-item") {
                                                Icon(name = icon, size = ICON_SIZE, class = "gds-files-drag-item__icon")
                                                text(class = "gds-files-drag-item__name") {{label}}
                                            }
                                        }
                                        {copy_hint.map(|hint| view! {
                                            row(class = "gds-drag-card__footer") {{hint}}
                                        })}
                                    }
                                })
                            } else {
                                let remaining = count.saturating_sub(PREVIEW_VISIBLE_ITEMS);
                                let footer_text = if remaining > 0 {
                                    Some(gds::i18n::count("operation-more-items", remaining))
                                } else {
                                    copy_hint
                                };
                                let rows = items
                                    .into_iter()
                                    .map(|item| {
                                        view! {
                                            row(class = "gds-files-drag-item") {
                                                Icon(
                                                    name = item.icon.base(),
                                                    size = ICON_SIZE,
                                                    class = "gds-files-drag-item__icon",
                                                )
                                                text(class = "gds-files-drag-item__name") {{item.name}}
                                            }
                                        }
                                        .into_view()
                                    })
                                    .collect::<Vec<_>>();
                                AnyView::new(view! {
                                    DragCard(position = overlay.position()) {
                                        row(class = "gds-drag-card__header") {
                                            Icon(
                                                name = FileIcon::multiple().base(),
                                                size = ICON_SIZE,
                                                class = "gds-files-drag-header__icon",
                                            )
                                            text(class = "gds-files-drag-item__name") {
                                                {gds::i18n::count("item-count", count)}
                                            }
                                        }
                                        column(class = "gds-drag-card__content") {{rows}}
                                        {footer_text.map(|more| view! {
                                            row(class = "gds-drag-card__footer") {{more}}
                                        })}
                                    }
                                })
                            }
                        }
                        WindowDragPayload::Tab { title, .. } => AnyView::new(view! {
                            DragCard(position = overlay.position()) {
                                column(class = "gds-drag-card__content") {
                                    row(class = "gds-files-drag-item") {
                                        Icon(name = IconName::Folder, size = ICON_SIZE, class = "gds-files-drag-item__icon")
                                        text(class = "gds-files-drag-item__name") {{title}}
                                    }
                                }
                            }
                        }),
                    }
                })}
            }
        })
    }

    pub(super) fn draggable_tab(&self, view: AnyView, id: u64, pinned: bool) -> AnyView {
        let coord = coordinator_for(self);
        let node = NodeRef::new();
        let target_key = format!("tab:{id}");
        let tab_token = coord.register_tab(id, pinned, node);
        let target_token = coord.register_target(target_key.clone(), node, TargetKind::Tab(id));
        let cleanup_coord = Rc::clone(&coord);
        on_cleanup_local(move || {
            cleanup_coord.unregister_tab(tab_token);
            cleanup_coord.unregister_target(target_token);
        });

        let overlay = coord.overlay;
        let is_hovered = Signal::derive_local(move || {
            overlay.hovered_target().get().as_deref() == Some(&target_key)
        });

        let down_coord = Rc::clone(&coord);
        let move_coord = Rc::clone(&coord);
        let move_browser = self.clone();
        let up_coord = Rc::clone(&coord);
        let up_browser = self.clone();
        let cancel_coord = Rc::clone(&coord);

        AnyView::new(view! {
            box(
                class = "gds-files-sidebar-row-wrap",
                node_ref = node,
                attr:data-drop-active = move || is_hovered.get().then(|| "true".to_owned()),
                on:pointer_down = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    *down_coord.armed.borrow_mut() = Some(ArmedDrag::Tab {
                        id,
                        pinned,
                        start: (ev.position.x.0, ev.position.y.0),
                    });
                },
                on:pointer_move = move |ev| {
                    let pos = (ev.position.x.0, ev.position.y.0);
                    let mut start_payload = None;
                    if let Some(ArmedDrag::Tab {
                        id: armed_id,
                        pinned: armed_pinned,
                        start,
                    }) = *move_coord.armed.borrow()
                        && armed_id == id
                        && distance(start, pos) >= DRAG_THRESHOLD
                    {
                        let _ = armed_pinned;
                        let title = move_browser.read_untracked(|state| {
                            state
                                .tabs
                                .iter()
                                .find(|tab| tab.id == id)
                                .map_or_else(String::new, |tab| {
                                    move_browser.location_title(tab.navigation.location())
                                })
                        });
                        start_payload = Some(WindowDragPayload::Tab { id, title });
                    }
                    if let Some(payload) = start_payload {
                        *move_coord.armed.borrow_mut() = None;
                        ev.capture_pointer();
                        move_coord.overlay.begin(payload, pos.0, pos.1);
                    }
                    if matches!(move_coord.overlay.payload().get_untracked(), Some(WindowDragPayload::Tab { .. })) {
                        move_coord.overlay.move_to(pos.0, pos.1);
                        let hovered = move_coord
                            .hit_tab(pos)
                            .filter(|tab| tab.pinned == pinned)
                            .map(|tab| format!("tab:{}", tab.id));
                        move_coord.overlay.set_hovered_target(hovered);
                    }
                },
                on:pointer_up = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    if matches!(
                        up_coord.armed.borrow().as_ref(),
                        Some(ArmedDrag::Tab { id: armed_id, .. }) if *armed_id == id
                    ) {
                        *up_coord.armed.borrow_mut() = None;
                    }
                    let is_tab_drag = matches!(
                        up_coord.overlay.payload().get_untracked(),
                        Some(WindowDragPayload::Tab { .. })
                    );
                    if !is_tab_drag {
                        return;
                    }
                    ev.release_pointer();
                    let pos = (ev.position.x.0, ev.position.y.0);
                    if let Some(WindowDragPayload::Tab { id: from_id, .. }) = up_coord.overlay.end()
                        && let Some(target_tab) = up_coord.hit_tab(pos)
                        && target_tab.id != from_id
                        && let (Some(from_idx), Some(to_idx)) = (
                            up_browser.tab_index(from_id),
                            up_browser.tab_index(target_tab.id),
                        )
                    {
                        up_browser.reorder_tab(from_idx, to_idx);
                        ev.stop_propagation();
                    }
                },
                on:pointer_cancel = move |ev| {
                    ev.release_pointer();
                    cancel_coord.cancel_all();
                },
            ) {
                {view}
            }
        })
    }

    pub(super) fn file_drag_target(&self, view: AnyView, destination: PathBuf) -> AnyView {
        let coord = coordinator_for(self);
        let node = NodeRef::new();
        let target_key = format!("dir:{}", destination.display());
        let target_token = coord.register_target(
            target_key.clone(),
            node,
            TargetKind::Folder(destination.clone()),
        );
        let cleanup_coord = Rc::clone(&coord);
        on_cleanup_local(move || {
            cleanup_coord.unregister_target(target_token);
        });

        let overlay = coord.overlay;
        let internal_active = Signal::derive_local(move || {
            overlay.hovered_target().get().as_deref() == Some(&target_key)
        });

        let drop_browser = self.clone();
        let drop_dest = destination;
        let on_external_drop =
            UnsyncCallback::new(move |(paths, _pos): (Vec<PathBuf>, (f32, f32))| {
                if !paths.is_empty() {
                    let job = PasteJob::new(drop_dest.clone(), transfer::Mode::Copy, paths);
                    drop_browser.inspect_paste(job, false, true, None);
                }
            });
        let attrs = drop_target_attrs(internal_active, Some(on_external_drop));

        AnyView::new(view! {
            box(class = "gds-files-sidebar-row-wrap", node_ref = node, {..attrs}) {
                {view}
            }
        })
    }

    pub(super) fn file_row_drag(&self, view: AnyView, row: &Row, _grid: Option<Grid>) -> AnyView {
        let Row::File(entry) = row else {
            return view;
        };
        if zgui::view::current_host().is_some() {
            zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
        }
        let coord = coordinator_for(self);
        let node = NodeRef::new();

        let dir_attrs = if entry.kind == EntryKind::Directory {
            let dest = entry.path.clone();
            let target_key = format!("row-dir:{}", dest.display());
            let target_token =
                coord.register_target(target_key.clone(), node, TargetKind::Folder(dest.clone()));
            let cleanup_coord = Rc::clone(&coord);
            on_cleanup_local(move || {
                cleanup_coord.unregister_target(target_token);
            });
            let overlay = coord.overlay;
            let internal_active = Signal::derive_local(move || {
                overlay.hovered_target().get().as_deref() == Some(&target_key)
            });
            let drop_browser = self.clone();
            let on_external_drop =
                UnsyncCallback::new(move |(paths, _pos): (Vec<PathBuf>, (f32, f32))| {
                    if !paths.is_empty() {
                        let job = PasteJob::new(dest.clone(), transfer::Mode::Copy, paths);
                        drop_browser.inspect_paste(job, false, true, None);
                    }
                });
            drop_target_attrs(internal_active, Some(on_external_drop))
                .attribute(zgui::view::AttrName::new("data-drop-target"), move || {
                    internal_active.get().then(|| "true".to_owned())
                })
        } else {
            Attrs::new()
        };

        let row_entry = entry.clone();
        let press_browser = self.clone();
        let press_coord = Rc::clone(&coord);
        let move_browser = self.clone();
        let move_coord = Rc::clone(&coord);
        let up_browser = self.clone();
        let up_coord = Rc::clone(&coord);
        let cancel_browser = self.clone();
        let cancel_coord = Rc::clone(&coord);

        AnyView::new(view! {
            box(
                class = "gds-files-drag-row-host",
                node_ref = node,
                {..dir_attrs},
                on:pointer_down = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    mark_row_pointer_down(&press_browser);
                    let tab_id = press_browser.read_untracked(|state| state.tab().id);
                    *press_coord.armed.borrow_mut() = Some(ArmedDrag::File {
                        entry: row_entry.clone(),
                        tab_id,
                        start: (ev.position.x.0, ev.position.y.0),
                    });
                },
                on:pointer_move = move |ev| {
                    let pos = (ev.position.x.0, ev.position.y.0);
                    move_coord.copy_modifier.set(ev.modifiers.control());

                    let start_drag = {
                        let armed = move_coord.armed.borrow();
                        match armed.as_ref() {
                            Some(ArmedDrag::File { entry, tab_id, start })
                                if distance(*start, pos) >= DRAG_THRESHOLD =>
                            {
                                Some((entry.clone(), *tab_id))
                            }
                            _ => None,
                        }
                    };
                    if let Some((entry, tab_id)) = start_drag {
                        *move_coord.armed.borrow_mut() = None;
                        let drag = materialize_file_drag(&move_browser, tab_id, &entry);
                        if let Some(drag) = drag {
                            set_active_drag_sources(&move_browser, drag.paths());
                            ev.capture_pointer();
                            move_coord.overlay.begin(WindowDragPayload::Files(drag), pos.0, pos.1);
                        }
                    }

                    if matches!(
                        move_coord.overlay.payload().get_untracked(),
                        Some(WindowDragPayload::Files(_))
                    ) {
                        move_coord.overlay.move_to(pos.0, pos.1);
                        update_file_drag_hover(&move_browser, &move_coord, pos);
                    }
                },
                on:pointer_up = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    if matches!(up_coord.armed.borrow().as_ref(), Some(ArmedDrag::File { .. })) {
                        *up_coord.armed.borrow_mut() = None;
                    }
                    if !matches!(
                        up_coord.overlay.payload().get_untracked(),
                        Some(WindowDragPayload::Files(_))
                    ) {
                        return;
                    }
                    ev.release_pointer();
                    up_coord.clear_spring();
                    let pos = (ev.position.x.0, ev.position.y.0);
                    let excluded = up_browser.drag_sources().get_untracked();
                    let target = up_coord.hit_target_excluding(pos, &excluded);
                    let ended = up_coord.overlay.end();
                    clear_active_drag_sources(&up_browser);
                    if let Some(WindowDragPayload::Files(drag)) = ended
                        && let Some(target) = target
                    {
                        let destination = match target.kind {
                            TargetKind::Folder(path) => Some(path),
                            TargetKind::Tab(tab_id) => up_browser.read_untracked(|state| {
                                state
                                    .tabs
                                    .iter()
                                    .find(|tab| tab.id == tab_id)
                                    .and_then(|tab| tab.navigation.path().map(Path::to_path_buf))
                            }),
                        };
                        if let Some(destination) = destination {
                            let copy = ev.modifiers.control();
                            let mode = if copy {
                                transfer::Mode::Copy
                            } else {
                                transfer::Mode::Move
                            };
                            let paths = drag.paths();
                            let invalid_self_target = paths
                                .iter()
                                .any(|src| destination == *src || destination.starts_with(src));
                            let same_dir = mode == transfer::Mode::Move
                                && paths.iter().all(|p| p.parent() == Some(destination.as_path()));
                            if !paths.is_empty() && !same_dir && !invalid_self_target {
                                let job = PasteJob::new(destination, mode, paths);
                                up_browser.inspect_paste(job, false, true, None);
                            }
                        }
                        ev.stop_propagation();
                    }
                },
                on:pointer_cancel = move |ev| {
                    ev.release_pointer();
                    clear_active_drag_sources(&cancel_browser);
                    cancel_coord.cancel_all();
                },
            ) {
                {view}
            }
        })
    }

    pub(super) fn mouse_navigation(
        &self,
        event: &mut EventCx<'_, zgui::view::events::PointerDown>,
    ) {
        match event.button {
            Some(PointerButton::Back) => {
                self.history(false);
                event.prevent_default();
                event.stop_propagation();
            }
            Some(PointerButton::Forward) => {
                self.history(true);
                event.prevent_default();
                event.stop_propagation();
            }
            _ => {}
        }
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn materialize_file_drag(browser: &Browser, tab_id: u64, entry: &Entry) -> Option<FileDrag> {
    browser.read_untracked(|state| {
        if state.picker.is_some() {
            return None;
        }
        let tab = state.tabs.iter().find(|tab| tab.id == tab_id)?;
        if !matches!(tab.contents, Contents::Directory | Contents::Tags { .. }) {
            return None;
        }
        let entries: Arc<[Entry]> = tab
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::File(item) => Some(item.clone()),
                _ => None,
            })
            .collect();
        let selected_paths: Selection<PathBuf> = tab
            .navigation
            .view()
            .selection
            .iter()
            .filter_map(|key| match key {
                super::model::RowKey::Local(path) => Some(path.clone()),
                _ => None,
            })
            .collect();
        let snapshot = DragSnapshot::new(&entries, &selected_paths);
        Some(snapshot.drag_for_row(entry, selected_paths.contains(&entry.path)))
    })
}

fn update_file_drag_hover(browser: &Browser, coord: &Rc<DragCoordinator>, pos: (f32, f32)) {
    let excluded = browser.drag_sources().get_untracked();
    let target = coord.hit_target_excluding(pos, &excluded);
    let next_key = target.as_ref().map(|t| t.key.clone());
    let prev_key = coord.overlay.hovered_target().get_untracked();
    if next_key == prev_key {
        return;
    }
    coord.overlay.set_hovered_target(next_key.clone());
    coord.clear_spring();
    let Some(target) = target else {
        return;
    };
    let already_open = match &target.kind {
        TargetKind::Folder(dest) => {
            browser.read_untracked(|state| state.tab().navigation.path() == Some(dest.as_path()))
        }
        TargetKind::Tab(tab_id) => browser.read_untracked(|state| state.tab().id == *tab_id),
    };
    if already_open {
        return;
    }
    let Some(timers) = Timers::current() else {
        return;
    };
    let weak = browser.downgrade();
    let spring_coord = Rc::downgrade(coord);
    let expected_key = target.key.clone();
    let kind = target.kind;
    let handle = timers.set_timeout(SPRING_LOAD_DELAY, move || {
        let Some(browser) = weak.upgrade() else {
            return;
        };
        let Some(coord) = spring_coord.upgrade() else {
            return;
        };
        if coord.overlay.hovered_target().get_untracked().as_deref() != Some(expected_key.as_str())
        {
            return;
        }
        match kind {
            TargetKind::Folder(dest) => {
                let location = browser.local_location(&dest);
                browser.open(location);
            }
            TargetKind::Tab(tab_id) => {
                if let Some(index) = browser.tab_index(tab_id) {
                    browser.select_tab(index);
                }
            }
        }
    });
    *coord.spring.borrow_mut() = Some(SpringLoad { _timer: handle });
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use super::{ACTIVE_DRAG_SOURCES, DragSnapshot, Entry, FileDrag};
    use crate::{
        entities::file::directory::{EntryDetails, EntryKind},
        shared::lib::selection::Selection,
    };

    fn sample_entry(index: usize) -> Entry {
        let name = format!("file-{index:04}.txt");
        Entry::new(
            PathBuf::from(format!("/tmp/drag-test/{name}")),
            name.into(),
            EntryKind::File,
            Some(128),
            None,
            EntryDetails::default(),
        )
    }

    #[test]
    fn drag_for_row_defers_selection_materialization_until_drag_starts() {
        let rows: Arc<[Entry]> = (0..5_000).map(sample_entry).collect::<Vec<_>>().into();
        let selected: Selection<PathBuf> = rows[..1_000]
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        let snapshot = DragSnapshot::new(&rows, &selected);

        let selected_drags: Vec<FileDrag> = rows[..30]
            .iter()
            .map(|entry| snapshot.drag_for_row(entry, true))
            .collect();
        let unselected_drags: Vec<FileDrag> = rows[1_000..1_030]
            .iter()
            .map(|entry| snapshot.drag_for(entry, &selected))
            .collect();
        assert!(!snapshot.is_materialized());

        assert_eq!(unselected_drags[0].paths(), vec![rows[1_000].path.clone()]);
        assert_eq!(unselected_drags[0].count(), 1);
        assert!(!snapshot.is_materialized());

        let expected_selected: Vec<PathBuf> = rows[..1_000]
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        assert_eq!(selected_drags[0].paths(), expected_selected);
        assert!(snapshot.is_materialized());
        assert_eq!(selected_drags[15].count(), 1_000);
    }

    #[test]
    fn active_drag_sources_track_and_clear_ghosted_paths() {
        let alpha = PathBuf::from("/tmp/drag-source-alpha.txt");
        let beta = PathBuf::from("/tmp/drag-source-beta.txt");
        let key = 0xfeed_usize;

        assert!(!FileDrag::is_source(&alpha));
        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow_mut()
                .insert(key, HashSet::from([alpha.clone()]));
        });
        assert!(FileDrag::is_source(Path::new("/tmp/drag-source-alpha.txt")));
        assert!(!FileDrag::is_source(&beta));

        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow_mut().remove(&key);
        });
        assert!(!FileDrag::is_source(&alpha));
    }
}
