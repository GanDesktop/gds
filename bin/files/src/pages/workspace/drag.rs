//! Native tab reordering, pointer/OS drag-and-drop, spring-loading, and lazy file drag snapshots.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use gds::appearance::tokens::{
    COLLAPSED_SIDEBAR_WIDTH, ICON_BUTTON_SIZE, ICON_SIZE, SIDEBAR_ICON_SIZE, SIDEBAR_ITEM_HEIGHT,
    SIDEBAR_PADDING,
};
use gds::ui::{
    IconButtonProps, IconName, IconProps,
    drag::{
        DRAG_THRESHOLD, DragCardProps, DragCardSurfaceProps, DragOverlayState, ExternalFileDrop,
        FileDropOptions, FileDropRejection, FileDropTarget, drop_hit_test, file_drop_target_attrs,
        render_drag_card_icon,
    },
    navigation::{
        CornerActionProps, CornerButtonProps, NavItemContentProps, NavItemDetailsProps,
        NavItemHeaderProps, NavItemLabelProps, NavItemProps,
    },
    sidebar::SidebarPresentation,
};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::{
    EventCx, ListenerGuard, ListenerOptions, OverlayLayer, Portal, TimeoutHandle, Timers, events,
};
use zgui::vocab::{Key, NamedKey, PointerButton};
use zgui::{component, css, view};

use super::{
    chrome::{ITEM_GAP, TAB_HEIGHT},
    geometry::Grid,
    model::{Contents, Row},
    ui::{Browser, WeakBrowser},
};
use crate::{
    entities::file::{
        directory::{Entry, EntryKind},
        ui::FileIcon,
    },
    features::file_operations::{paste::PasteJob, transfer},
    shared::{
        i18n,
        lib::{clipboard, selection::Selection},
    },
};

const PREVIEW_VISIBLE_ITEMS: usize = 3;
const SPRING_LOAD_DELAY: Duration = Duration::from_millis(600);
/// Maximum elapsed time between Wayland `wl_data_device.leave` on a rejected Files drop target and
/// the paired `wl_data_source.cancelled` emitted on pointer release.
const REJECT_CANCEL_WINDOW: Duration = Duration::from_millis(250);
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
        display: block;
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

    .gds-files-drag-tab-ghost.gds-files-drag-tab-ghost {
        position: fixed;
        pointer-events: none;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        box-sizing: border-box;
        border: none;
        border-radius: var(--gds-radius-control, 6px);
        background: var(--gds-panel);
        color: var(--gds-sidebar-foreground);
        --gds-nav-label-ink: var(--gds-sidebar-foreground);
        outline: 1px solid var(--gds-border);
        outline-offset: -1px;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.24);
        overflow: hidden;
        user-select: none;
    }

    .gds-files-drag-tab-ghost.gds-files-drag-tab-ghost[data-selected="true"] {
        background: var(--gds-selection);
        color: var(--gds-selection-foreground);
        --gds-nav-label-ink: var(--gds-selection-foreground);
        outline: none;
    }

    .gds-files-drag-tab-ghost .gds-nav-item,
    .gds-files-drag-tab-ghost .gds-files-tab-action,
    .gds-files-drag-tab-ghost .gds-corner-action,
    .gds-files-drag-tab-ghost .zui-button {
        pointer-events: none;
    }

    .gds-files-sidebar-row-wrap[data-dragging-tab="true"],
    .gds-files-sidebar-row-wrap[data-dragging-quick-access="true"] {
        opacity: 0.45;
    }

    .gds-files-quick-access-drop-zone {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        border-radius: var(--gds-radius-control, 6px);
        transition: background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-quick-access-drop-zone[data-quick-access-drop="true"] {
        background-color: color-mix(in srgb, var(--gds-primary) 10%, transparent);
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

    /// Returns `true` when `destination` equals or lies inside any active drag source in this
    /// process (across all open Files windows).
    pub(super) fn is_source_or_descendant(destination: &Path) -> bool {
        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow().values().any(|sources| {
                sources
                    .iter()
                    .any(|src| destination == src || destination.starts_with(src))
            })
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
struct TabDragPreview {
    id: u64,
    pinned: bool,
    selected: bool,
    icon: IconName,
    title: String,
    detail_path: String,
    grab_offset: (f32, f32),
}

const QUICK_ACCESS_ZONE_KEY: &str = "quick-access-zone";

#[derive(Clone)]
struct QuickAccessDragPreview {
    path: PathBuf,
    icon: IconName,
    label: String,
    selected: bool,
    grab_offset: (f32, f32),
}

#[derive(Clone)]
enum WindowDragPayload {
    Files(FileDrag),
    Tab(TabDragPreview),
    QuickAccess(QuickAccessDragPreview),
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
        grab_offset: (f32, f32),
    },
    QuickAccess {
        path: PathBuf,
        icon: IconName,
        label: String,
        start: (f32, f32),
        grab_offset: (f32, f32),
    },
}

#[derive(Clone)]
enum TargetKind {
    Folder(PathBuf),
    Tab(u64),
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragRejectReason {
    IntoSelf,
    UnsupportedDestination,
}

impl DragRejectReason {
    fn message(self) -> String {
        match self {
            Self::IntoSelf => i18n::text_untracked("drop-into-self"),
            Self::UnsupportedDestination => i18n::text_untracked("drop-unsupported-destination"),
        }
    }
}

struct PendingDragRejection {
    reason: DragRejectReason,
    target_window: usize,
    target: Option<WeakBrowser>,
    left_at: Option<Instant>,
}

impl PendingDragRejection {
    fn is_active_at(&self, now: Instant) -> bool {
        self.left_at
            .is_none_or(|left| now.saturating_duration_since(left) <= REJECT_CANCEL_WINDOW)
    }
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

#[derive(Clone)]
struct RegisteredQuickAccess {
    token: u64,
    path: PathBuf,
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
    quick_access_zones: RefCell<Vec<(u64, NodeRef)>>,
    quick_access_rows: RefCell<Vec<RegisteredQuickAccess>>,
    spring: RefCell<Option<SpringLoad>>,
    external_spring_key: RefCell<Option<String>>,
    native_drag_session: Cell<Option<DragSessionId>>,
    native_drag_paths: RefCell<Vec<PathBuf>>,
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
            quick_access_zones: RefCell::new(Vec::new()),
            quick_access_rows: RefCell::new(Vec::new()),
            spring: RefCell::new(None),
            external_spring_key: RefCell::new(None),
            native_drag_session: Cell::new(None),
            native_drag_paths: RefCell::new(Vec::new()),
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

    fn register_quick_access_zone(&self, node: NodeRef) -> u64 {
        let token = self.alloc_token();
        self.quick_access_zones.borrow_mut().push((token, node));
        token
    }

    fn unregister_quick_access_zone(&self, token: u64) {
        self.quick_access_zones
            .borrow_mut()
            .retain(|(item_token, _)| *item_token != token);
    }

    fn hit_quick_access_zone(&self, pos: (f32, f32)) -> bool {
        self.quick_access_zones
            .borrow()
            .iter()
            .rev()
            .any(|(_, node)| drop_hit_test(node, pos))
    }

    fn register_quick_access_row(&self, path: PathBuf, node: NodeRef) -> u64 {
        let token = self.alloc_token();
        let mut rows = self.quick_access_rows.borrow_mut();
        rows.retain(|item| item.path != path);
        rows.push(RegisteredQuickAccess { token, path, node });
        token
    }

    fn unregister_quick_access_row(&self, token: u64) {
        self.quick_access_rows
            .borrow_mut()
            .retain(|item| item.token != token);
    }

    fn hit_quick_access_row(&self, pos: (f32, f32)) -> Option<RegisteredQuickAccess> {
        self.quick_access_rows
            .borrow()
            .iter()
            .find(|item| drop_hit_test(&item.node, pos))
            .cloned()
    }

    fn hit_hover_target(
        &self,
        pos: (f32, f32),
        excluded_sources: &HashSet<PathBuf>,
    ) -> Option<RegisteredTarget> {
        self.targets
            .borrow()
            .iter()
            .rev()
            .find(|item| {
                match &item.kind {
                    TargetKind::Unsupported => return false,
                    TargetKind::Folder(dest)
                        if excluded_sources
                            .iter()
                            .any(|src| dest == src || dest.starts_with(src)) =>
                    {
                        return false;
                    }
                    _ => {}
                }
                drop_hit_test(&item.node, pos)
            })
            .cloned()
    }

    fn hit_drop_target(&self, pos: (f32, f32)) -> Option<RegisteredTarget> {
        self.targets
            .borrow()
            .iter()
            .rev()
            .find(|item| drop_hit_test(&item.node, pos))
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

    fn clear_external_spring(&self) {
        *self.external_spring_key.borrow_mut() = None;
        self.clear_spring();
    }

    fn cancel_all(&self) {
        *self.armed.borrow_mut() = None;
        self.clear_external_spring();
        self.overlay.cancel();
    }
}

thread_local! {
    static COORDINATORS: RefCell<HashMap<usize, Rc<DragCoordinator>>> = RefCell::new(HashMap::new());
    static CLEANUP_REGISTERED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    static ACTIVE_DRAG_SOURCES: RefCell<HashMap<usize, HashSet<PathBuf>>> = RefCell::new(HashMap::new());
    static PENDING_DRAG_REJECTION: RefCell<Option<PendingDragRejection>> = const { RefCell::new(None) };
    static ROW_PRESSED_WINDOW: Cell<usize> = const { Cell::new(0) };
}

fn has_active_drag_sources() -> bool {
    ACTIVE_DRAG_SOURCES.with(|slot| slot.borrow().values().any(|sources| !sources.is_empty()))
}

fn record_pending_drag_rejection(browser: &Browser, reason: DragRejectReason) {
    if !has_active_drag_sources() {
        return;
    }
    let target_window = Rc::as_ptr(&browser.0) as usize;
    let target = Some(browser.downgrade());
    PENDING_DRAG_REJECTION.with(|slot| {
        *slot.borrow_mut() = Some(PendingDragRejection {
            reason,
            target_window,
            target,
            left_at: None,
        });
    });
}

fn clear_pending_drag_rejection() {
    PENDING_DRAG_REJECTION.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

fn clear_pending_drag_rejection_if_left() {
    PENDING_DRAG_REJECTION.with(|slot| {
        let mut pending = slot.borrow_mut();
        if pending.as_ref().is_some_and(|item| item.left_at.is_some()) {
            *pending = None;
        }
    });
}

fn clear_pending_drag_rejection_for_window(window_key: usize) {
    PENDING_DRAG_REJECTION.with(|slot| {
        let mut pending = slot.borrow_mut();
        if pending
            .as_ref()
            .is_some_and(|item| item.target_window == window_key)
        {
            *pending = None;
        }
    });
}

fn mark_pending_drag_rejection_left(window_key: usize, now: Instant) {
    PENDING_DRAG_REJECTION.with(|slot| {
        if let Some(pending) = slot.borrow_mut().as_mut()
            && pending.target_window == window_key
            && pending.left_at.is_none()
        {
            pending.left_at = Some(now);
        }
    });
}

fn take_active_drag_rejection(now: Instant) -> Option<(DragRejectReason, Option<WeakBrowser>)> {
    PENDING_DRAG_REJECTION.with(|slot| {
        let pending = slot.borrow_mut().take()?;
        pending
            .is_active_at(now)
            .then_some((pending.reason, pending.target))
    })
}

fn report_drag_rejection(
    fallback: &Browser,
    reason: DragRejectReason,
    target: Option<WeakBrowser>,
) {
    let notice = reason.message();
    if let Some(target_browser) = target.and_then(|weak| weak.upgrade()) {
        target_browser.report_notice(notice);
    } else {
        fallback.report_notice(notice);
    }
}

fn background_rejection(browser: &Browser) -> Option<DragRejectReason> {
    browser.read_untracked(|state| {
        let tab = state.tab();
        if !matches!(tab.contents, Contents::Directory) {
            return Some(DragRejectReason::UnsupportedDestination);
        }
        match tab.navigation.path() {
            Some(dest) if FileDrag::is_source_or_descendant(dest) => {
                Some(DragRejectReason::IntoSelf)
            }
            Some(_) => None,
            None => Some(DragRejectReason::UnsupportedDestination),
        }
    })
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
        let native_weak = browser.downgrade();
        let native_coord = Rc::clone(&coord);
        let native_window = browser.0.window.clone();
        browser.0.owner.with(|| {
            let drag_source_guard = native_window.map(|window| {
                window.on_drag_source(move |event: DragSourceEvent| {
                    if native_coord.native_drag_session.get() != Some(event.session()) {
                        return;
                    }
                    match event {
                        DragSourceEvent::TargetMime {
                            mime_type: Some(_), ..
                        } => {
                            clear_pending_drag_rejection_if_left();
                            return;
                        }
                        DragSourceEvent::Dropped { .. } => {
                            clear_pending_drag_rejection();
                            return;
                        }
                        DragSourceEvent::TargetMime {
                            mime_type: None, ..
                        }
                        | DragSourceEvent::ActionChanged { .. } => return,
                        DragSourceEvent::Finished { .. } | DragSourceEvent::Cancelled { .. } => {}
                        _ => return,
                    }
                    native_coord.native_drag_session.set(None);
                    let dragged_paths =
                        std::mem::take(&mut *native_coord.native_drag_paths.borrow_mut());
                    let rejected = if matches!(event, DragSourceEvent::Cancelled { .. }) {
                        take_active_drag_rejection(Instant::now())
                    } else {
                        clear_pending_drag_rejection();
                        None
                    };
                    if let Some(browser) = native_weak.upgrade() {
                        clear_active_drag_sources(&browser);
                        // Do not unlink source paths on Move finish: for local `text/uri-list`,
                        // the destination file manager (GDS, Nautilus, Dolphin) already moved them.
                        if matches!(
                            event,
                            DragSourceEvent::Finished {
                                action: Some(DragAction::Move),
                                ..
                            }
                        ) && !dragged_paths.is_empty()
                        {
                            browser.refresh_affected(&dragged_paths);
                        }
                        if let Some((reason, target)) = rejected {
                            report_drag_rejection(&browser, reason, target);
                        }
                    }
                })
            });
            let root_guards: Rc<RefCell<Vec<ListenerGuard>>> = Rc::new(RefCell::new(Vec::new()));
            let root_guards_watch = Rc::clone(&root_guards);
            let attach_fx = RenderEffect::new(move |_| {
                if root_node.get().is_none() || !root_guards_watch.borrow().is_empty() {
                    return;
                }
                let weak_inner = weak.clone();
                let coord_inner = Rc::clone(&esc_coord);
                let mut attached = Vec::with_capacity(3);
                if let Some(guard) = root_node.listen(
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
                ) {
                    attached.push(guard);
                }
                // Clear any previous drop-target rejection at the start of every drag-over pass;
                // a rejecting drop target re-records its reason during target/bubble dispatch.
                if let Some(guard) = root_node.listen(
                    events::DRAG_OVER,
                    ListenerOptions::CAPTURE,
                    move |_: &mut EventCx<'_, events::DragOver>| {
                        clear_pending_drag_rejection();
                    },
                ) {
                    attached.push(guard);
                }
                if let Some(guard) = root_node.listen(
                    events::DRAG_LEAVE,
                    ListenerOptions::DEFAULT,
                    move |_: &mut EventCx<'_, events::DragLeave>| {
                        mark_pending_drag_rejection_left(window_key, Instant::now());
                    },
                ) {
                    attached.push(guard);
                }
                *root_guards_watch.borrow_mut() = attached;
            });

            on_cleanup_local(move || {
                drop(attach_fx);
                drop(drag_source_guard);
                root_guards.borrow_mut().clear();
                clear_pending_drag_rejection_for_window(window_key);
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

fn render_file_drag_card_body(
    count: usize,
    items: Vec<DragItem>,
    copy_hint: Option<String>,
) -> AnyView {
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
    }
    if count <= 1 {
        let (icon, label) = items
            .into_iter()
            .next()
            .map(|item| (item.icon.base(), item.name))
            .unwrap_or_else(|| (IconName::File, gds::i18n::count("item-count", 1)));
        AnyView::new(view! {
            column(class = "gds-drag-card__content") {
                row(class = "gds-files-drag-item") {
                    Icon(name = icon, size = ICON_SIZE, class = "gds-files-drag-item__icon")
                    text(class = "gds-files-drag-item__name") {{label}}
                }
            }
            {copy_hint.map(|hint| view! {
                row(class = "gds-drag-card__footer") {{hint}}
            })}
        })
    } else {
        let footer_text = if count > PREVIEW_VISIBLE_ITEMS {
            Some("…".to_owned())
        } else {
            copy_hint
        };
        let rows = items
            .into_iter()
            .take(PREVIEW_VISIBLE_ITEMS)
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
        })
    }
}

#[component]
fn DraggedTabGhost(
    preview: TabDragPreview,
    position: Signal<(f32, f32), LocalStorage>,
    presentation: Signal<SidebarPresentation, LocalStorage>,
) -> impl IntoView {
    gds::ui::drag::install_drag_stylesheet();
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
    }
    let collapsed_width = (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING).max(36.0);
    let item_width = Signal::derive_local(move || {
        (presentation.get().width() - 2.0 * SIDEBAR_PADDING).max(collapsed_width)
    });
    let actions_visible = Signal::derive_local(move || presentation.get().actions_visible());
    let collapsed_rail = Signal::derive_local(move || presentation.get().is_collapsed());
    let grab_x = preview.grab_offset.0;
    let grab_y = preview.grab_offset.1;
    let ghost_ref = NodeRef::new();
    #[cfg(test)]
    tests::record_tab_ghost_node(ghost_ref);

    Portal::new(move || {
        let icon = preview.icon;
        let title = preview.title.clone();
        let detail_path = preview.detail_path.clone();
        let pinned = preview.pinned;
        let selected = preview.selected;
        AnyView::new(view! {
            box(class = "gds-drag-portal-overlay", a11y:hidden = true) {
                box(
                    node_ref = ghost_ref,
                    class = "gds-files-drag-tab-ghost",
                    attr:data-selected = selected.then(|| "true".to_owned()),
                    style:left = move || {
                        let width = item_width.get();
                        Some(format!("{:.2}px", position.get().0 - grab_x.min(width)))
                    },
                    style:top = move || {
                        Some(format!("{:.2}px", position.get().1 - grab_y.min(TAB_HEIGHT)))
                    },
                    style:width = move || Some(format!("{:.2}px", item_width.get())),
                    style:height = format!("{TAB_HEIGHT}px"),
                ) {
                    NavItem(
                        selected = selected,
                        presentation = presentation,
                        class = "gds-files-tab-item",
                        style:height = format!("{TAB_HEIGHT}px"),
                        var:--gds-files-tab-reserve = move || {
                            Some(format!(
                                "{:.2}px",
                                presentation.get().interpolate(ICON_BUTTON_SIZE + ITEM_GAP, 0.0)
                            ))
                        },
                    ) {
                        Icon(
                            name = icon,
                            size = SIDEBAR_ICON_SIZE,
                            class = "gds-files-nav-icon",
                        )
                        NavItemContent {
                            NavItemHeader {
                                NavItemLabel {{title}}
                            }
                            NavItemDetails {
                                {detail_path}
                            }
                        }
                        {pinned.then(|| AnyView::new(view! {
                            box(
                                class = "gds-files-tab-action",
                                attr:data-revealed = move || {
                                    actions_visible.get().then(|| "true".to_owned())
                                },
                                a11y:hidden = true,
                            ) {
                                IconButton(
                                    icon = IconName::Pin,
                                    label = Signal::stored_local(String::new()),
                                )
                            }
                            CornerAction(
                                revealed = collapsed_rail,
                                button = AnyView::new(view! {
                                    CornerButton(
                                        icon = IconName::Pin,
                                        label = Signal::stored_local(String::new()),
                                    )
                                }),
                            ) {}
                        }))}
                    }
                }
            }
        })
    })
    .layer(OverlayLayer::Popover)
}

#[component]
fn DraggedQuickAccessGhost(
    preview: QuickAccessDragPreview,
    position: Signal<(f32, f32), LocalStorage>,
    presentation: Signal<SidebarPresentation, LocalStorage>,
) -> impl IntoView {
    gds::ui::drag::install_drag_stylesheet();
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
    }
    let collapsed_width = (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING).max(36.0);
    let item_width = Signal::derive_local(move || {
        (presentation.get().width() - 2.0 * SIDEBAR_PADDING).max(collapsed_width)
    });
    let grab_x = preview.grab_offset.0;
    let grab_y = preview.grab_offset.1;

    Portal::new(move || {
        let icon = preview.icon;
        let label = preview.label.clone();
        let selected = preview.selected;
        AnyView::new(view! {
            box(class = "gds-drag-portal-overlay", a11y:hidden = true) {
                box(
                    class = "gds-files-drag-tab-ghost",
                    attr:data-selected = selected.then(|| "true".to_owned()),
                    style:left = move || {
                        let width = item_width.get();
                        Some(format!("{:.2}px", position.get().0 - grab_x.min(width)))
                    },
                    style:top = move || {
                        Some(format!("{:.2}px", position.get().1 - grab_y.min(SIDEBAR_ITEM_HEIGHT)))
                    },
                    style:width = move || Some(format!("{:.2}px", item_width.get())),
                    style:height = format!("{SIDEBAR_ITEM_HEIGHT}px"),
                ) {
                    NavItem(
                        selected = selected,
                        presentation = presentation,
                        class = "gds-files-location-item",
                        style:height = format!("{SIDEBAR_ITEM_HEIGHT}px"),
                    ) {
                        Icon(
                            name = icon,
                            size = SIDEBAR_ICON_SIZE,
                            class = "gds-files-nav-icon",
                        )
                        NavItemLabel {{label}}
                    }
                }
            }
        })
    })
    .layer(OverlayLayer::Popover)
}

impl Browser {
    pub(super) fn has_native_drag_active(&self) -> bool {
        let key = Rc::as_ptr(&self.0) as usize;
        COORDINATORS.with(|slot| {
            slot.borrow()
                .get(&key)
                .is_some_and(|coord| coord.native_drag_session.get().is_some())
        })
    }

    pub(super) fn background_drop_target(&self, view: AnyView) -> AnyView {
        if zgui::view::current_host().is_some() {
            zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
        }
        let coord = coordinator_for(self);
        let can_accept_browser = self.clone();
        let can_accept = Rc::new(move || match background_rejection(&can_accept_browser) {
            None => true,
            Some(reason) => {
                record_pending_drag_rejection(&can_accept_browser, reason);
                false
            }
        });
        let drop_browser = self.clone();
        let drop_coord = Rc::clone(&coord);
        let on_external_drop = UnsyncCallback::new(move |drop: ExternalFileDrop| {
            drop_coord.clear_external_spring();
            clear_pending_drag_rejection();
            let destination = drop_browser.read_untracked(|state| {
                let tab = state.tab();
                matches!(tab.contents, Contents::Directory)
                    .then(|| tab.navigation.path().map(Path::to_path_buf))
                    .flatten()
            });
            if let Some(destination) = destination {
                drop_browser.handle_external_file_drop(destination, drop);
            } else {
                if let Some(handle) = drop.handle {
                    handle.reject();
                }
                drop_browser.report_notice(DragRejectReason::UnsupportedDestination.message());
            }
        });
        let reject_browser = self.clone();
        let reject_coord = Rc::clone(&coord);
        let on_reject = UnsyncCallback::new(move |rejection: FileDropRejection| {
            reject_coord.clear_external_spring();
            clear_pending_drag_rejection();
            match rejection {
                FileDropRejection::Disallowed => {
                    let reason = background_rejection(&reject_browser)
                        .unwrap_or(DragRejectReason::UnsupportedDestination);
                    reject_browser.report_notice(reason.message());
                }
                FileDropRejection::EmptyPaths => {
                    reject_browser.report_notice(i18n::text_untracked("drop-no-local-files"));
                }
            }
        });
        let drop_attrs = file_drop_target_attrs(FileDropTarget {
            active: Signal::stored_local(false),
            options: FileDropOptions::copy_or_move(DragAction::Move),
            can_accept: Some(can_accept),
            on_hover_change: None,
            on_drop: Some(on_external_drop),
            on_reject: Some(on_reject),
        });

        AnyView::new(view! {
            box(class = "gds-files-drop-background", {..drop_attrs}) {
                {view}
            }
        })
    }

    /// Window-level floating preview portal for in-window file and tab drags.
    ///
    /// Mounted once on the Files workspace pane so tab previews stay visible on non-directory views
    /// (Recent, Favorites, Trash, Network) and survive spring-loaded folder switches.
    pub(super) fn drag_overlay(&self) -> AnyView {
        if zgui::view::current_host().is_some() {
            zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
        }
        let coord = coordinator_for(self);
        let overlay = coord.overlay;
        let sidebar_presentation = self.sidebar_presentation();
        let copy_mode = Signal::derive_local(move || {
            let _ = overlay.position().get();
            coord.copy_modifier.get()
        });

        AnyView::new(view! {
            {{move || overlay.payload().get().map(|payload| {
                match payload {
                    WindowDragPayload::Files(drag) => {
                        let count = drag.count();
                        let items = drag.preview_items();
                        let copy_hint = copy_mode.get().then(|| i18n::text("copy-files"));
                        let body = render_file_drag_card_body(count, items, copy_hint);
                        AnyView::new(view! {
                            DragCard(position = overlay.position()) {{body}}
                        })
                    }
                    WindowDragPayload::Tab(preview) => AnyView::new(view! {
                        DraggedTabGhost(
                            preview = preview,
                            position = overlay.position(),
                            presentation = sidebar_presentation,
                        )
                    }),
                    WindowDragPayload::QuickAccess(preview) => AnyView::new(view! {
                        DraggedQuickAccessGhost(
                            preview = preview,
                            position = overlay.position(),
                            presentation = sidebar_presentation,
                        )
                    }),
                }
            })}}
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
        let hover_key = target_key.clone();
        let is_hovered = Signal::derive_local(move || {
            overlay.hovered_target().get().as_deref() == Some(&hover_key)
        });
        let is_dragging_self = Signal::derive_local(move || {
            overlay.payload().with(
                |payload| matches!(payload, Some(WindowDragPayload::Tab(tab)) if tab.id == id),
            )
        });

        let down_coord = Rc::clone(&coord);
        let down_presentation = self.sidebar_presentation();
        let move_coord = Rc::clone(&coord);
        let move_browser = self.clone();
        let up_coord = Rc::clone(&coord);
        let up_browser = self.clone();
        let cancel_coord = Rc::clone(&coord);
        let enter_browser = self.clone();
        let enter_coord = Rc::clone(&coord);
        let enter_key = target_key.clone();
        let leave_browser = self.clone();
        let leave_coord = Rc::clone(&coord);
        let leave_key = target_key;

        AnyView::new(view! {
            box(
                class = "gds-files-sidebar-row-wrap",
                node_ref = node,
                attr:data-drop-active = move || is_hovered.get().then(|| "true".to_owned()),
                attr:data-dragging-tab = move || is_dragging_self.get().then(|| "true".to_owned()),
                on:drag_enter = move |ev| {
                    if ev.offers_files() {
                        set_external_spring_target(
                            &enter_browser,
                            &enter_coord,
                            enter_key.clone(),
                            TargetKind::Tab(id),
                            true,
                        );
                    }
                },
                on:drag_leave = move |_| {
                    set_external_spring_target(
                        &leave_browser,
                        &leave_coord,
                        leave_key.clone(),
                        TargetKind::Tab(id),
                        false,
                    );
                },
                on:pointer_down = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    let pos = (ev.position.x.0, ev.position.y.0);
                    let grab_offset = tab_grab_offset(&node, pos, down_presentation.get_untracked());
                    *down_coord.armed.borrow_mut() = Some(ArmedDrag::Tab {
                        id,
                        pinned,
                        start: pos,
                        grab_offset,
                    });
                },
                on:pointer_move = move |ev| {
                    let pos = (ev.position.x.0, ev.position.y.0);
                    let mut start_payload = None;
                    if let Some(ArmedDrag::Tab {
                        id: armed_id,
                        pinned: armed_pinned,
                        start,
                        grab_offset,
                    }) = *move_coord.armed.borrow()
                        && armed_id == id
                        && distance(start, pos) >= DRAG_THRESHOLD
                    {
                        let (icon, title, detail_path, selected) = move_browser
                            .tab_drag_snapshot(id)
                            .unwrap_or_else(|| (IconName::Folder, String::new(), String::new(), false));
                        start_payload = Some(WindowDragPayload::Tab(TabDragPreview {
                            id,
                            pinned: armed_pinned,
                            selected,
                            icon,
                            title,
                            detail_path,
                            grab_offset,
                        }));
                    }
                    if let Some(payload) = start_payload {
                        *move_coord.armed.borrow_mut() = None;
                        ev.capture_pointer();
                        move_coord.overlay.begin(payload, pos.0, pos.1);
                    }
                    if matches!(move_coord.overlay.payload().get_untracked(), Some(WindowDragPayload::Tab(_))) {
                        move_coord.overlay.move_to(pos.0, pos.1);
                        let hovered = move_coord
                            .hit_tab(pos)
                            .filter(|tab| tab.pinned == pinned)
                            .map(|tab| format!("tab:{}", tab.id))
                            .or_else(|| {
                                (move_coord.hit_quick_access_zone(pos)
                                    && move_browser.tab_local_path(id).is_some())
                                .then(|| QUICK_ACCESS_ZONE_KEY.to_owned())
                            });
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
                        Some(WindowDragPayload::Tab(_))
                    );
                    if !is_tab_drag {
                        return;
                    }
                    ev.release_pointer();
                    let pos = (ev.position.x.0, ev.position.y.0);
                    if let Some(WindowDragPayload::Tab(TabDragPreview { id: from_id, .. })) =
                        up_coord.overlay.end()
                    {
                        if let Some(target_tab) = up_coord.hit_tab(pos)
                            && target_tab.id != from_id
                            && let (Some(from_idx), Some(to_idx)) = (
                                up_browser.tab_index(from_id),
                                up_browser.tab_index(target_tab.id),
                            )
                        {
                            up_browser.reorder_tab(from_idx, to_idx);
                            ev.stop_propagation();
                        } else if up_coord.hit_quick_access_zone(pos) {
                            up_browser.add_quick_access_from_tab(from_id);
                            ev.stop_propagation();
                        }
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

    pub(super) fn quick_access_drop_zone(&self, view: AnyView) -> AnyView {
        if zgui::view::current_host().is_some() {
            zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
        }
        let coord = coordinator_for(self);
        let node = NodeRef::new();
        let token = coord.register_quick_access_zone(node);
        let cleanup_coord = Rc::clone(&coord);
        on_cleanup_local(move || {
            cleanup_coord.unregister_quick_access_zone(token);
        });
        let overlay = coord.overlay;
        let active = Signal::derive_local(move || {
            overlay.hovered_target().get().as_deref() == Some(QUICK_ACCESS_ZONE_KEY)
        });

        AnyView::new(view! {
            column(
                node_ref = node,
                class = "gds-files-quick-access-drop-zone",
                attr:data-quick-access-drop = move || active.get().then(|| "true".to_owned()),
            ) {
                {view}
            }
        })
    }

    pub(super) fn draggable_quick_access(
        &self,
        view: AnyView,
        path: PathBuf,
        label: String,
        icon: IconName,
    ) -> AnyView {
        let coord = coordinator_for(self);
        let node = NodeRef::new();
        let row_key = format!("qa:{}", path.display());
        let row_token = coord.register_quick_access_row(path.clone(), node);
        let cleanup_coord = Rc::clone(&coord);
        on_cleanup_local(move || {
            cleanup_coord.unregister_quick_access_row(row_token);
        });

        let overlay = coord.overlay;
        let hover_key = row_key;
        let is_hovered = Signal::derive_local(move || {
            overlay.hovered_target().get().as_deref() == Some(&hover_key)
        });
        let dragging_path = path.clone();
        let is_dragging_self = Signal::derive_local(move || {
            overlay.payload().with(|payload| {
                matches!(
                    payload,
                    Some(WindowDragPayload::QuickAccess(item)) if item.path == dragging_path
                )
            })
        });

        let down_coord = Rc::clone(&coord);
        let down_path = path.clone();
        let down_label = label;
        let down_presentation = self.sidebar_presentation();
        let move_coord = Rc::clone(&coord);
        let move_path = path.clone();
        let move_browser = self.clone();
        let up_coord = Rc::clone(&coord);
        let up_path = path;
        let up_browser = self.clone();
        let cancel_coord = Rc::clone(&coord);

        AnyView::new(view! {
            box(
                class = "gds-files-sidebar-row-wrap",
                node_ref = node,
                attr:data-drop-active = move || is_hovered.get().then(|| "true".to_owned()),
                attr:data-dragging-quick-access = move || is_dragging_self.get().then(|| "true".to_owned()),
                on:pointer_down = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    let pos = (ev.position.x.0, ev.position.y.0);
                    let grab_offset = row_grab_offset(
                        &node,
                        pos,
                        down_presentation.get_untracked(),
                        SIDEBAR_ITEM_HEIGHT,
                    );
                    *down_coord.armed.borrow_mut() = Some(ArmedDrag::QuickAccess {
                        path: down_path.clone(),
                        icon,
                        label: down_label.clone(),
                        start: pos,
                        grab_offset,
                    });
                },
                on:pointer_move = move |ev| {
                    let pos = (ev.position.x.0, ev.position.y.0);
                    let mut start_payload = None;
                    if let Some(ArmedDrag::QuickAccess {
                        path: armed_path,
                        icon: armed_icon,
                        label: armed_label,
                        start,
                        grab_offset,
                    }) = move_coord.armed.borrow().as_ref()
                        && *armed_path == move_path
                        && distance(*start, pos) >= DRAG_THRESHOLD
                    {
                        let selected = move_browser
                            .read_untracked(|state| state.tab().navigation.path() == Some(armed_path.as_path()));
                        start_payload = Some(WindowDragPayload::QuickAccess(QuickAccessDragPreview {
                            path: armed_path.clone(),
                            icon: *armed_icon,
                            label: armed_label.clone(),
                            selected,
                            grab_offset: *grab_offset,
                        }));
                    }
                    if let Some(payload) = start_payload {
                        *move_coord.armed.borrow_mut() = None;
                        ev.capture_pointer();
                        move_coord.overlay.begin(payload, pos.0, pos.1);
                    }
                    if matches!(
                        move_coord.overlay.payload().get_untracked(),
                        Some(WindowDragPayload::QuickAccess(_))
                    ) {
                        move_coord.overlay.move_to(pos.0, pos.1);
                        let hovered = move_coord
                            .hit_quick_access_row(pos)
                            .map(|row| format!("qa:{}", row.path.display()));
                        move_coord.overlay.set_hovered_target(hovered);
                    }
                },
                on:pointer_up = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    if matches!(
                        up_coord.armed.borrow().as_ref(),
                        Some(ArmedDrag::QuickAccess { path: armed_path, .. }) if *armed_path == up_path
                    ) {
                        *up_coord.armed.borrow_mut() = None;
                    }
                    let is_qa_drag = matches!(
                        up_coord.overlay.payload().get_untracked(),
                        Some(WindowDragPayload::QuickAccess(_))
                    );
                    if !is_qa_drag {
                        return;
                    }
                    ev.release_pointer();
                    let pos = (ev.position.x.0, ev.position.y.0);
                    if let Some(WindowDragPayload::QuickAccess(QuickAccessDragPreview {
                        path: from_path,
                        ..
                    })) = up_coord.overlay.end()
                    {
                        if let Some(target) = up_coord.hit_quick_access_row(pos)
                            && target.path != from_path
                        {
                            up_browser.reorder_quick_access_item(&from_path, &target.path);
                        }
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
        let instance = coord.alloc_token();
        let target_key = format!("dir:{instance}:{}", destination.display());
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
        let hover_key = target_key.clone();
        let internal_active = Signal::derive_local(move || {
            overlay.hovered_target().get().as_deref() == Some(&hover_key)
        });

        let gate_browser = self.clone();
        let gate_dest = destination.clone();
        let can_accept = Rc::new(move || {
            if FileDrag::is_source_or_descendant(&gate_dest) {
                record_pending_drag_rejection(&gate_browser, DragRejectReason::IntoSelf);
                false
            } else {
                true
            }
        });
        let spring_browser = self.clone();
        let spring_coord = Rc::clone(&coord);
        let spring_dest = destination.clone();
        let on_hover_change = UnsyncCallback::new(move |hovering: bool| {
            set_external_spring_target(
                &spring_browser,
                &spring_coord,
                target_key.clone(),
                TargetKind::Folder(spring_dest.clone()),
                hovering,
            );
        });
        let drop_browser = self.clone();
        let drop_coord = Rc::clone(&coord);
        let drop_dest = destination;
        let on_external_drop = UnsyncCallback::new(move |drop: ExternalFileDrop| {
            drop_coord.clear_external_spring();
            clear_pending_drag_rejection();
            drop_browser.handle_external_file_drop(drop_dest.clone(), drop);
        });
        let reject_browser = self.clone();
        let reject_coord = Rc::clone(&coord);
        let on_reject = UnsyncCallback::new(move |rejection: FileDropRejection| {
            reject_coord.clear_external_spring();
            clear_pending_drag_rejection();
            match rejection {
                FileDropRejection::Disallowed => {
                    reject_browser.report_notice(DragRejectReason::IntoSelf.message());
                }
                FileDropRejection::EmptyPaths => {
                    reject_browser.report_notice(i18n::text_untracked("drop-no-local-files"));
                }
            }
        });
        let attrs = file_drop_target_attrs(FileDropTarget {
            active: internal_active,
            options: FileDropOptions::copy_or_move(DragAction::Move),
            can_accept: Some(can_accept),
            on_hover_change: Some(on_hover_change),
            on_drop: Some(on_external_drop),
            on_reject: Some(on_reject),
        });

        AnyView::new(view! {
            box(class = "gds-files-sidebar-row-wrap", node_ref = node, {..attrs}) {
                {view}
            }
        })
    }

    pub(super) fn unsupported_file_drop_target(&self, view: AnyView) -> AnyView {
        let coord = coordinator_for(self);
        let node = NodeRef::new();
        let target_key = format!("unsupported:{}", coord.alloc_token());
        let target_token = coord.register_target(target_key, node, TargetKind::Unsupported);
        let cleanup_coord = Rc::clone(&coord);
        on_cleanup_local(move || {
            cleanup_coord.unregister_target(target_token);
        });

        let gate_browser = self.clone();
        let can_accept = Rc::new(move || {
            record_pending_drag_rejection(&gate_browser, DragRejectReason::UnsupportedDestination);
            false
        });
        let reject_browser = self.clone();
        let reject_coord = Rc::clone(&coord);
        let on_reject = UnsyncCallback::new(move |rejection: FileDropRejection| {
            reject_coord.clear_external_spring();
            clear_pending_drag_rejection();
            match rejection {
                FileDropRejection::Disallowed => {
                    reject_browser
                        .report_notice(DragRejectReason::UnsupportedDestination.message());
                }
                FileDropRejection::EmptyPaths => {
                    reject_browser.report_notice(i18n::text_untracked("drop-no-local-files"));
                }
            }
        });
        let attrs = file_drop_target_attrs(FileDropTarget {
            active: Signal::stored_local(false),
            options: FileDropOptions::copy_or_move(DragAction::Move),
            can_accept: Some(can_accept),
            on_hover_change: None,
            on_drop: None,
            on_reject: Some(on_reject),
        });

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
            let hover_key = target_key.clone();
            let internal_active = Signal::derive_local(move || {
                overlay.hovered_target().get().as_deref() == Some(&hover_key)
            });
            let gate_browser = self.clone();
            let gate_dest = dest.clone();
            let can_accept = Rc::new(move || {
                if FileDrag::is_source_or_descendant(&gate_dest) {
                    record_pending_drag_rejection(&gate_browser, DragRejectReason::IntoSelf);
                    false
                } else {
                    true
                }
            });
            let spring_browser = self.clone();
            let spring_coord = Rc::clone(&coord);
            let spring_dest = dest.clone();
            let on_hover_change = UnsyncCallback::new(move |hovering: bool| {
                set_external_spring_target(
                    &spring_browser,
                    &spring_coord,
                    target_key.clone(),
                    TargetKind::Folder(spring_dest.clone()),
                    hovering,
                );
            });
            let drop_browser = self.clone();
            let drop_coord = Rc::clone(&coord);
            let on_external_drop = UnsyncCallback::new(move |drop: ExternalFileDrop| {
                drop_coord.clear_external_spring();
                clear_pending_drag_rejection();
                drop_browser.handle_external_file_drop(dest.clone(), drop);
            });
            let reject_browser = self.clone();
            let reject_coord = Rc::clone(&coord);
            let on_reject = UnsyncCallback::new(move |rejection: FileDropRejection| {
                reject_coord.clear_external_spring();
                clear_pending_drag_rejection();
                match rejection {
                    FileDropRejection::Disallowed => {
                        reject_browser.report_notice(DragRejectReason::IntoSelf.message());
                    }
                    FileDropRejection::EmptyPaths => {
                        reject_browser.report_notice(i18n::text_untracked("drop-no-local-files"));
                    }
                }
            });
            file_drop_target_attrs(FileDropTarget {
                active: internal_active,
                options: FileDropOptions::copy_or_move(DragAction::Move),
                can_accept: Some(can_accept),
                on_hover_change: Some(on_hover_change),
                on_drop: Some(on_external_drop),
                on_reject: Some(on_reject),
            })
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
                            let paths = drag.paths();
                            if try_start_native_file_drag(
                                &move_browser,
                                &move_coord,
                                &drag,
                                &paths,
                            ) {
                                return;
                            }
                            set_active_drag_sources(&move_browser, paths);
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
                    let target = up_coord.hit_drop_target(pos);
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
                                    .filter(|tab| matches!(tab.contents, Contents::Directory))
                                    .and_then(|tab| tab.navigation.path().map(Path::to_path_buf))
                            }),
                            TargetKind::Unsupported => None,
                        };
                        match destination {
                            Some(destination) => {
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
                                    && paths
                                        .iter()
                                        .all(|p| p.parent() == Some(destination.as_path()));
                                if invalid_self_target {
                                    up_browser.report_notice(DragRejectReason::IntoSelf.message());
                                } else if !paths.is_empty() && !same_dir {
                                    let job = PasteJob::new(destination, mode, paths);
                                    up_browser.inspect_paste(job, false, true, None);
                                }
                            }
                            None => {
                                up_browser.report_notice(
                                    DragRejectReason::UnsupportedDestination.message(),
                                );
                            }
                        }
                        ev.stop_propagation();
                    }
                },
                on:pointer_cancel = move |ev| {
                    ev.release_pointer();
                    *cancel_coord.armed.borrow_mut() = None;
                    // When the compositor took over via `WindowHandle::start_drag`, `on_drag_source`
                    // owns clearing `ACTIVE_DRAG_SOURCES` on `Finished`/`Cancelled`.
                    if cancel_coord.native_drag_session.get().is_none() {
                        clear_active_drag_sources(&cancel_browser);
                        cancel_coord.cancel_all();
                    }
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

fn tab_grab_offset(
    node: &NodeRef,
    pointer: (f32, f32),
    presentation: SidebarPresentation,
) -> (f32, f32) {
    row_grab_offset(node, pointer, presentation, TAB_HEIGHT)
}

fn row_grab_offset(
    node: &NodeRef,
    pointer: (f32, f32),
    presentation: SidebarPresentation,
    default_height: f32,
) -> (f32, f32) {
    let fallback_w = (presentation.width() - 2.0 * SIDEBAR_PADDING)
        .max(COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING);
    let Some(bounds) = node.window_bounds() else {
        return (fallback_w * 0.5, default_height * 0.5);
    };
    let scale = node.scale().max(0.01);
    let left = bounds.origin.x.0 / scale;
    let top = bounds.origin.y.0 / scale;
    let width = (bounds.size.width.0 / scale).max(1.0);
    let height = (bounds.size.height.0 / scale).max(1.0);
    (
        (pointer.0 - left).clamp(0.0, width),
        (pointer.1 - top).clamp(0.0, height),
    )
}

fn try_start_native_file_drag(
    browser: &Browser,
    coord: &Rc<DragCoordinator>,
    drag: &FileDrag,
    paths: &[PathBuf],
) -> bool {
    let Some(window) = browser
        .0
        .window
        .as_ref()
        .filter(|window| window.capabilities().drag_source)
    else {
        return false;
    };
    let Ok((uri_list, plain_text)) = clipboard::encode_drag_payloads(paths) else {
        return false;
    };
    let mut request = DragSourceRequest::new(
        vec![
            DragMime::new("text/uri-list", uri_list),
            DragMime::new("text/plain;charset=utf-8", plain_text.clone()),
            DragMime::new("text/plain", plain_text.clone()),
            DragMime::new("UTF8_STRING", plain_text),
        ],
        DragActions::COPY | DragActions::MOVE,
    );
    let count = drag.count();
    let preview_items = drag.preview_items();
    if let Some(icon) = render_drag_card_icon(window, move || {
        let body = render_file_drag_card_body(count, preview_items, None);
        view! {
            DragCardSurface {{body}}
        }
    }) {
        request = request.with_icon(icon);
    }
    match window.start_drag(request) {
        Ok(session) => {
            clear_pending_drag_rejection();
            browser.cancel_terminal_drag_hover();
            set_active_drag_sources(browser, paths.to_vec());
            coord.native_drag_session.set(Some(session));
            *coord.native_drag_paths.borrow_mut() = paths.to_vec();
            true
        }
        Err(error) => {
            tracing::debug!(%error, "native start_drag declined; falling back to window-local drag");
            false
        }
    }
}

fn set_external_spring_target(
    browser: &Browser,
    coord: &Rc<DragCoordinator>,
    key: String,
    kind: TargetKind,
    hovering: bool,
) {
    if !hovering {
        let should_clear = coord
            .external_spring_key
            .borrow()
            .as_deref()
            .is_some_and(|active| active == key);
        if should_clear {
            *coord.external_spring_key.borrow_mut() = None;
            coord.clear_spring();
        }
        return;
    }

    if coord.external_spring_key.borrow().as_deref() == Some(key.as_str()) {
        return;
    }
    *coord.external_spring_key.borrow_mut() = Some(key.clone());
    coord.clear_spring();

    let already_open = match &kind {
        TargetKind::Folder(dest) => {
            browser.read_untracked(|state| state.tab().navigation.path() == Some(dest.as_path()))
        }
        TargetKind::Tab(tab_id) => browser.read_untracked(|state| state.tab().id == *tab_id),
        TargetKind::Unsupported => return,
    };
    if already_open {
        return;
    }
    let Some(timers) = Timers::current() else {
        return;
    };
    let weak = browser.downgrade();
    let spring_coord = Rc::downgrade(coord);
    let expected_key = key;
    let handle = timers.set_timeout(SPRING_LOAD_DELAY, move || {
        let Some(browser) = weak.upgrade() else {
            return;
        };
        let Some(coord) = spring_coord.upgrade() else {
            return;
        };
        if coord.external_spring_key.borrow().as_deref() != Some(expected_key.as_str()) {
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
            TargetKind::Unsupported => {}
        }
    });
    *coord.spring.borrow_mut() = Some(SpringLoad { _timer: handle });
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
    let target = coord.hit_hover_target(pos, &excluded);
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
        TargetKind::Unsupported => return,
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
            TargetKind::Unsupported => {}
        }
    });
    *coord.spring.borrow_mut() = Some(SpringLoad { _timer: handle });
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::HashSet,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use zgui::prelude::NodeRef;

    use super::{ACTIVE_DRAG_SOURCES, DragSnapshot, Entry, FileDrag};

    thread_local! {
        static LAST_TAB_GHOST_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
    }

    pub(super) fn record_tab_ghost_node(node: NodeRef) {
        LAST_TAB_GHOST_NODE.with(|slot| *slot.borrow_mut() = Some(node));
    }
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

    #[test]
    fn is_source_or_descendant_blocks_self_and_children_only() {
        let dir = PathBuf::from("/tmp/drag-folder-a");
        let child = PathBuf::from("/tmp/drag-folder-a/nested/sub");
        let sibling = PathBuf::from("/tmp/drag-folder-ab");
        let key = 0xbeef_usize;

        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow_mut().insert(key, HashSet::from([dir.clone()]));
        });
        assert!(FileDrag::is_source_or_descendant(&dir));
        assert!(FileDrag::is_source_or_descendant(&child));
        assert!(!FileDrag::is_source_or_descendant(&sibling));

        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow_mut().remove(&key);
        });
        assert!(!FileDrag::is_source_or_descendant(&dir));
    }

    #[test]
    fn pending_drag_rejection_tracks_leave_window_and_ignores_stale_roundtrips() {
        use std::time::{Duration, Instant};

        use super::{
            DragRejectReason, PENDING_DRAG_REJECTION, PendingDragRejection, REJECT_CANCEL_WINDOW,
            clear_pending_drag_rejection, clear_pending_drag_rejection_if_left,
            mark_pending_drag_rejection_left, take_active_drag_rejection,
        };

        let window_key = 0x1234_usize;
        let t0 = Instant::now();

        let seed = || {
            PENDING_DRAG_REJECTION.with(|slot| {
                *slot.borrow_mut() = Some(PendingDragRejection {
                    reason: DragRejectReason::IntoSelf,
                    target_window: window_key,
                    target: None,
                    left_at: None,
                });
            });
        };

        // Delayed TargetMime(Some) from an earlier hover while still inside our window must NOT
        // clear an active rejection.
        seed();
        clear_pending_drag_rejection_if_left();
        assert_eq!(
            take_active_drag_rejection(t0).map(|(reason, _)| reason),
            Some(DragRejectReason::IntoSelf)
        );

        // Leave followed promptly by Cancelled within REJECT_CANCEL_WINDOW reports the notice.
        seed();
        mark_pending_drag_rejection_left(window_key, t0);
        assert_eq!(
            take_active_drag_rejection(t0 + Duration::from_millis(20)).map(|(reason, _)| reason),
            Some(DragRejectReason::IntoSelf)
        );

        // Once the pointer leaves our window and another app accepts (TargetMime(Some)), clear it.
        seed();
        mark_pending_drag_rejection_left(window_key, t0);
        clear_pending_drag_rejection_if_left();
        assert!(take_active_drag_rejection(t0 + Duration::from_millis(10)).is_none());

        // Leaving our window and cancelling long afterwards (e.g. dropping on desktop) stays quiet.
        seed();
        mark_pending_drag_rejection_left(window_key, t0);
        assert!(
            take_active_drag_rejection(t0 + REJECT_CANCEL_WINDOW + Duration::from_millis(50))
                .is_none()
        );

        // Moving onto neutral chrome or a valid target clears any prior rejection.
        seed();
        clear_pending_drag_rejection();
        assert!(take_active_drag_rejection(t0).is_none());
    }

    #[test]
    fn external_drop_failures_report_localized_notices_and_skip_same_dir_noop() {
        use std::rc::Rc;
        use std::sync::atomic::{AtomicU8, Ordering};

        use gds::app::Gds;
        use gds::i18n::Language;
        use gds::service::task_manager::Supervisor;
        use gds::ui::drag::ExternalFileDrop;
        use zgui::{
            app::Fonts,
            geom::{DevicePx, Size},
            platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
            prelude::*,
            runtime::AppError,
        };
        use zgui_platform_headless::Harness;

        use crate::entities::location::{places::Places, volumes};
        use crate::entities::preferences;
        use crate::pages::workspace::ui::{Browser, LaunchOptions};
        use crate::shared::i18n as files_i18n;

        #[derive(Default)]
        struct Sink {
            atlas: zgui::atlas::MemorySink,
            target: Option<zgui::render::RenderTarget>,
        }
        impl zgui::render::Renderer for Sink {
            fn capabilities(&self) -> zgui::render::RenderCapabilities {
                zgui::render::RenderCapabilities::MINIMAL
            }
            fn configure(&mut self, target: zgui::render::RenderTarget) {
                self.target = Some(target);
            }
            fn target(&self) -> Option<zgui::render::RenderTarget> {
                self.target
            }
            fn register_external(
                &mut self,
                _: zgui::render::ExternalTexture,
            ) -> zgui::render::TextureHandle {
                zgui::render::TextureHandle(0)
            }
            fn release_external(&mut self, _: zgui::render::TextureHandle) {}
            fn memory(&self) -> zgui::render::MemoryReport {
                zgui::render::MemoryReport::default()
            }
            fn draw(
                &mut self,
                _: &zgui::scene::Scene,
                _: &zgui::bits::DamageSet,
            ) -> zgui::render::FrameOutcome {
                zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
            }
            fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
                &mut self.atlas
            }
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let folder_a = home.join("FolderA");
        let folder_sub = folder_a.join("Sub");
        let note = home.join("note.txt");
        std::fs::create_dir_all(&folder_sub).expect("create FolderA/Sub");
        std::fs::write(&note, b"hi").expect("create note.txt");

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home.clone();

        let make_handle = || {
            let state = Arc::new(AtomicU8::new(0));
            let state_cb = Arc::clone(&state);
            let handle = DropHandle::new(move |ok| {
                state_cb.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            });
            (state, handle)
        };

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(960.0),
                DevicePx(640.0),
            )));
            harness.settle(12);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            let into_self_msg = files_i18n::text_untracked("drop-into-self");
            let no_files_msg = files_i18n::text_untracked("drop-no-local-files");
            let busy_msg = files_i18n::text_untracked("file-operation-busy");

            // 1. Drop folder into itself -> rejected + localized notice.
            browser.0.notice.set(None);
            let (settled, handle) = make_handle();
            browser.handle_external_file_drop(
                folder_a.clone(),
                ExternalFileDrop {
                    paths: vec![folder_a.clone()],
                    action: Some(DragAction::Move),
                    position: (0.0, 0.0),
                    handle: Some(handle),
                },
            );
            assert_eq!(settled.load(Ordering::Relaxed), 2);
            assert_eq!(
                browser.0.notice.get_untracked(),
                Some(into_self_msg.clone())
            );

            // 2. Drop folder into descendant subfolder -> rejected + localized notice.
            browser.0.notice.set(None);
            let (settled, handle) = make_handle();
            browser.handle_external_file_drop(
                folder_sub.clone(),
                ExternalFileDrop {
                    paths: vec![folder_a.clone()],
                    action: Some(DragAction::Copy),
                    position: (0.0, 0.0),
                    handle: Some(handle),
                },
            );
            assert_eq!(settled.load(Ordering::Relaxed), 2);
            assert_eq!(browser.0.notice.get_untracked(), Some(into_self_msg));

            // 3. Drop with empty local paths -> rejected + localized notice.
            browser.0.notice.set(None);
            let (settled, handle) = make_handle();
            browser.handle_external_file_drop(
                folder_a.clone(),
                ExternalFileDrop {
                    paths: Vec::new(),
                    action: Some(DragAction::Copy),
                    position: (0.0, 0.0),
                    handle: Some(handle),
                },
            );
            assert_eq!(settled.load(Ordering::Relaxed), 2);
            assert_eq!(browser.0.notice.get_untracked(), Some(no_files_msg));

            // 4. Move file into its exact same parent folder -> silent no-op (no error toast).
            browser.0.notice.set(None);
            let (settled, handle) = make_handle();
            browser.handle_external_file_drop(
                home.clone(),
                ExternalFileDrop {
                    paths: vec![note.clone()],
                    action: Some(DragAction::Move),
                    position: (0.0, 0.0),
                    handle: Some(handle),
                },
            );
            assert_eq!(settled.load(Ordering::Relaxed), 2);
            assert_eq!(browser.0.notice.get_untracked(), None);

            // 5. Drop valid file while another file operation is running -> busy notice.
            let _cancel = browser
                .begin_operation("busy")
                .expect("start synthetic busy operation");
            browser.0.notice.set(None);
            let (settled, handle) = make_handle();
            browser.handle_external_file_drop(
                folder_a.clone(),
                ExternalFileDrop {
                    paths: vec![note.clone()],
                    action: Some(DragAction::Move),
                    position: (0.0, 0.0),
                    handle: Some(handle),
                },
            );
            assert_eq!(settled.load(Ordering::Relaxed), 2);
            assert_eq!(browser.0.notice.get_untracked(), Some(busy_msg));
            browser.end_operation();

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_size(960.0, 640.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = Sink::default();
                zgui::render::Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                let scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::Chinese).expect("Chinese catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("external drop failure notice test ran");
    }

    #[test]
    fn native_wayland_hover_reject_then_cancel_reports_into_self_notice() {
        use std::rc::Rc;
        use std::time::{Duration, Instant};

        use gds::app::Gds;
        use gds::i18n::Language;
        use gds::service::task_manager::Supervisor;
        use zgui::{
            app::Fonts,
            geom::{CssPx, DevicePx, Point, Size},
            platform::{AppHandler, DragEvent, PlatformError, Surface, SurfaceEvent},
            prelude::*,
            runtime::AppError,
        };
        use zgui_platform_headless::Harness;

        use super::{
            TargetKind, clear_pending_drag_rejection, coordinator_for, set_active_drag_sources,
        };
        use crate::entities::location::{places::Places, volumes};
        use crate::entities::preferences;
        use crate::pages::workspace::ui::{Browser, LaunchOptions};
        use crate::shared::i18n as files_i18n;

        #[derive(Default)]
        struct Sink {
            atlas: zgui::atlas::MemorySink,
            target: Option<zgui::render::RenderTarget>,
        }
        impl zgui::render::Renderer for Sink {
            fn capabilities(&self) -> zgui::render::RenderCapabilities {
                zgui::render::RenderCapabilities::MINIMAL
            }
            fn configure(&mut self, target: zgui::render::RenderTarget) {
                self.target = Some(target);
            }
            fn target(&self) -> Option<zgui::render::RenderTarget> {
                self.target
            }
            fn register_external(
                &mut self,
                _: zgui::render::ExternalTexture,
            ) -> zgui::render::TextureHandle {
                zgui::render::TextureHandle(0)
            }
            fn release_external(&mut self, _: zgui::render::TextureHandle) {}
            fn memory(&self) -> zgui::render::MemoryReport {
                zgui::render::MemoryReport::default()
            }
            fn draw(
                &mut self,
                _: &zgui::scene::Scene,
                _: &zgui::bits::DamageSet,
            ) -> zgui::render::FrameOutcome {
                zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
            }
            fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
                &mut self.atlas
            }
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let folder_a = home.join("FolderA");
        std::fs::create_dir_all(&folder_a).expect("create FolderA");

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let start = home.clone();

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(960.0),
                DevicePx(640.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            let deadline = Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|state| state.tab().rows.is_empty()) {
                assert!(Instant::now() < deadline, "timed out waiting for rows");
                harness.advance(Duration::from_millis(25));
                harness.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            harness.settle(8);

            let coord = coordinator_for(&browser);
            let folder_node = coord
                .targets
                .borrow()
                .iter()
                .find_map(|target| match &target.kind {
                    TargetKind::Folder(path) if path == &folder_a => Some(target.node),
                    _ => None,
                })
                .expect("FolderA drop target registered");
            let bounds = folder_node.window_bounds().expect("FolderA row bounds");
            let scale = folder_node.scale().max(0.01);
            let folder_center = Point::new(
                CssPx((bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale),
                CssPx((bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale),
            );
            let surface = harness.platform().offscreens()[0].clone();

            // 1. Simulate Wayland drag of FolderA hovered over FolderA itself -> rejected ->
            // Leave + Cancelled must surface the localized drop-into-self notice.
            let session_1 = DragSessionId::new(11);
            let offer_1 = DragOfferId::new(21);
            clear_pending_drag_rejection();
            browser.0.notice.set(None);
            set_active_drag_sources(&browser, vec![folder_a.clone()]);
            coord.native_drag_session.set(Some(session_1));
            *coord.native_drag_paths.borrow_mut() = vec![folder_a.clone()];

            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Entered {
                offer: offer_1,
                mime_types: vec!["text/uri-list".to_owned()],
                source_actions: DragActions::COPY | DragActions::MOVE,
                action: Some(DragAction::Move),
                paths: Vec::new(),
                position: folder_center,
            }));
            harness.settle(4);
            assert_eq!(
                surface.take_drag_responses(),
                vec![(offer_1, DragDecision::reject())]
            );

            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Left { offer: offer_1 }));
            harness.deliver_to_first(SurfaceEvent::DragSource(DragSourceEvent::Cancelled {
                session: session_1,
            }));
            harness.settle(4);
            assert_eq!(
                browser.0.notice.get_untracked(),
                Some(files_i18n::text_untracked("drop-into-self"))
            );

            // 2. Hovering FolderA (rejected) and then moving onto neutral chrome before release
            // must clear the rejection so normal cancellation stays silent.
            let session_2 = DragSessionId::new(12);
            let offer_2 = DragOfferId::new(22);
            browser.0.notice.set(None);
            set_active_drag_sources(&browser, vec![folder_a.clone()]);
            coord.native_drag_session.set(Some(session_2));
            *coord.native_drag_paths.borrow_mut() = vec![folder_a.clone()];

            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Entered {
                offer: offer_2,
                mime_types: vec!["text/uri-list".to_owned()],
                source_actions: DragActions::COPY | DragActions::MOVE,
                action: Some(DragAction::Move),
                paths: Vec::new(),
                position: folder_center,
            }));
            harness.settle(4);
            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Moved {
                offer: offer_2,
                position: Point::new(CssPx(500.0), CssPx(10.0)),
            }));
            harness.settle(4);
            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Left { offer: offer_2 }));
            harness.deliver_to_first(SurfaceEvent::DragSource(DragSourceEvent::Cancelled {
                session: session_2,
            }));
            harness.settle(4);
            assert_eq!(browser.0.notice.get_untracked(), None);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_size(960.0, 640.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = Sink::default();
                zgui::render::Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                let scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n().install_catalog(
                    files_i18n::prepare(Language::Chinese).expect("Chinese catalog"),
                );
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                *slot_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .expect("native wayland hover-reject cancel test ran");
    }

    #[test]
    fn dragged_tab_ghost_adapts_to_expanded_and_collapsed_sidebar() {
        use gds::appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_PADDING, SIDEBAR_WIDTH};
        use gds::ui::{IconName, sidebar::SidebarPresentation};
        use zgui::{
            app::Fonts,
            geom::{DevicePx, Size},
            platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
            prelude::*,
            reactive::RwSignal,
            runtime::AppError,
            view,
        };
        use zgui_platform_headless::Harness;

        use super::{DraggedTabGhostProps, TAB_HEIGHT, TabDragPreview};

        #[derive(Default)]
        struct Sink {
            atlas: zgui::atlas::MemorySink,
            target: Option<zgui::render::RenderTarget>,
        }
        impl zgui::render::Renderer for Sink {
            fn capabilities(&self) -> zgui::render::RenderCapabilities {
                zgui::render::RenderCapabilities::MINIMAL
            }
            fn configure(&mut self, target: zgui::render::RenderTarget) {
                self.target = Some(target);
            }
            fn target(&self) -> Option<zgui::render::RenderTarget> {
                self.target
            }
            fn register_external(
                &mut self,
                _: zgui::render::ExternalTexture,
            ) -> zgui::render::TextureHandle {
                zgui::render::TextureHandle(0)
            }
            fn release_external(&mut self, _: zgui::render::TextureHandle) {}
            fn memory(&self) -> zgui::render::MemoryReport {
                zgui::render::MemoryReport::default()
            }
            fn draw(
                &mut self,
                _: &zgui::scene::Scene,
                _: &zgui::bits::DamageSet,
            ) -> zgui::render::FrameOutcome {
                zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
            }
            fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
                &mut self.atlas
            }
        }

        let presentation_slot: std::rc::Rc<
            std::cell::RefCell<Option<RwSignal<SidebarPresentation, LocalStorage>>>,
        > = std::rc::Rc::new(std::cell::RefCell::new(None));
        let pres_view = std::rc::Rc::clone(&presentation_slot);
        let pres_driver = std::rc::Rc::clone(&presentation_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(400.0),
                DevicePx(240.0),
            )));
            harness.settle(8);

            let ghost_ref = LAST_TAB_GHOST_NODE
                .with(|slot| *slot.borrow())
                .expect("tab ghost node recorded");
            let expanded_rect = ghost_ref
                .window_bounds()
                .expect("expanded tab ghost must have layout bounds");
            let expected_expanded = SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING;
            assert!(
                (expanded_rect.size.width.0 - expected_expanded).abs() < 1.0,
                "expected expanded ghost width ~{expected_expanded}, got {}",
                expanded_rect.size.width.0
            );
            assert!(
                (expanded_rect.size.height.0 - TAB_HEIGHT).abs() < 1.0,
                "expected ghost height ~{TAB_HEIGHT}, got {}",
                expanded_rect.size.height.0
            );

            // Switch sidebar presentation to collapsed -> ghost shrinks to collapsed icon width.
            pres_driver
                .borrow()
                .expect("presentation signal initialized")
                .set(SidebarPresentation::from_collapsed(true));
            harness.settle(8);

            let collapsed_rect = ghost_ref
                .window_bounds()
                .expect("collapsed tab ghost must have layout bounds");
            let expected_collapsed = COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING;
            assert!(
                (collapsed_rect.size.width.0 - expected_collapsed).abs() < 1.0,
                "expected collapsed ghost width ~{expected_collapsed}, got {}",
                collapsed_rect.size.width.0
            );
            assert!(
                (collapsed_rect.size.height.0 - TAB_HEIGHT).abs() < 1.0,
                "expected collapsed ghost height ~{TAB_HEIGHT}, got {}",
                collapsed_rect.size.height.0
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_size(400.0, 240.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = Sink::default();
                zgui::render::Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                crate::pages::workspace::chrome::install_chrome_stylesheets();

                let pres = RwSignal::new_local(SidebarPresentation::from_collapsed(false));
                *pres_view.borrow_mut() = Some(pres);
                let pos = Signal::stored_local((120.0, 80.0));
                let preview = TabDragPreview {
                    id: 1,
                    pinned: true,
                    selected: true,
                    icon: IconName::Folder,
                    title: "Projects".to_owned(),
                    detail_path: "/home/atlas/Projects".to_owned(),
                    grab_offset: (18.0, 22.0),
                };
                view! {
                    box(style:width = "400px", style:height = "240px") {
                        DraggedTabGhost(
                            preview = preview,
                            position = pos,
                            presentation = pres.read_only().into(),
                        )
                    }
                }
            })
            .expect("tab ghost presentation test ran");
    }

    #[cfg(feature = "files-e2e")]
    #[test]
    fn offscreen_file_drag_card_renders_fixed_width_single_three_and_overflow_variants() {
        use std::rc::Rc;

        use gds::ui::drag::{DragCardSurfaceProps, render_drag_card_icon};
        use zgui::{
            app::Fonts,
            geom::{DevicePx, Size},
            platform::{AppHandler, PlatformError, SurfaceEvent},
            prelude::*,
            render::{RenderTarget, Renderer},
            runtime::AppError,
            view,
        };
        use zgui_platform_headless::Harness;
        use zgui_render_wgpu::{SharedGraphics, wgpu};

        use super::{DragItem, FileIcon, render_file_drag_card_body};

        let graphics = SharedGraphics::new();
        if graphics.open_gpu().is_err() {
            return;
        }
        let window_graphics = graphics.clone();
        let offscreen_graphics = graphics;

        let handle_slot: Rc<RefCell<Option<WindowHandle>>> = Rc::default();
        let handle_view = Rc::clone(&handle_slot);
        let handle_driver = Rc::clone(&handle_slot);

        let item = |name: &str, kind: EntryKind| DragItem {
            path: PathBuf::from(format!("/tmp/{name}")),
            name: name.to_owned(),
            icon: FileIcon::for_entry(&Entry::new(
                PathBuf::from(format!("/tmp/{name}")),
                name.into(),
                kind,
                Some(1024),
                None,
                EntryDetails::default(),
            )),
        };

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(400.0),
                DevicePx(240.0),
            )));
            harness.settle(6);

            let window = handle_driver
                .borrow()
                .clone()
                .expect("window handle captured");

            let single = render_drag_card_icon(&window, || {
                let body = render_file_drag_card_body(
                    1,
                    vec![item("rongxiaogou-1.0-pc.zip", EntryKind::File)],
                    None,
                );
                view! { DragCardSurface {{body}} }
            })
            .expect("single-item drag card rendered");

            let three = render_drag_card_icon(&window, || {
                let body = render_file_drag_card_body(
                    3,
                    vec![
                        item("alpha.txt", EntryKind::File),
                        item("Projects", EntryKind::Directory),
                        item("rongxiaogou-1.0-pc.zip", EntryKind::File),
                    ],
                    None,
                );
                view! { DragCardSurface {{body}} }
            })
            .expect("three-item drag card rendered");

            let five = render_drag_card_icon(&window, || {
                let body = render_file_drag_card_body(
                    5,
                    vec![
                        item(
                            "very-long-latin-file-name-that-overflows-with-ellipsis.tar.gz",
                            EntryKind::File,
                        ),
                        item(
                            "超长中文文件夹名称溢出自动显示省略号测试",
                            EntryKind::Directory,
                        ),
                        item("rongxiaogou-1.0-pc.zip", EntryKind::File),
                    ],
                    None,
                );
                view! { DragCardSurface {{body}} }
            })
            .expect("five-item overflow drag card rendered");

            // All variants share the exact fixed card width (+ symmetric horizontal shadow pad).
            assert_eq!(single.image.width(), three.image.width());
            assert_eq!(three.image.width(), five.image.width());
            // Height grows cleanly from 1 item -> 3 items (+ header) -> 5 items (+ ellipsis row).
            assert!(three.image.height() > single.image.height());
            assert!(five.image.height() > three.image.height());
            // Corner pixels stay transparent; card interior has opaque rasterised pixels.
            assert_eq!(single.image.rgba()[3], 0);
            assert!(
                five.image
                    .rgba()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|px| px[3] == 255)
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::system())
            .with_size(400.0, 240.0)
            .with_renderer(Box::new(move |_, target: RenderTarget| {
                let mut r = window_graphics.renderer_offscreen(
                    target,
                    wgpu::TextureFormat::Rgba8Unorm,
                    false,
                )?;
                zgui_render_vector_vello::attach(&mut r, target.size);
                Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
            }))
            .with_offscreen_renderer(Box::new(move |target: RenderTarget| {
                let mut r = offscreen_graphics.renderer_offscreen(
                    target,
                    wgpu::TextureFormat::Rgba8Unorm,
                    false,
                )?;
                zgui_render_vector_vello::attach(&mut r, target.size);
                Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
            }))
            .run_on(driver, move || {
                let gds = gds::app::Gds::install(gds::service::task_manager::Scope::default());
                gds.appearance().install_window_sheet();
                *handle_view.borrow_mut() = Some(use_window());
                view! { column() }
            })
            .expect("offscreen drag card test ran");
    }
}
