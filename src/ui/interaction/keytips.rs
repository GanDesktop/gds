//! Passive `Ctrl` / `Alt` hold-to-reveal shortcut hints (`shortcuts.md` §3, `motion.md`).
//!
//! Controls that own a binding stage it at their bounds while mounted. Holding `Ctrl` or `Alt`
//! immediately reveals compact suffix badges without intercepting key events (`pointer-events: none`),
//! strips the held modifier prefix (`Ctrl+R` -> `R`, `Ctrl+Shift+Tab` -> `Shift+Tab`, `Alt+A` -> `A`),
//! suppresses background workspace hints while a modal [`DialogScopeId`] is active, and clears all
//! hints immediately on modifier release or window focus loss.

#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use zgui::geom::{Css, CssPx, Device, DevicePx, Edges, Point, Rect, Size};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, RenderEffect, RwSignal, Signal, on_cleanup_local, provide_local_context,
    use_local_context,
};
use zgui::view::time::{FrameHandle, Timers};
use zgui::view::{ListenerGuard, OverlayLayer, PortalProps, ShowProps, current_host, events};
use zgui::vocab::{Key, Modifiers, NamedKey, Role, Timestamp};
use zgui::{component, css, view};

use super::shortcut::{HeldModifier, Shortcut};
use crate::app::Gds;

pub(crate) const LABEL_PADDING_X: f32 = 4.0;
pub(crate) const LABEL_PADDING_Y: f32 = 1.0;
pub(crate) const LABEL_TEXT_SIZE: f32 = 10.0;
pub(crate) const LABEL_LINE_HEIGHT: f32 = 1.2;
pub(crate) const EDGE_INSET: f32 = 1.0;
pub const SUFFIX_DURATION_MS: u64 = 140;
pub(crate) const SUFFIX_GAP: f32 = 6.0;
pub(crate) const LABEL_GAP: f32 = 2.0;
pub(crate) const EDGE_MARGIN: f32 = 4.0;
pub(crate) const LABEL_SEPARATION: f32 = 2.0;
pub(crate) const TRAILING_INSET: f32 = 4.0;
const MAX_ROWS: usize = 8;
const NUDGES: [f32; 9] = [0.0, 2.0, -2.0, 4.0, -4.0, 6.0, -6.0, 8.0, -8.0];
const CHAR_WIDTH_FACTOR: f32 = 0.62;

const SHEET: &str = css!(
    r#"
    .gds-keytip-host {
        display: contents;
    }

    .gds-keytip-overlay {
        position: fixed;
        inset: 0;
        pointer-events: none;
    }

    .gds-keytip-floating {
        position: fixed;
        pointer-events: none;
    }

    .gds-keytip-badge {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        padding: 1px 4px;
        border-radius: 4px;
        background: var(--gds-panel);
        border: 1px solid var(--gds-border);
        color: var(--gds-muted-foreground);
        font-family: var(--gds-font-sans);
        font-size: calc(10px * var(--gds-font-scale, 1));
        font-weight: 400;
        line-height: 1.2;
        white-space: nowrap;
        pointer-events: none;
        user-select: none;
    }

    .gds-keytip-suffix {
        display: inline-flex;
        align-items: center;
        flex-shrink: 0;
        overflow: hidden;
        pointer-events: none;
    }

    .gds-keytip-suffix__inner {
        display: inline-flex;
        align-items: center;
        flex-shrink: 0;
        pointer-events: none;
    }
    "#
);

/// Active modal dialog scope identifier inherited by controls mounted inside a modal dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DialogScopeId(pub u64);

impl DialogScopeId {
    /// Returns the enclosing modal dialog scope, if the caller is mounted inside one.
    #[must_use]
    pub fn current() -> Option<Self> {
        zgui::reactive::is_ui_thread()
            .then(use_local_context::<Self>)
            .flatten()
    }
}

thread_local! {
    static NEXT_SCOPE_ID: Cell<u64> = const { Cell::new(1) };
    static FALLBACK_MODAL_SCOPES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn allocate_dialog_scope_id() -> u64 {
    NEXT_SCOPE_ID.with(|cell| {
        let id = cell.get();
        cell.set(id.saturating_add(1));
        id
    })
}

pub(crate) fn push_modal_scope(registry: Option<KeytipRegistry>, scope_id: u64) {
    FALLBACK_MODAL_SCOPES.with(|stack| {
        let mut stack = stack.borrow_mut();
        if !stack.contains(&scope_id) {
            stack.push(scope_id);
        }
    });
    if let Some(reg) = registry {
        let _ = reg.modal_scopes.try_update(|stack| {
            if !stack.contains(&scope_id) {
                stack.push(scope_id);
            }
        });
    }
}

pub(crate) fn remove_modal_scope(registry: Option<KeytipRegistry>, scope_id: u64) {
    FALLBACK_MODAL_SCOPES.with(|stack| {
        stack.borrow_mut().retain(|&id| id != scope_id);
    });
    if let Some(reg) = registry {
        let _ = reg.modal_scopes.try_update(|stack| {
            stack.retain(|&id| id != scope_id);
        });
    }
}

/// Returns `true` when at least one modal `DialogScope` is currently active in the window.
#[must_use]
pub fn has_active_modal_scope() -> bool {
    if let Some(reg) = KeytipRegistry::current() {
        return !reg.modal_scopes.get_untracked().is_empty();
    }
    FALLBACK_MODAL_SCOPES.with(|stack| !stack.borrow().is_empty())
}

pub(crate) fn scope_allows(active_modals: &[u64], hint_scope: Option<u64>) -> bool {
    match active_modals.last().copied() {
        None => hint_scope.is_none(),
        Some(top_modal) => hint_scope == Some(top_modal),
    }
}

/// Preferred anchor placement for a floating keytip badge relative to its control (`main:keytips.rs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HintPlacement {
    /// Inside an icon button or collapsed row's top-trailing corner (`EDGE_INSET = 1px`).
    Corner,
    /// Vertically centered in a wide row's trailing space (`TRAILING_INSET = 4px`).
    Trailing,
    /// Vertically centered before a trailing action occupying `f32` px from the row's right edge.
    TrailingBefore(f32),
    /// Inside a compact row's bottom-leading corner when its top-trailing corner holds an action.
    BottomLeadingCorner,
}

/// Registered floating keytip entry within the current window.
#[derive(Clone)]
pub struct StagedHint {
    pub id: u64,
    pub target: NodeRef,
    pub shortcut: Shortcut,
    pub placement: Signal<HintPlacement, LocalStorage>,
    pub enabled: Signal<bool, LocalStorage>,
    pub dialog_scope: Option<u64>,
    pub badge_ref: NodeRef,
    observed_target: Signal<Option<Rect<DevicePx, Device>>, LocalStorage>,
}

/// Window-local passive modifier-hold state and staged keytip registry.
#[derive(Clone, Copy)]
pub struct KeytipRegistry {
    held: RwSignal<Option<HeldModifier>, LocalStorage>,
    active: RwSignal<bool, LocalStorage>,
    suppressed_until_release: RwSignal<bool, LocalStorage>,
    hints: RwSignal<Vec<StagedHint>, LocalStorage>,
    modal_scopes: RwSignal<Vec<u64>, LocalStorage>,
    next_hint_id: RwSignal<u64, LocalStorage>,
    prefix: RwSignal<String, LocalStorage>,
}

impl KeytipRegistry {
    /// Returns the enclosing window's [`KeytipRegistry`], if running inside a [`KeytipHost`].
    #[must_use]
    pub fn current() -> Option<Self> {
        zgui::reactive::is_ui_thread()
            .then(use_local_context::<Self>)
            .flatten()
    }

    /// Signal carrying the currently held modifier group (`Ctrl` or `Alt`), if any.
    #[must_use]
    pub fn held(self) -> Signal<Option<HeldModifier>, LocalStorage> {
        Signal::derive_local(move || {
            if self.suppressed_until_release.get() {
                None
            } else {
                self.held.get()
            }
        })
    }

    /// Signal indicating whether any modifier group is currently active.
    #[must_use]
    pub fn active(self) -> Signal<bool, LocalStorage> {
        self.active.into()
    }

    /// Signal retained for compatibility with callers inspecting chord prefixes.
    #[must_use]
    pub fn prefix(self) -> Signal<String, LocalStorage> {
        self.prefix.into()
    }

    /// Returns `true` (reactively) when `dialog_scope` is allowed to display hints right now.
    #[must_use]
    pub fn is_scope_active(self, dialog_scope: Option<u64>) -> bool {
        let modals = self.modal_scopes.get();
        scope_allows(&modals, dialog_scope)
    }

    /// Sets keytip visibility or clears the held modifier state.
    pub fn set_active(self, next_active: bool) {
        if next_active {
            self.suppressed_until_release.set(false);
            if self.held.get_untracked().is_none() {
                self.held.set(Some(HeldModifier::Alt));
            }
            self.active.set(true);
        } else {
            self.held.set(None);
            self.active.set(false);
            self.prefix.set(String::new());
        }
    }

    /// Updates the held modifier group immediately on key press/release.
    pub fn set_held(self, next: Option<HeldModifier>) {
        self.suppressed_until_release.set(false);
        if self.held.get_untracked() != next {
            self.held.set(next);
        }
        let is_active = next.is_some();
        if self.active.get_untracked() != is_active {
            self.active.set(is_active);
        }
    }

    /// Clears any active modifier-hold state immediately (e.g. on window focus loss).
    pub fn release(self) {
        self.set_held(None);
    }

    /// Stages a [`Shortcut`] on `target` using `placement` until the calling reactive scope cleans up.
    pub fn stage(
        self,
        target: NodeRef,
        shortcut: Shortcut,
        placement: HintPlacement,
        enabled: Signal<bool, LocalStorage>,
    ) {
        self.stage_dynamic(target, shortcut, Signal::stored_local(placement), enabled);
    }

    /// Stages a [`Shortcut`] on `target` using reactive `placement` until the calling reactive scope cleans up.
    pub fn stage_dynamic(
        self,
        target: NodeRef,
        shortcut: Shortcut,
        placement: Signal<HintPlacement, LocalStorage>,
        enabled: Signal<bool, LocalStorage>,
    ) {
        let id = self.next_hint_id.get_untracked();
        self.next_hint_id.set(id.saturating_add(1));
        let dialog_scope = DialogScopeId::current().map(|s| s.0);
        let badge_ref = NodeRef::new();
        let is_held = self.held();
        let observed_target = if current_host().is_some() {
            target.observe_border_box_while(move || is_held.get().is_some())
        } else {
            Signal::stored_local(None)
        };

        self.hints.update(|list| {
            list.retain(|entry| entry.target != target || entry.shortcut != shortcut);
            list.push(StagedHint {
                id,
                target,
                shortcut,
                placement,
                enabled,
                dialog_scope,
                badge_ref,
                observed_target,
            });
        });

        let hints = self.hints;
        on_cleanup_local(move || {
            let _ = hints.try_update(|list| list.retain(|entry| entry.id != id));
        });
    }
}

/// Stages a [`Shortcut`] on `target` with explicit [`HintPlacement`] and reactive `enabled` guard.
pub fn stage_shortcut(
    target: NodeRef,
    shortcut: impl Into<Shortcut>,
    placement: HintPlacement,
    enabled: Signal<bool, LocalStorage>,
) {
    if let Some(registry) = KeytipRegistry::current() {
        registry.stage(target, shortcut.into(), placement, enabled);
    }
}

/// Stages a [`Shortcut`] on `target` with reactive [`HintPlacement`] and `enabled` guard.
pub fn stage_shortcut_dynamic(
    target: NodeRef,
    shortcut: impl Into<Shortcut>,
    placement: Signal<HintPlacement, LocalStorage>,
    enabled: Signal<bool, LocalStorage>,
) {
    if let Some(registry) = KeytipRegistry::current() {
        registry.stage_dynamic(target, shortcut.into(), placement, enabled);
    }
}

/// Stages a top-trailing corner keytip badge (`HintPlacement::Corner`) on `target`.
pub fn anchor(target: NodeRef, shortcut: impl Into<Shortcut>) {
    stage_shortcut(
        target,
        shortcut,
        HintPlacement::Corner,
        Signal::stored_local(true),
    );
}

/// Stages a top-trailing corner keytip badge gated by `enabled`.
pub fn anchor_when(
    target: NodeRef,
    shortcut: impl Into<Shortcut>,
    enabled: Signal<bool, LocalStorage>,
) {
    stage_shortcut(target, shortcut, HintPlacement::Corner, enabled);
}

/// Stages a row-trailing keytip badge (`HintPlacement::Trailing`) on `target`.
pub fn row_anchor(target: NodeRef, shortcut: impl Into<Shortcut>) {
    stage_shortcut(
        target,
        shortcut,
        HintPlacement::Trailing,
        Signal::stored_local(true),
    );
}

/// Stages a row-trailing keytip badge before a trailing control of width `reserved` px.
pub fn trailing_before_anchor(target: NodeRef, shortcut: impl Into<Shortcut>, reserved: f32) {
    stage_shortcut(
        target,
        shortcut,
        HintPlacement::TrailingBefore(reserved),
        Signal::stored_local(true),
    );
}

/// Stages a compact row's bottom-leading corner keytip badge (`HintPlacement::BottomLeadingCorner`).
pub fn bottom_corner_anchor(target: NodeRef, shortcut: impl Into<Shortcut>) {
    stage_shortcut(
        target,
        shortcut,
        HintPlacement::BottomLeadingCorner,
        Signal::stored_local(true),
    );
}

/// Computes the modifier bit contributed by pressing/releasing `key` itself so Wayland, winit,
/// and headless backends agree regardless of whether `ModifiersChanged` arrives before or after `Key`.
#[must_use]
pub(crate) fn modifier_bit_for_key(key: &Key) -> Modifiers {
    match key {
        Key::Named(NamedKey::Control) => Modifiers::CONTROL,
        Key::Named(NamedKey::Alt) => Modifiers::ALT,
        Key::Named(NamedKey::AltGraph) => Modifiers::CONTROL | Modifiers::ALT,
        Key::Named(NamedKey::Shift) => Modifiers::SHIFT,
        Key::Named(NamedKey::Meta | NamedKey::Super) => Modifiers::META,
        _ => Modifiers::NONE,
    }
}

#[must_use]
pub(crate) fn held_after_key_down(key: &Key, modifiers: Modifiers) -> Option<HeldModifier> {
    if matches!(key, Key::Named(NamedKey::AltGraph)) {
        return None;
    }
    let effective = modifiers.held() | modifier_bit_for_key(key);
    HeldModifier::from_modifiers(effective)
}

#[must_use]
pub(crate) fn held_after_key_up(key: &Key, modifiers: Modifiers) -> Option<HeldModifier> {
    let effective = modifiers.held() & !modifier_bit_for_key(key);
    HeldModifier::from_modifiers(effective)
}

/// Measured or estimated slot awaiting collision-free placement inside `content`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LabelSlot {
    pub anchor: Rect<CssPx, Css>,
    pub size: Size<CssPx, Css>,
    pub placement: HintPlacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Below,
    Above,
}

fn rect_center_x(r: Rect<CssPx, Css>) -> f32 {
    r.left().0 + r.width().0 * 0.5
}

fn rect_center_y(r: Rect<CssPx, Css>) -> f32 {
    r.top().0 + r.height().0 * 0.5
}

pub(crate) fn rects_intersect(a: Rect<CssPx, Css>, b: Rect<CssPx, Css>) -> bool {
    a.intersects(b)
}

fn free(origin: Point<CssPx, Css>, size: Size<CssPx, Css>, placed: &[Rect<CssPx, Css>]) -> bool {
    let dilated = Rect::new(origin, size).outset(Edges::uniform(CssPx(LABEL_SEPARATION)));
    placed.iter().all(|existing| !dilated.intersects(*existing))
}

fn clamp_axis(origin: f32, length: f32, start: f32, end: f32) -> f32 {
    origin.clamp(
        start + EDGE_MARGIN,
        (end - EDGE_MARGIN - length).max(start + EDGE_MARGIN),
    )
}

fn fits(left: f32, width: f32, content: Rect<CssPx, Css>) -> bool {
    left >= content.left().0 + EDGE_MARGIN && left + width <= content.right().0 - EDGE_MARGIN
}

fn shifted_columns(preferred: f32, slot: LabelSlot, content: Rect<CssPx, Css>) -> Vec<f32> {
    let mut columns: Vec<f32> = NUDGES
        .into_iter()
        .map(|nudge| preferred + nudge)
        .filter(|left| fits(*left, slot.size.width.0, content))
        .collect();
    if columns.is_empty() {
        columns.push(clamp_axis(
            preferred,
            slot.size.width.0,
            content.left().0,
            content.right().0,
        ));
    }
    columns
}

fn corner_origins(slot: LabelSlot, content: Rect<CssPx, Css>) -> Vec<Point<CssPx, Css>> {
    let top = clamp_axis(
        slot.anchor.top().0 + EDGE_INSET,
        slot.size.height.0,
        content.top().0,
        content.bottom().0,
    );
    let preferred = slot.anchor.right().0 - EDGE_INSET - slot.size.width.0;
    shifted_columns(preferred, slot, content)
        .into_iter()
        .map(|left| Point::new(CssPx(left), CssPx(top)))
        .collect()
}

fn trailing_origin(slot: LabelSlot, content: Rect<CssPx, Css>) -> Option<Point<CssPx, Css>> {
    if slot.anchor.size.width.0 < slot.size.width.0 + TRAILING_INSET * 2.0
        || slot.anchor.size.height.0 < slot.size.height.0
    {
        return None;
    }
    Some(Point::new(
        CssPx(clamp_axis(
            slot.anchor.right().0 - TRAILING_INSET - slot.size.width.0,
            slot.size.width.0,
            content.left().0,
            content.right().0,
        )),
        CssPx(clamp_axis(
            rect_center_y(slot.anchor) - slot.size.height.0 * 0.5,
            slot.size.height.0,
            content.top().0,
            content.bottom().0,
        )),
    ))
}

fn trailing_before_origins(
    slot: LabelSlot,
    content: Rect<CssPx, Css>,
    reserved: f32,
) -> Vec<Point<CssPx, Css>> {
    if slot.anchor.size.height.0 < slot.size.height.0 {
        return Vec::new();
    }
    let step = slot.size.width.0 + LABEL_SEPARATION;
    let top = clamp_axis(
        rect_center_y(slot.anchor) - slot.size.height.0 * 0.5,
        slot.size.height.0,
        content.top().0,
        content.bottom().0,
    );
    let mut origins = Vec::new();
    let mut left = slot.anchor.right().0 - reserved - slot.size.width.0;
    while left >= slot.anchor.left().0 && fits(left, slot.size.width.0, content) {
        origins.push(Point::new(CssPx(left), CssPx(top)));
        left -= step;
    }
    origins
}

fn bottom_leading_origins(slot: LabelSlot, content: Rect<CssPx, Css>) -> Vec<Point<CssPx, Css>> {
    let top = clamp_axis(
        slot.anchor.bottom().0 - EDGE_INSET - slot.size.height.0,
        slot.size.height.0,
        content.top().0,
        content.bottom().0,
    );
    let preferred = slot.anchor.left().0 + EDGE_INSET;
    let mut columns: Vec<f32> = NUDGES
        .into_iter()
        .filter(|nudge| *nudge >= 0.0)
        .map(|nudge| preferred + nudge)
        .filter(|left| fits(*left, slot.size.width.0, content))
        .collect();
    if columns.is_empty() {
        columns.push(clamp_axis(
            preferred,
            slot.size.width.0,
            content.left().0,
            content.right().0,
        ));
    }
    columns
        .into_iter()
        .map(|left| Point::new(CssPx(left), CssPx(top)))
        .collect()
}

fn edge_top(slot: LabelSlot, content: Rect<CssPx, Css>, side: Side, row: usize) -> Option<f32> {
    let height = slot.size.height.0;
    let step = height + LABEL_SEPARATION;
    let top = match side {
        Side::Below if row == 0 => slot.anchor.bottom().0 - height * 0.5,
        Side::Below => slot.anchor.bottom().0 + height * 0.5 + LABEL_GAP + step * (row - 1) as f32,
        Side::Above if row == 0 => slot.anchor.top().0 - height * 0.5,
        Side::Above => {
            slot.anchor.top().0 - height * 0.5 - LABEL_GAP - height - step * (row - 1) as f32
        }
    };
    (top >= content.top().0 + EDGE_MARGIN && top + height <= content.bottom().0 - EDGE_MARGIN)
        .then_some(top)
}

fn edge_columns(slot: LabelSlot, content: Rect<CssPx, Css>) -> Vec<f32> {
    shifted_columns(
        slot.anchor.right().0 - EDGE_INSET - slot.size.width.0,
        slot,
        content,
    )
}

fn candidates(slot: LabelSlot, content: Rect<CssPx, Css>) -> Vec<Point<CssPx, Css>> {
    let mut list = match slot.placement {
        HintPlacement::Corner => corner_origins(slot, content),
        HintPlacement::Trailing => trailing_origin(slot, content).into_iter().collect(),
        HintPlacement::TrailingBefore(reserved) => trailing_before_origins(slot, content, reserved),
        HintPlacement::BottomLeadingCorner => bottom_leading_origins(slot, content),
    };
    for side in [Side::Below, Side::Above] {
        for row in 0..MAX_ROWS {
            let Some(top) = edge_top(slot, content, side, row) else {
                break;
            };
            list.extend(
                edge_columns(slot, content)
                    .into_iter()
                    .map(|left| Point::new(CssPx(left), CssPx(top))),
            );
        }
    }
    list
}

fn overflow_origin(
    slot: LabelSlot,
    content: Rect<CssPx, Css>,
    placed: &[Rect<CssPx, Css>],
) -> Point<CssPx, Css> {
    let left = content.left().0 + EDGE_MARGIN;
    let right = content.right().0 - EDGE_MARGIN;
    let top = content.top().0 + EDGE_MARGIN;
    let bottom = content.bottom().0 - EDGE_MARGIN;
    let step_x = (slot.size.width.0 + LABEL_SEPARATION).max(1.0);
    let step_y = (slot.size.height.0 + LABEL_SEPARATION).max(1.0);
    let mut y = top;
    while y + slot.size.height.0 <= bottom {
        let mut x = left;
        while x + slot.size.width.0 <= right {
            let origin = Point::new(CssPx(x), CssPx(y));
            if free(origin, slot.size, placed) {
                return origin;
            }
            x += step_x;
        }
        y += step_y;
    }
    Point::new(
        CssPx(clamp_axis(
            rect_center_x(slot.anchor) - slot.size.width.0 * 0.5,
            slot.size.width.0,
            content.left().0,
            content.right().0,
        )),
        CssPx(clamp_axis(
            slot.anchor.bottom().0 + LABEL_GAP,
            slot.size.height.0,
            content.top().0,
            content.bottom().0,
        )),
    )
}

/// Places ordered `slots` inside `content` without overlap (`main:keytips.rs:1235`).
#[must_use]
pub(crate) fn place_labels(
    slots: &[LabelSlot],
    content: Rect<CssPx, Css>,
) -> Vec<Point<CssPx, Css>> {
    let mut placed: Vec<Rect<CssPx, Css>> = Vec::with_capacity(slots.len());
    let mut origins = Vec::with_capacity(slots.len());
    for &slot in slots {
        let origin = candidates(slot, content)
            .into_iter()
            .find(|origin| free(*origin, slot.size, &placed))
            .unwrap_or_else(|| overflow_origin(slot, content, &placed));
        placed.push(Rect::new(origin, slot.size));
        origins.push(origin);
    }
    origins
}

fn typography_scale_untracked() -> f32 {
    Gds::try_current().map_or(1.0, |gds| {
        gds.appearance().typography().scaled_untracked(1.0)
    })
}

fn estimated_badge_size(label: &str) -> Size<CssPx, Css> {
    let scale = typography_scale_untracked();
    let text_px = LABEL_TEXT_SIZE * scale;
    let chars = label.chars().count().max(1) as f32;
    let width = (chars * text_px * CHAR_WIDTH_FACTOR + LABEL_PADDING_X * 2.0 + 2.0).ceil();
    let height = (text_px * LABEL_LINE_HEIGHT + LABEL_PADDING_Y * 2.0 + 2.0).ceil();
    Size::new(CssPx(width.max(14.0)), CssPx(height.max(14.0)))
}

fn motion_reduced() -> bool {
    Gds::try_current().is_some_and(|gds| gds.appearance().motion().reduced())
}

fn motion_reduced_untracked() -> bool {
    Gds::try_current().is_some_and(|gds| {
        let _z = zgui::reactive::enter_non_reactive_zone();
        gds.appearance().motion().reduced()
    })
}

fn ease_out_cubic(t: f32) -> f32 {
    let clamped = t.clamp(0.0, 1.0);
    1.0 - (1.0 - clamped).powi(3)
}

struct SuffixAnim {
    from: f32,
    to: f32,
    started_at: Option<Timestamp>,
    last_frame_at: Option<Timestamp>,
    frame_handle: Option<FrameHandle>,
}

/// Frame-driven `140ms` `ease-out cubic` scalar interpolator with smooth mid-flight retargeting,
/// immediate snap under reduced motion, and zero frame requests once settled (`motion.md`).
pub fn use_inline_transition(
    target: Signal<f32, LocalStorage>,
    duration_ms: u64,
) -> Signal<f32, LocalStorage> {
    let initial = target.get_untracked();
    let current = RwSignal::new_local(initial);
    let timers = Timers::current();
    let state = Rc::new(RefCell::new(SuffixAnim {
        from: initial,
        to: initial,
        started_at: None,
        last_frame_at: None,
        frame_handle: None,
    }));

    fn schedule_next(
        timers: &Timers,
        state: &Rc<RefCell<SuffixAnim>>,
        current: RwSignal<f32, LocalStorage>,
        duration_ms: u64,
    ) {
        let timers_clone = timers.clone();
        let state_clone = Rc::clone(state);
        let handle = timers.request_frame(move |at| {
            if motion_reduced_untracked() || duration_ms == 0 {
                let final_to = {
                    let mut anim = state_clone.borrow_mut();
                    anim.started_at = None;
                    anim.last_frame_at = None;
                    anim.frame_handle = None;
                    anim.from = anim.to;
                    anim.to
                };
                current.set(final_to);
                return;
            }

            let (next_val, settled) = {
                let mut anim = state_clone.borrow_mut();
                let start = match anim.started_at {
                    Some(start) => start,
                    None => {
                        let base = anim.last_frame_at.unwrap_or_else(|| {
                            if duration_ms >= 32 {
                                Timestamp::from_origin(
                                    at.since_origin().saturating_sub(Duration::from_millis(8)),
                                )
                            } else {
                                at
                            }
                        });
                        anim.started_at = Some(base);
                        base
                    }
                };
                anim.last_frame_at = Some(at);
                let elapsed_secs = at.saturating_since(start).as_secs_f32();
                let total_secs = (duration_ms as f32) / 1000.0;
                let progress = (elapsed_secs / total_secs).clamp(0.0, 1.0);
                if progress >= 1.0 {
                    let final_to = anim.to;
                    anim.from = final_to;
                    anim.started_at = None;
                    anim.last_frame_at = None;
                    anim.frame_handle = None;
                    (final_to, true)
                } else {
                    let eased = ease_out_cubic(progress);
                    let interpolated = anim.from + (anim.to - anim.from) * eased;
                    (interpolated, false)
                }
            };

            current.set(next_val);
            if !settled {
                schedule_next(&timers_clone, &state_clone, current, duration_ms);
            }
        });
        state.borrow_mut().frame_handle = Some(handle);
    }

    let watch_state = Rc::clone(&state);
    let effect = RenderEffect::new(move |_| {
        let goal = target.get();
        let reduced = motion_reduced();
        let cur = current.get_untracked();

        if reduced || duration_ms == 0 || timers.is_none() {
            let canceled = {
                let mut anim = watch_state.borrow_mut();
                anim.from = goal;
                anim.to = goal;
                anim.started_at = None;
                anim.last_frame_at = None;
                anim.frame_handle.take()
            };
            drop(canceled);
            if (cur - goal).abs() > f32::EPSILON {
                current.set(goal);
            }
            return;
        }

        let needs_schedule = {
            let mut anim = watch_state.borrow_mut();
            if (anim.to - goal).abs() <= 1e-4 {
                false
            } else {
                anim.from = cur;
                anim.to = goal;
                anim.started_at = None;
                if (cur - goal).abs() <= 1e-4 {
                    anim.last_frame_at = None;
                    let old = anim.frame_handle.take();
                    drop(old);
                    false
                } else {
                    anim.frame_handle.is_none()
                }
            }
        };

        if needs_schedule && let Some(ref timers) = timers {
            schedule_next(timers, &watch_state, current, duration_ms);
        }
    });

    let cleanup_state = Rc::clone(&state);
    on_cleanup_local(move || {
        drop(effect);
        let handle = cleanup_state.borrow_mut().frame_handle.take();
        drop(handle);
    });

    current.into()
}

pub(crate) fn install_keytip_stylesheet() {
    if current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-keytips", SHEET);
    }
}

const POSITION_EPSILON: f32 = 0.05;

#[derive(Clone)]
struct VisibleKeytip {
    id: u64,
    badge_ref: NodeRef,
    label: String,
}

fn render_positioned_keytip(
    item: VisibleKeytip,
    positions: RwSignal<HashMap<u64, (f32, f32)>, LocalStorage>,
) -> AnyView {
    let id = item.id;
    let aria = item.label.clone();
    AnyView::new(view! {
        box(
            node_ref = item.badge_ref,
            class = "gds-keytip-floating",
            style:left = move || {
                positions.with(|map| map.get(&id).map(|(x, _)| format!("{:.1}px", x.round())))
            },
            style:top = move || {
                positions.with(|map| map.get(&id).map(|(_, y)| format!("{:.1}px", y.round())))
            },
            a11y:role = Role::Label,
            a11y:label = aria,
        ) {
            text(class = "gds-keytip-badge") {{item.label}}
        }
    })
}

fn sample_keytips(
    host_ref: NodeRef,
    held_mod: HeldModifier,
    active_modals: &[u64],
    staged: &[StagedHint],
    tracked: bool,
) -> (Vec<VisibleKeytip>, HashMap<u64, (f32, f32)>) {
    if tracked {
        let _ = host_ref.get();
    }
    let window_root = host_ref.window_root();
    let toast_occlusion = crate::ui::feedback::notifications::active_stack_occlusion(
        window_root.and_then(|r| r.get_untracked()),
    );
    let content = window_root
        .and_then(|root| {
            let bounds = root.window_bounds()?;
            let scale = root.scale().max(0.01);
            Some(Rect::new(
                Point::new(
                    CssPx(bounds.origin.x.0 / scale),
                    CssPx(bounds.origin.y.0 / scale),
                ),
                Size::new(
                    CssPx(bounds.size.width.0 / scale),
                    CssPx(bounds.size.height.0 / scale),
                ),
            ))
        })
        .unwrap_or_else(|| {
            Rect::new(
                Point::new(CssPx(0.0), CssPx(0.0)),
                Size::new(CssPx(1280.0), CssPx(800.0)),
            )
        });

    let mut candidates_list = Vec::new();
    for entry in staged {
        if !scope_allows(active_modals, entry.dialog_scope) {
            continue;
        }
        let is_enabled = if tracked {
            entry.enabled.get()
        } else {
            entry.enabled.get_untracked()
        };
        if !is_enabled {
            continue;
        }
        let Some(label) = entry.shortcut.compact_label(held_mod) else {
            continue;
        };
        if tracked {
            let _ = entry.observed_target.get();
        }
        let Some(dev_bounds) = entry.target.window_bounds() else {
            continue;
        };
        let scale = entry.target.scale().max(0.01);
        let anchor = Rect::new(
            Point::new(
                CssPx(dev_bounds.origin.x.0 / scale),
                CssPx(dev_bounds.origin.y.0 / scale),
            ),
            Size::new(
                CssPx(dev_bounds.size.width.0 / scale),
                CssPx(dev_bounds.size.height.0 / scale),
            ),
        );
        if anchor.size.width.0 <= 0.0
            || anchor.size.height.0 <= 0.0
            || !rects_intersect(anchor, content)
        {
            continue;
        }

        let badge_size = entry
            .badge_ref
            .window_bounds()
            .map(|b| {
                let s = entry.badge_ref.scale().max(0.01);
                Size::new(
                    CssPx((b.size.width.0 / s).ceil()),
                    CssPx((b.size.height.0 / s).ceil()),
                )
            })
            .filter(|sz| sz.width.0 > 1.0 && sz.height.0 > 1.0)
            .unwrap_or_else(|| estimated_badge_size(&label));

        let placement = entry.placement.get();
        if let Some((toast_ref, toast_rect)) = toast_occlusion {
            let is_toast_hint = entry.target == toast_ref
                || entry
                    .target
                    .get_untracked()
                    .is_some_and(|id| toast_ref.contains(id));
            if !is_toast_hint {
                let control_occluded = anchor.size.width.0 <= toast_rect.size.width.0
                    && anchor.size.height.0 <= toast_rect.size.height.0
                    && rects_intersect(anchor, toast_rect);
                let slot = LabelSlot {
                    anchor,
                    size: badge_size,
                    placement,
                };
                let badge_occluded = candidates(slot, content).first().is_some_and(|origin| {
                    rects_intersect(Rect::new(*origin, badge_size), toast_rect)
                });
                if control_occluded || badge_occluded {
                    continue;
                }
            }
        }

        candidates_list.push((
            entry.id,
            entry.badge_ref,
            placement,
            label,
            anchor,
            badge_size,
        ));
    }

    // Deterministic visual order: top-to-bottom, then left-to-right, then id.
    candidates_list.sort_by(|left, right| {
        left.4
            .origin
            .y
            .0
            .total_cmp(&right.4.origin.y.0)
            .then_with(|| left.4.origin.x.0.total_cmp(&right.4.origin.x.0))
            .then_with(|| left.0.cmp(&right.0))
    });

    let slots: Vec<LabelSlot> = candidates_list
        .iter()
        .map(|(_, _, placement, _, anchor, size)| LabelSlot {
            anchor: *anchor,
            size: *size,
            placement: *placement,
        })
        .collect();

    let origins = place_labels(&slots, content);
    let mut next_items = Vec::with_capacity(candidates_list.len());
    let mut next_positions = HashMap::with_capacity(candidates_list.len());
    for ((id, badge_ref, _, label, _, _), pos) in candidates_list.into_iter().zip(origins) {
        next_positions.insert(id, (pos.x.0, pos.y.0));
        next_items.push(VisibleKeytip {
            id,
            badge_ref,
            label,
        });
    }
    (next_items, next_positions)
}

fn commit_keytip_sample(
    visible_items: RwSignal<Vec<VisibleKeytip>, LocalStorage>,
    positions: RwSignal<HashMap<u64, (f32, f32)>, LocalStorage>,
    next_items: Vec<VisibleKeytip>,
    next_positions: HashMap<u64, (f32, f32)>,
) {
    let positions_changed = positions.with_untracked(|cur| {
        cur.len() != next_positions.len()
            || next_positions.iter().any(|(id, (nx, ny))| {
                cur.get(id).is_none_or(|(cx, cy)| {
                    (nx - cx).abs() > POSITION_EPSILON || (ny - cy).abs() > POSITION_EPSILON
                })
            })
    });
    if positions_changed {
        positions.set(next_positions);
    }

    let items_changed = visible_items.with_untracked(|cur| {
        cur.len() != next_items.len()
            || cur
                .iter()
                .zip(&next_items)
                .any(|(a, b)| a.id != b.id || a.label != b.label)
    });
    if items_changed {
        visible_items.set(next_items);
    }
}

#[allow(clippy::too_many_arguments)]
fn schedule_keytip_frame(
    timers: &Timers,
    frame_slot: &Rc<RefCell<Option<FrameHandle>>>,
    host_ref: NodeRef,
    effective_held: Signal<Option<HeldModifier>, LocalStorage>,
    modal_scopes: RwSignal<Vec<u64>, LocalStorage>,
    hints: RwSignal<Vec<StagedHint>, LocalStorage>,
    visible_items: RwSignal<Vec<VisibleKeytip>, LocalStorage>,
    positions: RwSignal<HashMap<u64, (f32, f32)>, LocalStorage>,
) {
    if frame_slot.borrow().is_some() {
        return;
    }
    let timers_next = timers.clone();
    let slot_cb = Rc::clone(frame_slot);
    let handle = timers.request_frame(move |_at| {
        slot_cb.borrow_mut().take();
        let Some(held_mod) = effective_held.get_untracked() else {
            commit_keytip_sample(visible_items, positions, Vec::new(), HashMap::new());
            return;
        };
        let active_modals = modal_scopes.get_untracked();
        let staged = hints.get_untracked();
        let (next_items, next_positions) =
            sample_keytips(host_ref, held_mod, &active_modals, &staged, false);
        commit_keytip_sample(visible_items, positions, next_items, next_positions);
        schedule_keytip_frame(
            &timers_next,
            &slot_cb,
            host_ref,
            effective_held,
            modal_scopes,
            hints,
            visible_items,
            positions,
        );
    });
    *frame_slot.borrow_mut() = Some(handle);
}

/// In-flow `140ms` width-animated keytip suffix rendered beside text [`crate::ui::button::Button`] labels.
#[component]
pub fn KeytipSuffix(
    shortcut: Shortcut,
    #[prop(into, default = Signal::stored_local(true))] enabled: Signal<bool, LocalStorage>,
) -> impl IntoView {
    install_keytip_stylesheet();
    let registry = KeytipRegistry::current();
    let my_scope = DialogScopeId::current().map(|s| s.0);
    let last_label: RwSignal<Option<String>, LocalStorage> = RwSignal::new_local(None);

    let active_label = Signal::derive_local({
        let shortcut = shortcut.clone();
        move || {
            let reg = registry?;
            if !enabled.get() || !reg.is_scope_active(my_scope) {
                return None;
            }
            let held = reg.held().get()?;
            shortcut.compact_label(held)
        }
    });

    let cache_fx = RenderEffect::new(move |_| {
        if let Some(label) = active_label.get()
            && last_label.get_untracked().as_deref() != Some(label.as_str())
        {
            last_label.set(Some(label));
        }
    });
    on_cleanup_local(move || drop(cache_fx));

    let target = Signal::derive_local(move || {
        if active_label.get().is_some() {
            1.0
        } else {
            0.0
        }
    });
    let progress = use_inline_transition(target, SUFFIX_DURATION_MS);

    let inner_ref = NodeRef::new();
    let show = Signal::derive_local(move || {
        (progress.get() > 0.001 || active_label.get().is_some()) && last_label.get().is_some()
    });
    let observed_inner = inner_ref.observe_border_size_while(move || show.get());

    view! {
        Show(when = move || show.get()) {
            box(
                class = "gds-keytip-suffix",
                a11y:hidden = true,
                style:width = move || {
                    let scale = Gds::try_current()
                        .map_or(1.0, |gds| gds.appearance().typography().scaled(1.0));
                    let gap_px = (SUFFIX_GAP * scale).round();
                    let label_text = last_label.get().unwrap_or_default();
                    let fallback_width = estimated_badge_size(&label_text).width.0 + gap_px;
                    let total_width = observed_inner
                        .get()
                        .map(|sz| (sz.width.0 / inner_ref.scale().max(0.01)).ceil())
                        .filter(|&w| w > 1.0)
                        .unwrap_or(fallback_width);
                    let p = progress.get();
                    Some(format!("{:.2}px", (total_width * p).max(0.0)))
                },
            ) {
                box(
                    node_ref = inner_ref,
                    class = "gds-keytip-suffix__inner",
                    style:padding-left = move || {
                        let scale = Gds::try_current()
                            .map_or(1.0, |gds| gds.appearance().typography().scaled(1.0));
                        Some(format!("{:.1}px", (SUFFIX_GAP * scale).round()))
                    },
                    style:opacity = move || Some(format!("{:.3}", progress.get().clamp(0.0, 1.0))),
                    a11y:role = Role::Label,
                    a11y:label = move || last_label.get().unwrap_or_default(),
                ) {
                    text(class = "gds-keytip-badge") {{move || last_label.get().unwrap_or_default()}}
                }
            }
        }
    }
}

/// Provides per-window passive `Ctrl` / `Alt` hold-to-reveal state and renders collision-free
/// floating keytip badges (`OverlayLayer::Toast`, `pointer-events: none`).
#[component]
pub fn KeytipHost(
    #[prop(optional)] active: Option<RwSignal<bool, LocalStorage>>,
    children: Children,
) -> impl IntoView {
    install_keytip_stylesheet();

    let held = RwSignal::new_local(None);
    let active = active.unwrap_or_else(|| RwSignal::new_local(false));
    let suppressed_until_release = RwSignal::new_local(false);
    let hints = RwSignal::new_local(Vec::new());
    let initial_modals = FALLBACK_MODAL_SCOPES.with(|stack| stack.borrow().clone());
    let modal_scopes = RwSignal::new_local(initial_modals);
    let next_hint_id = RwSignal::new_local(1);
    let prefix = RwSignal::new_local(String::new());

    let registry = KeytipRegistry {
        held,
        active,
        suppressed_until_release,
        hints,
        modal_scopes,
        next_hint_id,
        prefix,
    };
    provide_local_context(registry);

    // If an external caller clears `active` to `false` (e.g. opening a menu overlay), suppress
    // the current hold until the next key transition without looping.
    let sync_active_fx = RenderEffect::new(move |prev_active: Option<bool>| {
        let now_active = active.get();
        if prev_active == Some(true)
            && !now_active
            && held.get_untracked().is_some()
            && modal_scopes.get_untracked().is_empty()
        {
            suppressed_until_release.set(true);
        }
        now_active
    });
    on_cleanup_local(move || drop(sync_active_fx));

    if let Some(window) = zgui::prelude::try_use_window() {
        let focus_edge_guard = window.on_focus_change(move |focused| {
            if !focused {
                registry.release();
            }
        });
        let window_focused = window.focused();
        let focus_watch = RenderEffect::new(move |_| {
            if !window_focused.get() {
                registry.release();
            }
        });
        on_cleanup_local(move || {
            drop(focus_edge_guard);
            drop(focus_watch);
        });
    }

    let host_ref = NodeRef::new();
    let root_guards: Rc<RefCell<Vec<ListenerGuard>>> = Rc::new(RefCell::new(Vec::new()));

    let listener_watch = {
        let root_guards = Rc::clone(&root_guards);
        RenderEffect::new(move |_| {
            if host_ref.get().is_none() || !root_guards.borrow().is_empty() {
                return;
            }
            let Some(root) = host_ref.window_root() else {
                return;
            };

            let mut installed = Vec::new();
            if let Some(guard) = root.listen(
                events::KEY_DOWN,
                ListenerOptions::CAPTURE,
                move |ev: &mut EventCx<'_, events::KeyDown>| {
                    let next = held_after_key_down(&ev.key, ev.modifiers);
                    registry.set_held(next);
                },
            ) {
                installed.push(guard);
            }

            if let Some(guard) = root.listen(
                events::KEY_UP,
                ListenerOptions::CAPTURE,
                move |ev: &mut EventCx<'_, events::KeyUp>| {
                    let next = held_after_key_up(&ev.key, ev.modifiers);
                    registry.set_held(next);
                },
            ) {
                installed.push(guard);
            }

            *root_guards.borrow_mut() = installed;
        })
    };

    on_cleanup_local(move || {
        drop(listener_watch);
        root_guards.borrow_mut().clear();
    });

    let effective_held = registry.held();
    let visible_items: RwSignal<Vec<VisibleKeytip>, LocalStorage> = RwSignal::new_local(Vec::new());
    let positions: RwSignal<HashMap<u64, (f32, f32)>, LocalStorage> =
        RwSignal::new_local(HashMap::new());
    let timers = Timers::current();
    let tracking_frame: Rc<RefCell<Option<FrameHandle>>> = Rc::new(RefCell::new(None));

    let tracking_fx = {
        let tracking_frame = Rc::clone(&tracking_frame);
        let timers = timers.clone();
        RenderEffect::new(move |_| {
            let Some(held_mod) = effective_held.get() else {
                let pending = tracking_frame.borrow_mut().take();
                drop(pending);
                commit_keytip_sample(visible_items, positions, Vec::new(), HashMap::new());
                return;
            };
            let active_modals = modal_scopes.get();
            let staged = hints.get();
            let (next_items, next_positions) =
                sample_keytips(host_ref, held_mod, &active_modals, &staged, true);
            commit_keytip_sample(visible_items, positions, next_items, next_positions);
            if let Some(ref timers) = timers {
                schedule_keytip_frame(
                    timers,
                    &tracking_frame,
                    host_ref,
                    effective_held,
                    modal_scopes,
                    hints,
                    visible_items,
                    positions,
                );
            }
        })
    };

    on_cleanup_local(move || {
        drop(tracking_fx);
        let pending = tracking_frame.borrow_mut().take();
        drop(pending);
    });

    view! {
        box(node_ref = host_ref, class = "gds-keytip-host") {
            {children.into_view_once()}
            Portal(layer = OverlayLayer::Toast) {
                Show(when = move || effective_held.get().is_some()) {
                    box(class = "gds-keytip-overlay") {
                        for item in move || visible_items.get(), key = |item| (item.id, item.label.clone()) {
                            {render_positioned_keytip(item, positions)}
                        }
                    }
                }
            }
        }
    }
}

/// Compact mnemonic badge rendered next to keytip-enabled controls.
#[component]
pub fn KeytipBadge(
    #[prop(into)] chord: String,
    #[prop(into, optional)] class: Classes,
) -> impl IntoView {
    install_keytip_stylesheet();
    view! {
        text(class = "gds-keytip-badge", class = class) {{chord}}
    }
}

#[cfg(test)]
mod live_tracking_tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use zgui::app::Fonts;
    use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
    use zgui::geom::{Device, DevicePx, Rect, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{LocalStorage, RwSignal, flush};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::view;
    use zgui::vocab::{
        KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, Role, Timestamp,
    };
    use zgui_platform_headless::Harness;

    use super::{EDGE_INSET, KeytipHostProps, KeytipRegistry, anchor};
    use crate::app::Gds;
    use crate::appearance::motion::{WORKSPACE_DURATION_MS, use_transition};
    use crate::service::task_manager::Scope;
    use crate::ui::shortcut::Shortcut;

    struct NoopSink;

    impl TextureSink for NoopSink {
        fn create_texture(
            &mut self,
            _: TextureId,
            _: Size<i32, Device>,
            _: TextureFormat,
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn write_texture(
            &mut self,
            _: TextureId,
            _: Rect<i32, Device>,
            _: TextureFormat,
            _: &[u8],
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn destroy_texture(&mut self, _: TextureId) {}
    }

    struct NoopRenderer {
        target: Option<RenderTarget>,
        sink: NoopSink,
    }

    impl Renderer for NoopRenderer {
        fn capabilities(&self) -> RenderCapabilities {
            RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.target
        }

        fn draw(&mut self, _: &zgui::scene::Scene, _: &zgui::bits::DamageSet) -> FrameOutcome {
            FrameOutcome::Presented(FrameStats::default())
        }

        fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
            TextureHandle(1)
        }

        fn release_external(&mut self, _: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::ZERO
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            &mut self.sink
        }
    }

    fn noop_renderer(
        _: &Arc<dyn Surface>,
        target: RenderTarget,
    ) -> Result<Box<dyn Renderer>, AppError> {
        Ok(Box::new(NoopRenderer {
            target: Some(target),
            sink: NoopSink,
        }))
    }

    #[derive(Clone, Debug)]
    struct A11yEntry {
        role: Role,
        label: Option<String>,
        bounds: Option<(f64, f64, f64, f64)>,
        tx: f64,
        children: Vec<String>,
    }

    struct A11yMirror {
        root: Option<String>,
        nodes: HashMap<String, A11yEntry>,
    }

    impl A11yMirror {
        fn new() -> Self {
            Self {
                root: None,
                nodes: HashMap::new(),
            }
        }

        fn sync(&mut self, surface: &zgui_platform_headless::OffscreenSurface) {
            for update in surface.take_a11y_log() {
                if let Some(tree) = update.tree {
                    self.root = Some(format!("{:?}", tree.root));
                }
                for (id, node) in update.nodes {
                    let tx = node.transform().map_or(0.0, |aff| aff.as_coeffs()[4]);
                    let bounds = node.bounds().map(|b| (b.x0, b.y0, b.x1, b.y1));
                    let children = node.children().iter().map(|c| format!("{c:?}")).collect();
                    self.nodes.insert(
                        format!("{id:?}"),
                        A11yEntry {
                            role: node.role(),
                            label: node.label().map(str::to_owned),
                            bounds,
                            tx,
                            children,
                        },
                    );
                }
            }
        }

        fn label_right_edge(&self, expected_label: &str) -> Option<f32> {
            let root_id = self.root.as_ref()?;
            Self::find_right_edge(&self.nodes, root_id, 0.0, expected_label)
        }

        fn find_right_edge(
            nodes: &HashMap<String, A11yEntry>,
            id: &str,
            accum_tx: f64,
            expected_label: &str,
        ) -> Option<f32> {
            let entry = nodes.get(id)?;
            let next_tx = accum_tx + entry.tx;
            if entry.role == Role::Label
                && entry.label.as_deref() == Some(expected_label)
                && let Some((_, _, x1, _)) = entry.bounds
            {
                return Some((next_tx + x1) as f32);
            }
            for child in &entry.children {
                if let Some(found) = Self::find_right_edge(nodes, child, next_tx, expected_label) {
                    return Some(found);
                }
            }
            None
        }
    }

    #[test]
    fn floating_keytip_badge_tracks_animated_target_every_frame_and_idles_on_release() {
        let x_slot: Rc<RefCell<Option<RwSignal<f32, LocalStorage>>>> = Rc::new(RefCell::new(None));
        let smooth_slot: Rc<RefCell<Option<RwSignal<f32, LocalStorage>>>> =
            Rc::new(RefCell::new(None));
        let target_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
        let smooth_target_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
        let reg_slot: Rc<RefCell<Option<KeytipRegistry>>> = Rc::new(RefCell::new(None));

        let x_driver = Rc::clone(&x_slot);
        let smooth_driver = Rc::clone(&smooth_slot);
        let target_driver = Rc::clone(&target_slot);
        let smooth_target_driver = Rc::clone(&smooth_target_slot);
        let reg_driver = Rc::clone(&reg_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(640.0),
                DevicePx(240.0),
            )));
            harness.settle(16);

            let surface = harness.platform().offscreens()[0].clone();
            let mut a11y = A11yMirror::new();
            a11y.sync(&surface);

            // 1. Idle when Ctrl/Alt is NOT held: zero request_frame callbacks and 0% CPU.
            harness.reset_counts();
            let idle_frames =
                harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            assert_eq!(
                idle_frames, 0,
                "KeytipHost must schedule zero frames when no modifier is held"
            );

            let x_sig = x_driver.borrow().expect("x signal");
            let smooth_sig = smooth_driver.borrow().expect("smooth signal");
            let target_ref = target_driver.borrow().expect("target ref");
            let smooth_target_ref = smooth_target_driver.borrow().expect("smooth target ref");
            let reg = reg_driver.borrow().expect("registry");

            let badge_ref_for = |label_str: &str| -> NodeRef {
                reg.hints
                    .get_untracked()
                    .into_iter()
                    .find(|h| {
                        h.shortcut
                            .compact_label(crate::ui::shortcut::HeldModifier::Ctrl)
                            == Some(label_str.to_owned())
                    })
                    .expect("staged hint")
                    .badge_ref
            };

            // 2. Hold Ctrl -> Frame 0 immediately places badge on X=40px without (0,0) flash.
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: KeyEvent::named(NamedKey::Control, PhysicalKey::Code(KeyCode::ControlLeft)),
                modifiers: Modifiers::CONTROL,
                timestamp: Timestamp::from_origin(Duration::from_millis(200)),
            });
            harness.settle(8);
            a11y.sync(&surface);

            let badge_i = badge_ref_for("I");
            let badge_b = badge_ref_for("B");

            let assert_locked = |ctrl: NodeRef,
                                 badge: NodeRef,
                                 a11y_tree: &A11yMirror,
                                 label_txt: &str,
                                 expected_ctrl_x: Option<f32>| {
                let scale = ctrl.scale().max(1.0);
                let ctrl_b = ctrl.window_bounds().expect("control bounds");
                let badge_b = badge.window_bounds().expect("badge bounds");
                let ctrl_x = ctrl_b.origin.x.0 / scale;
                let ctrl_right = (ctrl_b.origin.x.0 + ctrl_b.size.width.0) / scale;
                let badge_right = (badge_b.origin.x.0 + badge_b.size.width.0) / scale;
                if let Some(exp_x) = expected_ctrl_x {
                    assert!(
                        (ctrl_x - exp_x).abs() <= 0.5,
                        "expected control X={exp_x}, got {ctrl_x}"
                    );
                }
                let expected_badge_right = ctrl_right - EDGE_INSET;
                assert!(
                    (badge_right - expected_badge_right).abs() <= 1.5,
                    "badge '{label_txt}' window_bounds right={badge_right} lagged behind control right-inset={expected_badge_right}"
                );
                let a11y_right = a11y_tree
                    .label_right_edge(label_txt)
                    .expect("AT-SPI keytip label node");
                assert!(
                    (a11y_right - expected_badge_right).abs() <= 1.5,
                    "badge '{label_txt}' AT-SPI right={a11y_right} lagged behind control right-inset={expected_badge_right}"
                );
            };

            // Verify Frame 0 at X = 40px, then step across frames 40px -> 160px -> 320px!
            assert_locked(target_ref, badge_i, &a11y, "I", Some(40.0));

            for next_x in [160.0_f32, 320.0_f32] {
                x_sig.set(next_x);
                flush();
                harness.run_for(Duration::from_millis(16), Duration::from_millis(16));
                a11y.sync(&surface);
                assert_locked(target_ref, badge_i, &a11y, "I", Some(next_x));
            }

            // 3. Continuous 160ms `use_transition` animation (40px -> 320px) while Ctrl is held:
            //    verify badge stays locked onto the moving control on EVERY SINGLE 16ms frame!
            smooth_sig.set(320.0);
            flush();
            let mut prev_x = 40.0_f32;
            for step_idx in 0..12 {
                harness.run_for(Duration::from_millis(16), Duration::from_millis(16));
                a11y.sync(&surface);
                let cur_x = smooth_target_ref
                    .window_bounds()
                    .expect("smooth bounds")
                    .origin
                    .x
                    .0
                    / smooth_target_ref.scale().max(1.0);
                if step_idx == 0 {
                    assert!(
                        cur_x > prev_x + 5.0,
                        "first animation frame must advance immediately past {prev_x}, got {cur_x}"
                    );
                }
                assert!(cur_x >= prev_x - 0.1);
                prev_x = cur_x;
                assert_locked(smooth_target_ref, badge_b, &a11y, "B", None);
            }
            assert!((prev_x - 320.0).abs() <= 0.5);

            // 4. Release Ctrl -> immediately cancels pending FrameHandle and parks idle at 0% CPU.
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Released,
                event: KeyEvent::named(NamedKey::Control, PhysicalKey::Code(KeyCode::ControlLeft)),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(500)),
            });
            harness.settle(8);
            assert!(badge_i.window_bounds().is_none());
            harness.reset_counts();
            let post_release_frames =
                harness.run_for(Duration::from_secs(2), Duration::from_millis(16));
            assert_eq!(
                post_release_frames, 0,
                "releasing Ctrl must cancel FrameHandle immediately with zero extra frames"
            );
            assert_eq!(harness.parked_deadline(), None);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(640.0, 240.0)
            .run_on(driver, move || {
                let _gds = Gds::install(Scope::default());
                let x_sig = RwSignal::new_local(40.0_f32);
                let smooth_goal = RwSignal::new_local(40.0_f32);
                let smooth_x = use_transition(smooth_goal.into(), WORKSPACE_DURATION_MS);
                let target_ref = NodeRef::new();
                let smooth_target_ref = NodeRef::new();

                *x_slot.borrow_mut() = Some(x_sig);
                *smooth_slot.borrow_mut() = Some(smooth_goal);
                *target_slot.borrow_mut() = Some(target_ref);
                *smooth_target_slot.borrow_mut() = Some(smooth_target_ref);

                let reg_slot = Rc::clone(&reg_slot);
                view! {
                    KeytipHost {
                        {{
                            *reg_slot.borrow_mut() = KeytipRegistry::current();
                            anchor(target_ref, Shortcut::parse("Ctrl+I").expect("Ctrl+I"));
                            anchor(smooth_target_ref, Shortcut::parse("Ctrl+B").expect("Ctrl+B"));
                            view! {
                                box(style:position = "relative", style:width = "640px", style:height = "240px") {
                                    box(
                                        node_ref = target_ref,
                                        style:position = "absolute",
                                        style:top = "24px",
                                        style:left = move || Some(format!("{:.2}px", x_sig.get())),
                                        style:width = "32px",
                                        style:height = "32px",
                                    ) {}
                                    box(
                                        node_ref = smooth_target_ref,
                                        style:position = "absolute",
                                        style:top = "96px",
                                        style:left = move || Some(format!("{:.2}px", smooth_x.get())),
                                        style:width = "32px",
                                        style:height = "32px",
                                    ) {}
                                }
                            }
                        }}
                    }
                }
            })
            .expect("live keytip tracking headless test ran");
    }

    #[test]
    fn inline_suffix_first_frame_pushes_sibling_floating_keytip_and_skips_offscreen_targets() {
        use crate::ui::button::{ButtonProps, IconButtonProps};
        use crate::ui::icon::IconName;

        let btn_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
        let icon_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
        let offscreen_x_slot: Rc<RefCell<Option<RwSignal<f32, LocalStorage>>>> =
            Rc::new(RefCell::new(None));
        let reg_slot: Rc<RefCell<Option<KeytipRegistry>>> = Rc::new(RefCell::new(None));

        let btn_driver = Rc::clone(&btn_slot);
        let icon_driver = Rc::clone(&icon_slot);
        let offscreen_x_driver = Rc::clone(&offscreen_x_slot);
        let reg_driver = Rc::clone(&reg_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(480.0),
                DevicePx(160.0),
            )));
            harness.settle(16);
            harness.advance(Duration::from_millis(32));
            harness.settle(8);

            let btn_ref = btn_driver.borrow().expect("btn ref");
            let icon_ref = icon_driver.borrow().expect("icon ref");
            let offscreen_x = offscreen_x_driver.borrow().expect("offscreen x");
            let reg = reg_driver.borrow().expect("registry");

            let scale = btn_ref.scale().max(1.0);
            let initial_btn_w = btn_ref.window_bounds().expect("btn bounds").size.width.0 / scale;

            let badge_for = |label_str: &str| -> NodeRef {
                reg.hints
                    .get_untracked()
                    .into_iter()
                    .find(|h| {
                        h.shortcut
                            .compact_label(crate::ui::shortcut::HeldModifier::Ctrl)
                            == Some(label_str.to_owned())
                    })
                    .expect("staged hint")
                    .badge_ref
            };

            // Hold Ctrl: Frame 1 (16ms) must immediately expand KeytipSuffix past initial_btn_w
            // (zero 1-frame dead stall), and the sibling IconButton's floating 'I' badge must
            // follow IconButton's right edge on every single frame! Meanwhile offscreen 'O' at
            // x=-200px does not intersect window content and must not mount a visible badge.
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: KeyEvent::named(NamedKey::Control, PhysicalKey::Code(KeyCode::ControlLeft)),
                modifiers: Modifiers::CONTROL,
                timestamp: Timestamp::from_origin(Duration::from_millis(32)),
            });
            harness.settle(8);

            let badge_i = badge_for("I");
            let badge_o = badge_for("O");
            assert!(
                badge_o.window_bounds().is_none(),
                "offscreen target outside window content must not render a clamped keytip badge"
            );

            for frame_idx in 0..10 {
                harness.run_for(Duration::from_millis(16), Duration::from_millis(16));
                let cur_btn_w = btn_ref.window_bounds().expect("btn bounds").size.width.0 / scale;
                if frame_idx == 0 {
                    assert!(
                        cur_btn_w > initial_btn_w + 1.0,
                        "KeytipSuffix first frame must advance immediately past {initial_btn_w}, got {cur_btn_w}"
                    );
                }
                let icon_b = icon_ref.window_bounds().expect("icon bounds");
                let badge_b = badge_i.window_bounds().expect("badge I bounds");
                let icon_right = (icon_b.origin.x.0 + icon_b.size.width.0) / scale;
                let badge_right = (badge_b.origin.x.0 + badge_b.size.width.0) / scale;
                assert!(
                    (badge_right - (icon_right - EDGE_INSET)).abs() <= 1.5,
                    "frame {frame_idx}: sibling badge right={badge_right} lagged behind icon right-inset={}",
                    icon_right - EDGE_INSET
                );
            }

            // Bring 'O' back onscreen -> badge mounts and locks onto x=120px!
            offscreen_x.set(120.0);
            flush();
            harness.run_for(Duration::from_millis(16), Duration::from_millis(16));
            let badge_o_b = badge_o
                .window_bounds()
                .expect("onscreen target must render badge");
            let badge_o_right = (badge_o_b.origin.x.0 + badge_o_b.size.width.0) / scale;
            assert!(
                (badge_o_right - (120.0 + 32.0 - EDGE_INSET)).abs() <= 1.5,
                "restored onscreen badge right={badge_o_right}"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(480.0, 160.0)
            .run_on(driver, move || {
                let _gds = Gds::install(Scope::default());
                let btn_ref = NodeRef::new();
                let icon_ref = NodeRef::new();
                let offscreen_ref = NodeRef::new();
                let offscreen_x = RwSignal::new_local(-200.0_f32);

                *btn_slot.borrow_mut() = Some(btn_ref);
                *icon_slot.borrow_mut() = Some(icon_ref);
                *offscreen_x_slot.borrow_mut() = Some(offscreen_x);

                let reg_slot = Rc::clone(&reg_slot);
                view! {
                    KeytipHost {
                        {{
                            *reg_slot.borrow_mut() = KeytipRegistry::current();
                            anchor(offscreen_ref, Shortcut::parse("Ctrl+O").expect("Ctrl+O"));
                            view! {
                                box(style:position = "relative", style:width = "480px", style:height = "160px") {
                                    row(style:gap = "8px", style:align-items = "center") {
                                        Button(
                                            node_ref = btn_ref,
                                            shortcut = Shortcut::parse("Ctrl+R").expect("Ctrl+R"),
                                        ) { "Run" }
                                        IconButton(
                                            node_ref = icon_ref,
                                            icon = IconName::Info,
                                            label = "Details",
                                            shortcut = Shortcut::parse("Ctrl+I").expect("Ctrl+I"),
                                        )
                                    }
                                    box(
                                        node_ref = offscreen_ref,
                                        style:position = "absolute",
                                        style:top = "80px",
                                        style:left = move || Some(format!("{:.1}px", offscreen_x.get())),
                                        style:width = "32px",
                                        style:height = "32px",
                                    ) {}
                                }
                            }
                        }}
                    }
                }
            })
            .expect("inline suffix + offscreen clipping test ran");
    }
}
