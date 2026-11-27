//! Per-window drag-and-drop coordinator, lazy selection snapshots, spring-loading, and rejection tracking.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use gds::ui::{
    IconName,
    drag::{DRAG_THRESHOLD, DragOverlayState, drop_hit_test},
};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, Signal, on_cleanup_local};
use zgui::view;
use zgui::view::{EventCx, ListenerGuard, ListenerOptions, TimeoutHandle, Timers, events};
use zgui::vocab::{Key, NamedKey, PointerButton};

use crate::{
    entities::file::{directory::Entry, ui::FileIcon},
    pages::workspace::{
        navigation::Navigator,
        operations::Lifecycle,
        projection::Rows,
        state::context::{Context, WeakContext},
    },
    shared::{i18n, lib::selection::Selection},
};

pub(super) const PREVIEW_VISIBLE_ITEMS: usize = 3;
const SPRING_LOAD_DELAY: Duration = Duration::from_millis(600);
/// Maximum elapsed time between Wayland `wl_data_device.leave` on a rejected Files drop target and
/// the paired `wl_data_source.cancelled` emitted on pointer release.
pub(super) const REJECT_CANCEL_WINDOW: Duration = Duration::from_millis(250);
pub(super) const QUICK_ACCESS_ZONE_KEY: &str = "quick-access-zone";

#[derive(Clone, Debug)]
pub(super) struct DragItem {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) icon: FileIcon,
}

impl From<&Entry> for DragItem {
    fn from(entry: &Entry) -> Self {
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
struct DragSnapshotInner {
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
                    .map(DragItem::from)
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
pub(in crate::pages::workspace) struct FileDrag {
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

    pub(in crate::pages::workspace) fn is_source(path: &Path) -> bool {
        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow()
                .values()
                .any(|sources| !sources.is_empty() && sources.contains(path))
        })
    }

    /// Returns `true` when `destination` equals or lies inside any active drag source in this
    /// process (across all open Files windows).
    pub(in crate::pages::workspace) fn is_source_or_descendant(destination: &Path) -> bool {
        ACTIVE_DRAG_SOURCES.with(|slot| {
            slot.borrow().values().any(|sources| {
                sources
                    .iter()
                    .any(|src| destination == src || destination.starts_with(src))
            })
        })
    }

    pub(in crate::pages::workspace) fn paths(&self) -> Vec<PathBuf> {
        match &self.payload {
            DragPayload::Single(entry) => vec![entry.path.clone()],
            DragPayload::Selection(inner) => inner
                .entries()
                .iter()
                .map(|item| item.path.clone())
                .collect(),
        }
    }

    pub(in crate::pages::workspace) fn count(&self) -> usize {
        match &self.payload {
            DragPayload::Single(_) => 1,
            DragPayload::Selection(inner) => inner.entries().len(),
        }
    }

    pub(super) fn preview_items(&self) -> Vec<DragItem> {
        match &self.payload {
            DragPayload::Single(entry) => vec![DragItem::from(entry)],
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
pub(super) struct TabDragPreview {
    pub(super) id: u64,
    pub(super) pinned: bool,
    pub(super) selected: bool,
    pub(super) icon: IconName,
    pub(super) title: String,
    pub(super) detail_path: String,
    pub(super) grab_offset: (f32, f32),
}

#[derive(Clone)]
pub(super) struct QuickAccessDragPreview {
    pub(super) path: PathBuf,
    pub(super) icon: IconName,
    pub(super) label: String,
    pub(super) selected: bool,
    pub(super) grab_offset: (f32, f32),
}

#[derive(Clone)]
pub(super) enum WindowDragPayload {
    Files(FileDrag),
    Tab(TabDragPreview),
    QuickAccess(QuickAccessDragPreview),
}

pub(super) enum TabDropAction {
    Reorder { from_id: u64, to_id: u64 },
    AddQuickAccess { tab_id: u64 },
}

pub(super) struct QuickAccessReorder {
    pub(super) from: PathBuf,
    pub(super) to: PathBuf,
}

pub(super) struct FileDropOutcome {
    pub(super) drag: FileDrag,
    pub(super) kind: TargetKind,
    pub(super) copy: bool,
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
pub(super) enum TargetKind {
    Folder(PathBuf),
    Tab(u64),
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DragRejectReason {
    IntoSelf,
    UnsupportedDestination,
}

impl DragRejectReason {
    pub(super) fn message(self) -> String {
        match self {
            Self::IntoSelf => i18n::text_untracked("drop-into-self"),
            Self::UnsupportedDestination => i18n::text_untracked("drop-unsupported-destination"),
        }
    }
}

struct PendingDragRejection {
    reason: DragRejectReason,
    target_window: usize,
    target: Option<WeakContext>,
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

#[derive(Clone, Copy)]
enum SpringSource {
    External,
    Overlay,
}

pub(super) struct DragCoordinator {
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
    copy_modifier: Rc<Cell<bool>>,
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
            copy_modifier: Rc::new(Cell::new(false)),
            next_token: Cell::new(1),
        }
    }

    pub(super) fn alloc_token(&self) -> u64 {
        let token = self.next_token.get();
        self.next_token.set(token.saturating_add(1));
        token
    }

    pub(super) fn register_target(&self, key: String, node: NodeRef, kind: TargetKind) -> u64 {
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

    pub(super) fn unregister_target(&self, token: u64) {
        self.targets.borrow_mut().retain(|item| item.token != token);
    }

    pub(super) fn register_tab(&self, id: u64, pinned: bool, node: NodeRef) -> u64 {
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

    pub(super) fn unregister_tab(&self, token: u64) {
        self.tabs.borrow_mut().retain(|item| item.token != token);
    }

    pub(super) fn register_quick_access_zone(&self, node: NodeRef) -> u64 {
        let token = self.alloc_token();
        self.quick_access_zones.borrow_mut().push((token, node));
        token
    }

    pub(super) fn unregister_quick_access_zone(&self, token: u64) {
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

    pub(super) fn register_quick_access_row(&self, path: PathBuf, node: NodeRef) -> u64 {
        let token = self.alloc_token();
        let mut rows = self.quick_access_rows.borrow_mut();
        rows.retain(|item| item.path != path);
        rows.push(RegisteredQuickAccess { token, path, node });
        token
    }

    pub(super) fn unregister_quick_access_row(&self, token: u64) {
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

    #[cfg(test)]
    pub(super) fn folder_target_node(&self, folder: &Path) -> Option<NodeRef> {
        self.targets
            .borrow()
            .iter()
            .find_map(|target| match &target.kind {
                TargetKind::Folder(path) if path == folder => Some(target.node),
                _ => None,
            })
    }

    fn clear_spring(&self) {
        *self.spring.borrow_mut() = None;
    }

    pub(super) fn clear_external_spring(&self) {
        *self.external_spring_key.borrow_mut() = None;
        self.clear_spring();
    }

    pub(super) fn cancel_all(&self) {
        *self.armed.borrow_mut() = None;
        self.clear_external_spring();
        self.overlay.cancel();
    }

    fn cancel_active_drag(&self) -> bool {
        if self.overlay.payload().get_untracked().is_none() {
            return false;
        }
        self.cancel_all();
        true
    }

    pub(super) fn hovered_signal(&self, target_key: String) -> Signal<bool, LocalStorage> {
        let overlay = self.overlay;
        Signal::derive_local(move || overlay.hovered_target().get().as_deref() == Some(&target_key))
    }

    pub(super) fn dragging_tab_signal(&self, id: u64) -> Signal<bool, LocalStorage> {
        let overlay = self.overlay;
        Signal::derive_local(move || {
            overlay.payload().with(
                |payload| matches!(payload, Some(WindowDragPayload::Tab(tab)) if tab.id == id),
            )
        })
    }

    pub(super) fn dragging_quick_access_signal(&self, path: PathBuf) -> Signal<bool, LocalStorage> {
        let overlay = self.overlay;
        Signal::derive_local(move || {
            overlay.payload().with(|payload| {
                matches!(
                    payload,
                    Some(WindowDragPayload::QuickAccess(item)) if item.path == path
                )
            })
        })
    }

    pub(super) fn render_overlay(
        &self,
        render_payload: impl Fn(
            WindowDragPayload,
            Signal<(f32, f32), LocalStorage>,
            Signal<bool, LocalStorage>,
        ) -> AnyView
        + 'static,
    ) -> AnyView {
        let overlay = self.overlay;
        let copy_modifier = Rc::clone(&self.copy_modifier);
        let copy_mode = Signal::derive_local(move || {
            let _ = overlay.position().get();
            copy_modifier.get()
        });

        AnyView::new(view! {
            {{move || {
                overlay
                    .payload()
                    .get()
                    .map(|payload| render_payload(payload, overlay.position(), copy_mode))
            }}}
        })
    }

    pub(super) fn begin_native_drag(
        &self,
        ctx: &Context,
        session: DragSessionId,
        paths: Vec<PathBuf>,
    ) {
        set_active_drag_sources(ctx, paths.clone());
        self.native_drag_session.set(Some(session));
        ctx.0.native_drag_active.set(true);
        *self.native_drag_paths.borrow_mut() = paths;
    }

    fn handle_drag_source_event(&self, weak: &WeakContext, event: DragSourceEvent) {
        if self.native_drag_session.get() != Some(event.session()) {
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
        self.native_drag_session.set(None);
        let dragged_paths = std::mem::take(&mut *self.native_drag_paths.borrow_mut());
        let rejected = if matches!(event, DragSourceEvent::Cancelled { .. }) {
            take_active_drag_rejection(Instant::now())
        } else {
            clear_pending_drag_rejection();
            None
        };
        if let Some(ctx) = weak.upgrade() {
            ctx.0.native_drag_active.set(false);
            clear_active_drag_sources(&ctx);
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
                Lifecycle(&ctx).refresh_affected(&dragged_paths);
            }
            if let Some((reason, target)) = rejected {
                report_drag_rejection(&ctx, reason, target);
            }
        }
    }

    pub(super) fn enter_external_spring_target(
        self: &Rc<Self>,
        ctx: &Context,
        key: String,
        kind: TargetKind,
    ) {
        if self.external_spring_key.borrow().as_deref() == Some(key.as_str()) {
            return;
        }
        *self.external_spring_key.borrow_mut() = Some(key.clone());
        self.clear_spring();
        self.schedule_spring_load(ctx, key, kind, SpringSource::External);
    }

    pub(super) fn leave_external_spring_target(&self, key: &str) {
        let should_clear = self
            .external_spring_key
            .borrow()
            .as_deref()
            .is_some_and(|active| active == key);
        if should_clear {
            *self.external_spring_key.borrow_mut() = None;
            self.clear_spring();
        }
    }

    fn update_file_drag_hover(self: &Rc<Self>, ctx: &Context, pos: (f32, f32)) {
        let excluded = ctx.drag_sources().get_untracked();
        let target = self.hit_hover_target(pos, &excluded);
        let next_key = target.as_ref().map(|t| t.key.clone());
        let prev_key = self.overlay.hovered_target().get_untracked();
        if next_key == prev_key {
            return;
        }
        self.overlay.set_hovered_target(next_key);
        self.clear_spring();
        let Some(target) = target else {
            return;
        };
        self.schedule_spring_load(ctx, target.key, target.kind, SpringSource::Overlay);
    }

    fn is_spring_still_hovered(&self, source: SpringSource, expected_key: &str) -> bool {
        match source {
            SpringSource::External => {
                self.external_spring_key.borrow().as_deref() == Some(expected_key)
            }
            SpringSource::Overlay => {
                self.overlay.hovered_target().get_untracked().as_deref() == Some(expected_key)
            }
        }
    }

    fn schedule_spring_load(
        self: &Rc<Self>,
        ctx: &Context,
        expected_key: String,
        kind: TargetKind,
        source: SpringSource,
    ) {
        let already_open = match &kind {
            TargetKind::Folder(dest) => {
                ctx.read_untracked(|state| state.tab().navigation.path() == Some(dest.as_path()))
            }
            TargetKind::Tab(tab_id) => ctx.read_untracked(|state| state.tab().id == *tab_id),
            TargetKind::Unsupported => return,
        };
        if already_open {
            return;
        }
        let Some(timers) = Timers::current() else {
            return;
        };
        let weak = ctx.downgrade();
        let spring_coord = Rc::downgrade(self);
        let handle = timers.set_timeout(SPRING_LOAD_DELAY, move || {
            let Some(ctx) = weak.upgrade() else {
                return;
            };
            let Some(coord) = spring_coord.upgrade() else {
                return;
            };
            if !coord.is_spring_still_hovered(source, &expected_key) {
                return;
            }
            let nav = Navigator(&ctx);
            match kind {
                TargetKind::Folder(dest) => {
                    let location = Rows(&ctx).local_location(&dest);
                    nav.open(location);
                }
                TargetKind::Tab(tab_id) => {
                    if let Some(index) = nav.tab_index(tab_id) {
                        nav.select_tab(index);
                    }
                }
                TargetKind::Unsupported => {}
            }
        });
        *self.spring.borrow_mut() = Some(SpringLoad { _timer: handle });
    }

    fn has_overlay(&self, pred: impl FnOnce(&WindowDragPayload) -> bool) -> bool {
        self.overlay
            .payload()
            .with_untracked(|payload| payload.as_ref().is_some_and(pred))
    }

    fn arm_primary(
        &self,
        ev: &EventCx<'_, events::PointerDown>,
        build: impl FnOnce((f32, f32)) -> ArmedDrag,
    ) {
        if ev.button != Some(PointerButton::Primary) {
            return;
        }
        let armed = build((ev.position.x.0, ev.position.y.0));
        *self.armed.borrow_mut() = Some(armed);
    }

    fn take_promoted_armed<T>(
        &self,
        pos: (f32, f32),
        extract: impl FnOnce(&ArmedDrag, (f32, f32)) -> Option<T>,
    ) -> Option<T> {
        let promoted = self
            .armed
            .borrow()
            .as_ref()
            .and_then(|armed| extract(armed, pos));
        if promoted.is_some() {
            *self.armed.borrow_mut() = None;
        }
        promoted
    }

    fn end_active_drag<T>(
        &self,
        ev: &mut EventCx<'_, events::PointerUp>,
        disarm: impl FnOnce(&ArmedDrag) -> bool,
        matches_payload: impl FnOnce(&WindowDragPayload) -> bool,
        extract: impl FnOnce(WindowDragPayload) -> Option<T>,
    ) -> Option<(T, (f32, f32))> {
        if ev.button != Some(PointerButton::Primary) {
            return None;
        }
        if self.armed.borrow().as_ref().is_some_and(disarm) {
            *self.armed.borrow_mut() = None;
        }
        if !self.has_overlay(matches_payload) {
            return None;
        }
        ev.release_pointer();
        let pos = (ev.position.x.0, ev.position.y.0);
        self.overlay.end().and_then(extract).map(|item| (item, pos))
    }

    pub(super) fn arm_tab_drag(
        &self,
        id: u64,
        pinned: bool,
        ev: &EventCx<'_, events::PointerDown>,
        grab_offset: impl FnOnce((f32, f32)) -> (f32, f32),
    ) {
        self.arm_primary(ev, |start| ArmedDrag::Tab {
            id,
            pinned,
            start,
            grab_offset: grab_offset(start),
        });
    }

    pub(super) fn move_tab_drag(
        &self,
        id: u64,
        pinned: bool,
        build_preview: impl FnOnce(bool, (f32, f32)) -> TabDragPreview,
        can_add_quick_access: impl FnOnce() -> bool,
        ev: &mut EventCx<'_, events::PointerMove>,
    ) {
        let pos = (ev.position.x.0, ev.position.y.0);
        if let Some((armed_pinned, grab_offset)) =
            self.take_promoted_armed(pos, |armed, pos| match armed {
                ArmedDrag::Tab {
                    id: armed_id,
                    pinned: armed_pinned,
                    start,
                    grab_offset,
                } if *armed_id == id && distance(*start, pos) >= DRAG_THRESHOLD => {
                    Some((*armed_pinned, *grab_offset))
                }
                _ => None,
            })
        {
            ev.capture_pointer();
            self.overlay.begin(
                WindowDragPayload::Tab(build_preview(armed_pinned, grab_offset)),
                pos.0,
                pos.1,
            );
        }
        if self.has_overlay(|p| matches!(p, WindowDragPayload::Tab(_))) {
            self.overlay.move_to(pos.0, pos.1);
            let hovered = self
                .hit_tab(pos)
                .filter(|tab| tab.pinned == pinned)
                .map(|tab| format!("tab:{}", tab.id))
                .or_else(|| {
                    (self.hit_quick_access_zone(pos) && can_add_quick_access())
                        .then(|| QUICK_ACCESS_ZONE_KEY.to_owned())
                });
            self.overlay.set_hovered_target(hovered);
        }
    }

    pub(super) fn finish_tab_drag(
        &self,
        id: u64,
        ev: &mut EventCx<'_, events::PointerUp>,
    ) -> Option<TabDropAction> {
        let (from_id, pos) = self.end_active_drag(
            ev,
            |armed| matches!(armed, ArmedDrag::Tab { id: armed_id, .. } if *armed_id == id),
            |p| matches!(p, WindowDragPayload::Tab(_)),
            |p| match p {
                WindowDragPayload::Tab(TabDragPreview { id, .. }) => Some(id),
                _ => None,
            },
        )?;
        if let Some(target_tab) = self.hit_tab(pos)
            && target_tab.id != from_id
        {
            return Some(TabDropAction::Reorder {
                from_id,
                to_id: target_tab.id,
            });
        }
        self.hit_quick_access_zone(pos)
            .then_some(TabDropAction::AddQuickAccess { tab_id: from_id })
    }

    pub(super) fn arm_quick_access_drag(
        &self,
        path: &Path,
        icon: IconName,
        label: &str,
        ev: &EventCx<'_, events::PointerDown>,
        grab_offset: impl FnOnce((f32, f32)) -> (f32, f32),
    ) {
        self.arm_primary(ev, |start| ArmedDrag::QuickAccess {
            path: path.to_path_buf(),
            icon,
            label: label.to_owned(),
            start,
            grab_offset: grab_offset(start),
        });
    }

    pub(super) fn move_quick_access_drag(
        &self,
        path: &Path,
        is_selected: impl FnOnce(&Path) -> bool,
        ev: &mut EventCx<'_, events::PointerMove>,
    ) {
        let pos = (ev.position.x.0, ev.position.y.0);
        if let Some((armed_path, armed_icon, armed_label, grab_offset)) =
            self.take_promoted_armed(pos, |armed, pos| match armed {
                ArmedDrag::QuickAccess {
                    path: armed_path,
                    icon: armed_icon,
                    label: armed_label,
                    start,
                    grab_offset,
                } if armed_path.as_path() == path && distance(*start, pos) >= DRAG_THRESHOLD => {
                    Some((
                        armed_path.clone(),
                        *armed_icon,
                        armed_label.clone(),
                        *grab_offset,
                    ))
                }
                _ => None,
            })
        {
            let selected = is_selected(&armed_path);
            let payload = WindowDragPayload::QuickAccess(QuickAccessDragPreview {
                path: armed_path,
                icon: armed_icon,
                label: armed_label,
                selected,
                grab_offset,
            });
            ev.capture_pointer();
            self.overlay.begin(payload, pos.0, pos.1);
        }
        if self.has_overlay(|p| matches!(p, WindowDragPayload::QuickAccess(_))) {
            self.overlay.move_to(pos.0, pos.1);
            let hovered = self
                .hit_quick_access_row(pos)
                .map(|row| format!("qa:{}", row.path.display()));
            self.overlay.set_hovered_target(hovered);
        }
    }

    pub(super) fn finish_quick_access_drag(
        &self,
        path: &Path,
        ev: &mut EventCx<'_, events::PointerUp>,
    ) -> Option<QuickAccessReorder> {
        let (from_path, pos) = self.end_active_drag(
            ev,
            |armed| {
                matches!(
                    armed,
                    ArmedDrag::QuickAccess { path: armed_path, .. } if armed_path.as_path() == path
                )
            },
            |p| matches!(p, WindowDragPayload::QuickAccess(_)),
            |p| match p {
                WindowDragPayload::QuickAccess(QuickAccessDragPreview { path, .. }) => Some(path),
                _ => None,
            },
        )?;
        ev.stop_propagation();
        self.hit_quick_access_row(pos)
            .filter(|target| target.path != from_path)
            .map(|target| QuickAccessReorder {
                from: from_path,
                to: target.path,
            })
    }

    pub(super) fn arm_file_drag(
        &self,
        ctx: &Context,
        entry: &Entry,
        ev: &EventCx<'_, events::PointerDown>,
    ) {
        self.arm_primary(ev, |start| {
            mark_row_pointer_down(ctx);
            let tab_id = ctx.read_untracked(|state| state.tab().id);
            ArmedDrag::File {
                entry: entry.clone(),
                tab_id,
                start,
            }
        });
    }

    pub(super) fn move_file_drag(
        self: &Rc<Self>,
        ctx: &Context,
        ev: &mut EventCx<'_, events::PointerMove>,
        materialize: impl FnOnce(u64, &Entry) -> Option<FileDrag>,
        try_native: impl FnOnce(&FileDrag, &[PathBuf]) -> bool,
    ) {
        let pos = (ev.position.x.0, ev.position.y.0);
        self.copy_modifier.set(ev.modifiers.control());
        if let Some((entry, tab_id)) = self.take_promoted_armed(pos, |armed, pos| match armed {
            ArmedDrag::File {
                entry,
                tab_id,
                start,
            } if distance(*start, pos) >= DRAG_THRESHOLD => Some((entry.clone(), *tab_id)),
            _ => None,
        }) && let Some(drag) = materialize(tab_id, &entry)
        {
            let paths = drag.paths();
            if try_native(&drag, &paths) {
                return;
            }
            set_active_drag_sources(ctx, paths);
            ev.capture_pointer();
            self.overlay
                .begin(WindowDragPayload::Files(drag), pos.0, pos.1);
        }
        if self.has_overlay(|p| matches!(p, WindowDragPayload::Files(_))) {
            self.overlay.move_to(pos.0, pos.1);
            self.update_file_drag_hover(ctx, pos);
        }
    }

    pub(super) fn finish_file_drag(
        &self,
        ctx: &Context,
        ev: &mut EventCx<'_, events::PointerUp>,
    ) -> Option<FileDropOutcome> {
        let (drag, pos) = self.end_active_drag(
            ev,
            |armed| matches!(armed, ArmedDrag::File { .. }),
            |p| matches!(p, WindowDragPayload::Files(_)),
            |p| match p {
                WindowDragPayload::Files(drag) => Some(drag),
                _ => None,
            },
        )?;
        self.clear_spring();
        clear_active_drag_sources(ctx);
        let target = self.hit_drop_target(pos)?;
        ev.stop_propagation();
        Some(FileDropOutcome {
            drag,
            kind: target.kind,
            copy: ev.modifiers.control(),
        })
    }

    pub(super) fn cancel_file_drag(
        &self,
        ctx: &Context,
        ev: &mut EventCx<'_, events::PointerCancel>,
    ) {
        ev.release_pointer();
        *self.armed.borrow_mut() = None;
        // When the compositor took over via `WindowHandle::start_drag`, `on_drag_source`
        // owns clearing `ACTIVE_DRAG_SOURCES` on `Finished`/`Cancelled`.
        if self.native_drag_session.get().is_none() {
            clear_active_drag_sources(ctx);
            self.cancel_all();
        }
    }
}

thread_local! {
    static COORDINATORS: RefCell<HashMap<usize, Rc<DragCoordinator>>> = RefCell::new(HashMap::new());
    static CLEANUP_REGISTERED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    pub(super) static ACTIVE_DRAG_SOURCES: RefCell<HashMap<usize, HashSet<PathBuf>>> = RefCell::new(HashMap::new());
    static PENDING_DRAG_REJECTION: RefCell<Option<PendingDragRejection>> = const { RefCell::new(None) };
    static ROW_PRESSED_WINDOW: Cell<usize> = const { Cell::new(0) };
}

fn has_active_drag_sources() -> bool {
    ACTIVE_DRAG_SOURCES.with(|slot| slot.borrow().values().any(|sources| !sources.is_empty()))
}

pub(super) fn record_pending_drag_rejection(ctx: &Context, reason: DragRejectReason) {
    if !has_active_drag_sources() {
        return;
    }
    let target_window = Rc::as_ptr(&ctx.0) as usize;
    let target = Some(ctx.downgrade());
    PENDING_DRAG_REJECTION.with(|slot| {
        *slot.borrow_mut() = Some(PendingDragRejection {
            reason,
            target_window,
            target,
            left_at: None,
        });
    });
}

#[cfg(test)]
pub(super) fn seed_pending_drag_rejection(reason: DragRejectReason, target_window: usize) {
    PENDING_DRAG_REJECTION.with(|slot| {
        *slot.borrow_mut() = Some(PendingDragRejection {
            reason,
            target_window,
            target: None,
            left_at: None,
        });
    });
}

pub(super) fn clear_pending_drag_rejection() {
    PENDING_DRAG_REJECTION.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

pub(super) fn clear_pending_drag_rejection_if_left() {
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

pub(super) fn mark_pending_drag_rejection_left(window_key: usize, now: Instant) {
    PENDING_DRAG_REJECTION.with(|slot| {
        if let Some(pending) = slot.borrow_mut().as_mut()
            && pending.target_window == window_key
            && pending.left_at.is_none()
        {
            pending.left_at = Some(now);
        }
    });
}

pub(super) fn take_active_drag_rejection(
    now: Instant,
) -> Option<(DragRejectReason, Option<WeakContext>)> {
    PENDING_DRAG_REJECTION.with(|slot| {
        let pending = slot.borrow_mut().take()?;
        pending
            .is_active_at(now)
            .then_some((pending.reason, pending.target))
    })
}

fn report_drag_rejection(
    fallback: &Context,
    reason: DragRejectReason,
    target: Option<WeakContext>,
) {
    let notice = reason.message();
    if let Some(target_ctx) = target.and_then(|weak| weak.upgrade()) {
        target_ctx.report_notice(notice);
    } else {
        fallback.report_notice(notice);
    }
}

fn set_active_drag_sources(ctx: &Context, paths: Vec<PathBuf>) {
    let window_key = Rc::as_ptr(&ctx.0) as usize;
    let set: HashSet<PathBuf> = paths.iter().cloned().collect();
    ACTIVE_DRAG_SOURCES.with(|slot| {
        slot.borrow_mut().insert(window_key, set);
    });
    ctx.set_drag_sources(paths);
}

fn clear_active_drag_sources(ctx: &Context) {
    let window_key = Rc::as_ptr(&ctx.0) as usize;
    ACTIVE_DRAG_SOURCES.with(|slot| {
        slot.borrow_mut().remove(&window_key);
    });
    ctx.clear_drag_sources();
}

fn mark_row_pointer_down(ctx: &Context) {
    ROW_PRESSED_WINDOW.with(|slot| slot.set(Rc::as_ptr(&ctx.0) as usize));
}

pub(in crate::pages::workspace) fn take_row_pointer_down(ctx: &Context) -> bool {
    let key = Rc::as_ptr(&ctx.0) as usize;
    ROW_PRESSED_WINDOW.with(|slot| {
        let matched = slot.get() == key;
        slot.set(0);
        matched
    })
}

pub(super) fn coordinator_for(ctx: &Context) -> Rc<DragCoordinator> {
    let window_key = Rc::as_ptr(&ctx.0) as usize;
    if let Some(coord) = COORDINATORS.with(|slot| slot.borrow().get(&window_key).cloned()) {
        return coord;
    }
    // DragCoordinator allocates RwSignal overlay state; anchor it to the window owner so
    // unmounting the first list/row branch does not dispose signals cached per window.
    let coord = ctx.0.owner.with(|| Rc::new(DragCoordinator::new()));
    let needs_cleanup = CLEANUP_REGISTERED.with(|slot| slot.borrow_mut().insert(window_key));
    if needs_cleanup {
        let root_node = ctx.0.root_focus;
        let weak = ctx.downgrade();
        let esc_coord = Rc::clone(&coord);
        let native_weak = ctx.downgrade();
        let cleanup_weak = ctx.downgrade();
        let native_coord = Rc::clone(&coord);
        let native_window = ctx.0.window.clone();
        ctx.0.owner.with(|| {
            let drag_source_guard = native_window.map(|window| {
                window.on_drag_source(move |event: DragSourceEvent| {
                    native_coord.handle_drag_source_event(&native_weak, event);
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
                        if !coord_inner.cancel_active_drag() {
                            return;
                        }
                        if let Some(ctx) = weak_inner.upgrade() {
                            clear_active_drag_sources(&ctx);
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
                if let Some(ctx) = cleanup_weak.upgrade() {
                    ctx.0.native_drag_active.set(false);
                }
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

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (b.0 - a.0).hypot(b.1 - a.1)
}
