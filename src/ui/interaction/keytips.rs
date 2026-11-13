//! Held-modifier shortcut hints.
//!
//! Controls that own a binding stage it at their own bounds while they are on
//! screen. This window-level layer paints those bindings on their controls only
//! while Ctrl or Alt is held, and only where a press would really run the action
//! from the focused element, so a label never advertises a key that would not
//! work here. The held modifier is left out of its own label: the press already
//! shows it, and `Ctrl+R` reads `R`.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
    time::Duration,
};

use gpui_kit::base::{Root, RootPlugin};
use gpui_kit::component::{ActiveTheme, window_paddings};
use gpui_kit::{IntoElement, prelude::*, *};

use crate::app::AppExt as _;
use crate::ui::{HeldModifier, Shortcut, ShortcutBadge};

/// Horizontal padding of a compact label.
const LABEL_PADDING_X: f32 = 4.0;
/// Vertical padding of a compact label; its height comes from the text line
/// plus the border, which keeps it around 16–19px at the default text size.
const LABEL_PADDING_Y: f32 = 1.0;
const LABEL_RADIUS: f32 = 4.0;
/// Label text size before the user's text scale is applied: small enough to sit
/// on a control without hiding its icon or its label.
const LABEL_TEXT_SIZE: f32 = 10.0;
/// Line height relative to the label text, tight enough that the label stays a
/// slim tag instead of a pill.
const LABEL_LINE_HEIGHT: f32 = 1.2;
/// Distance from a control's inside edges to its corner label.
const EDGE_INSET: Pixels = px(1.);
const SUFFIX_DURATION: Duration = Duration::from_millis(140);
const SUFFIX_GAP: f32 = 6.;
/// Free space kept between a label and the edge it is attached to.
const LABEL_GAP: Pixels = px(2.);
/// Distance kept between a label and the window edge when it has to move.
const EDGE_MARGIN: Pixels = px(4.);
/// Free space kept between two labels.
const LABEL_SEPARATION: Pixels = px(2.);
/// Distance kept between a label and its row's trailing edge.
const TRAILING_INSET: Pixels = px(4.);
/// Rows a label may try on either side of its control before it gives up its
/// edge. Each row is one label tall.
const MAX_ROWS: usize = 8;
/// Sideways offsets tried on a control's edge, closest to the trailing corner
/// first. They let a neighbour's label pass without pushing this one off its
/// own control or down to another row.
const NUDGES: [f32; 9] = [0., 2., -2., 4., -4., 6., -6., 8., -8.];

/// Paint order of the hint layer.
///
/// `gpui-base` paints each dialog in a deferred draw at `10 + dialog layer`, and
/// popups and tooltips at 100 and 200, so a plain overlay registered as a root
/// plugin is always painted under a dialog and its backdrop. The hints belong
/// above every dialog and below the surfaces the pointer owns.
const LAYER_PRIORITY: usize = 90;

/// The per-window hint layer, registered as a root plugin so it draws above the
/// application content and the dialog layer without affecting their layout.
pub struct Keytips {
    this: WeakEntity<Self>,
    staged: Rc<RefCell<Staged>>,
    last_rendered: Option<RenderSnapshot>,
    watcher_armed: bool,
    #[cfg(all(test, feature = "gui-tests"))]
    renders: usize,
    _activation: Subscription,
}

#[derive(Clone, PartialEq)]
struct RenderSnapshot {
    focus: Option<gpui_kit::FocusHandle>,
    content: Bounds<Pixels>,
    held: Option<HeldModifier>,
    generation: u64,
}

impl RenderSnapshot {
    fn capture(
        focus: Option<&gpui_kit::FocusHandle>,
        content: Bounds<Pixels>,
        staged: &Staged,
    ) -> Self {
        Self {
            focus: focus.cloned(),
            content,
            held: staged.held,
            generation: staged.generation,
        }
    }
}

#[derive(Default)]
struct Staged {
    held: Option<HeldModifier>,
    hints: HashMap<ElementId, Hint>,
    generation: u64,
}

struct Hint {
    shortcut: Shortcut,
    bounds: Bounds<Pixels>,
    placement: HintPlacement,
    /// Dropped together with the element that staged this hint; the layer then
    /// stops showing it without a timer or a per-frame sweep.
    mounted: Weak<()>,
}

/// Where a label prefers to sit relative to its control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HintPlacement {
    /// In an icon control's own top trailing corner, inside the control, so the
    /// label reads as a badge on that control.
    Corner,
    /// In a row control's free trailing space, vertically centred. Row controls
    /// leave that space empty, which keeps their label off the row below.
    Trailing,
    /// In a row control's free trailing space, ending before the control that
    /// owns the row's trailing edge (a close button) together with the width it
    /// occupies, so the row's own label never covers that control's corner
    /// badge. Several labels for one row stack leftwards.
    TrailingBefore(Pixels),
    /// In a compact row's bottom leading corner. Its top trailing corner holds a
    /// close control, so this is the free corner that keeps a short label on the
    /// row instead of dropping it when that control shows.
    BottomLeadingCorner,
}

/// One measured label waiting for a position.
#[derive(Clone, Copy, Debug)]
struct LabelSlot {
    anchor: Bounds<Pixels>,
    size: Size<Pixels>,
    placement: HintPlacement,
}

/// The side of its control a label hangs on when it leaves its corner or tail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Side {
    Below,
    Above,
}

impl Keytips {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                let changed = this.staged.borrow_mut().release();
                if changed {
                    cx.notify();
                    window.refresh();
                }
            }
        });
        Self {
            this: cx.entity().downgrade(),
            staged: Rc::new(RefCell::new(Staged::default())),
            last_rendered: None,
            watcher_armed: false,
            #[cfg(all(test, feature = "gui-tests"))]
            renders: 0,
            _activation: activation,
        }
    }

    /// Arm a lightweight next-frame check while a modifier group is held so
    /// focus, viewport bounds, or hint changes are detected without running a
    /// full `render()` and label collision pass on idle frames.
    fn arm_watcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.watcher_armed || self.staged.borrow().held.is_none() {
            return;
        }
        self.watcher_armed = true;
        cx.on_next_frame(window, |this, window, cx| {
            this.watcher_armed = false;
            this.check_next_frame(window, cx);
        });
    }

    fn check_next_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = {
            let mut staged = self.staged.borrow_mut();
            staged.prune();
            RenderSnapshot::capture(window.focused(cx).as_ref(), content_bounds(window), &staged)
        };
        if self.last_rendered.as_ref() != Some(&snapshot) {
            cx.notify();
        } else if snapshot.held.is_some() {
            self.arm_watcher(window, cx);
        }
    }
}

/// Drop any hint that is on screen.
///
/// The plugin also clears on window deactivation, which may lose the matching
/// modifier release. Callers can explicitly clear during other focus transitions.
pub(crate) fn release(window: &mut Window, cx: &mut App) {
    let Some(layer) = layer(window, cx) else {
        return;
    };
    let id = layer.entity_id();
    let staged = layer.read(cx).staged.clone();
    let mut staged = staged.borrow_mut();
    let changed = staged.release();
    drop(staged);
    if changed {
        cx.notify(id);
        window.refresh();
    }
}

struct Initialized;
impl Global for Initialized {}

/// Register the hint layer once for all application windows.
///
/// `Gds::initialize` calls this at startup after `gpui_kit::init`. The layer
/// paints itself in its own deferred layer, so its registration order no
/// longer decides whether it covers a dialog.
pub(crate) fn install(cx: &mut App) {
    if cx.has_global::<Initialized>() {
        return;
    }
    cx.set_global(Initialized);
    Root::register_plugin::<Keytips>(cx, Keytips::new);
}

impl RootPlugin for Keytips {
    fn decorate(
        &self,
        surface: AnyElement,
        _root: &Root,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement {
        let this = self.this.clone();
        let staged = self.staged.clone();
        div()
            .size_full()
            .on_modifiers_changed(move |event, window, cx| {
                // Only one group is ever armed. Shift does not change it; a
                // Ctrl+Alt mix (AltGr, for one) arms neither, so neither
                // group's labels can light up for the other's press.
                let held = HeldModifier::from_modifiers(event.modifiers);
                if staged.borrow_mut().set_held(held)
                    && let Some(layer) = this.upgrade()
                {
                    cx.notify(layer.entity_id());
                    window.refresh();
                }
            })
            .child(surface)
    }
}

impl Render for Keytips {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(all(test, feature = "gui-tests"))]
        {
            self.renders += 1;
        }
        let focus = window.focused(cx);
        let content = content_bounds(window);
        let mut staged = self.staged.borrow_mut();
        staged.prune();
        self.last_rendered = Some(RenderSnapshot::capture(focus.as_ref(), content, &staged));
        let mut slots = Vec::new();
        let mut labels = Vec::new();
        if let Some(held) = staged.held
            && let Some(focus) = focus.as_ref()
        {
            let mut placed: Vec<_> = staged
                .hints
                .iter()
                .filter_map(|(key, hint)| {
                    let bindings = vec![hint.active_binding(window, cx, focus, held)?];
                    Some((
                        hint.bounds,
                        hint.placement,
                        hint.shortcut.action().name(),
                        bindings,
                        key.clone(),
                    ))
                })
                .collect();
            // Hash order is arbitrary; a stable visual order keeps neighbouring
            // labels from swapping which one is on top between frames. It is
            // also the order the layout packs rows in, top to bottom, and the
            // order a row with several labels fills its free space in.
            placed.sort_by(|left, right| {
                let order = f32::from(left.0.origin.y).total_cmp(&f32::from(right.0.origin.y));
                order
                    .then(f32::from(left.0.origin.x).total_cmp(&f32::from(right.0.origin.x)))
                    .then_with(|| left.2.cmp(right.2))
            });
            for (anchor, placement, _, bindings, key) in placed {
                slots.push(LabelSlot {
                    anchor,
                    placement,
                    // Measured from the real element in `HintLayer::prepaint`.
                    size: Size::default(),
                });
                labels.push(compact_label(key, &bindings, held, cx).into_any_element());
            }
        }
        let armed = staged.held.is_some();
        drop(staged);
        if armed {
            self.arm_watcher(window, cx);
        }
        deferred(
            div()
                .absolute()
                .inset_0()
                .child(HintLayer::new(content, slots, labels)),
        )
        .with_priority(LAYER_PRIORITY)
    }
}

impl Hint {
    /// The key a press would run right now: the focused scope must bind the
    /// action to a single keystroke *and* a handler for it must be on the
    /// dispatch path.
    ///
    /// Resolving the binding alone is not enough. A global command such as the
    /// sidebar toggle keeps a binding in every scope, including a dialog's, but
    /// its handler sits outside the dialog and never receives the key, so the
    /// label would promise something that does not work.
    fn active_binding(
        &self,
        window: &Window,
        cx: &mut App,
        focus: &FocusHandle,
        held: HeldModifier,
    ) -> Option<KeyBinding> {
        let handled = window.is_action_available(self.shortcut.action(), cx);
        let binding =
            window.highest_precedence_binding_for_action_in(self.shortcut.action(), focus);
        visible_binding(handled, binding, held)
    }
}

/// The binding a label may show for a staged control under `held`.
///
/// The control's key must carry the held modifier. A binding without it, such
/// as `F2` or `F5`, is not what this press is hinting at, and neither is one
/// that only carries the other group's modifier.
fn visible_binding(
    handled: bool,
    binding: Option<KeyBinding>,
    held: HeldModifier,
) -> Option<KeyBinding> {
    let binding = binding?;
    let [stroke] = binding.keystrokes() else {
        return None;
    };
    (handled && held.matches(stroke.as_keystroke().modifiers)).then_some(binding)
}

impl Staged {
    fn release(&mut self) -> bool {
        self.set_held(None)
    }

    /// Drop hints whose staging element left the tree. Returns whether any
    /// dead entry was removed.
    fn prune(&mut self) -> bool {
        let before = self.hints.len();
        self.hints.retain(|_, hint| hint.mounted.strong_count() > 0);
        let removed = self.hints.len() != before;
        if removed {
            self.generation += 1;
        }
        removed
    }

    /// Arm one modifier group, or disarm both. Returns whether the layer needs
    /// a frame to show the change.
    fn set_held(&mut self, held: Option<HeldModifier>) -> bool {
        if self.held == held {
            return false;
        }
        self.held = held;
        if held.is_none() {
            self.hints.clear();
        }
        self.generation += 1;
        true
    }

    /// Record a control's binding and bounds. Returns whether the layer needs a
    /// frame to show the change; it does not while no group is armed.
    fn stage(
        &mut self,
        key: ElementId,
        shortcut: Shortcut,
        bounds: Bounds<Pixels>,
        placement: HintPlacement,
        mounted: Weak<()>,
    ) -> bool {
        if let Some(hint) = self.hints.get_mut(&key)
            && hint.mounted.strong_count() > 0
            && hint.bounds == bounds
            && hint.placement == placement
            && hint.shortcut.same_scope(&shortcut)
        {
            if !Weak::ptr_eq(&hint.mounted, &mounted) {
                hint.mounted = mounted;
            }
            return false;
        }
        self.hints.insert(
            key,
            Hint {
                shortcut,
                bounds,
                placement,
                mounted,
            },
        );
        self.generation += 1;
        self.held.is_some()
    }
}

/// The label layer of the window's root, when one is mounted.
fn layer(window: &Window, cx: &App) -> Option<Entity<Keytips>> {
    window.root::<Root>()??.read(cx).plugin::<Keytips>()
}

/// An out-of-flow anchor for custom controls whose parent render has no window argument.
#[derive(IntoElement)]
pub struct Anchor {
    key: ElementId,
    shortcut: Shortcut,
    placement: HintPlacement,
}

impl Anchor {
    pub fn new(key: impl Into<ElementId>, shortcut: impl Into<Shortcut>) -> Self {
        Self {
            key: key.into(),
            shortcut: shortcut.into(),
            placement: HintPlacement::Corner,
        }
    }

    pub fn trailing_before(mut self, reserved: Pixels) -> Self {
        self.placement = HintPlacement::TrailingBefore(reserved);
        self
    }

    pub fn bottom_corner(mut self) -> Self {
        self.placement = HintPlacement::BottomLeadingCorner;
        self
    }
}

impl RenderOnce for Anchor {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        staged_anchor(window, cx, self.key, self.shortcut, self.placement)
    }
}

/// Stage one control's binding as a corner badge at its own bounds.
///
/// Icon controls put the label in their own top trailing corner, inside the
/// control, so it reads as a badge on that icon rather than a tag floating
/// below it. The returned element is out of flow and inert: it neither occupies
/// layout nor intercepts input, so showing a control's binding never moves or
/// blocks the control. It is empty when the window has no hint layer, which
/// keeps unit tests that render controls without a window root working
/// unchanged.
pub fn anchor(window: &mut Window, cx: &mut App, key: ElementId, shortcut: Shortcut) -> AnyElement {
    staged_anchor(window, cx, key, shortcut, HintPlacement::Corner)
}

/// Stage a binding in a row control's trailing space.
///
/// Navigation and page rows are full width and leave their trailing edge empty,
/// so their label fits inside the row instead of covering the row below it. A
/// row too narrow for the label, such as a collapsed sidebar item, falls back to
/// [`anchor`]'s placement.
pub fn row_anchor(
    window: &mut Window,
    cx: &mut App,
    key: ElementId,
    shortcut: Shortcut,
) -> AnyElement {
    staged_anchor(window, cx, key, shortcut, HintPlacement::Trailing)
}

/// Stage a binding in a row control's trailing space, in front of the control
/// that owns the row's trailing edge.
///
/// `reserved` is the width that control occupies, measured from the row's
/// trailing edge; it belongs to that control's own badge, so the row's label
/// ends before it. Rows that also have room for a second label, such as a
/// two-tab strip whose two directions land on the same row, line both up to the
/// left instead of covering each other.
pub fn trailing_before_anchor(
    window: &mut Window,
    cx: &mut App,
    key: ElementId,
    shortcut: Shortcut,
    reserved: Pixels,
) -> AnyElement {
    staged_anchor(
        window,
        cx,
        key,
        shortcut,
        HintPlacement::TrailingBefore(reserved),
    )
}

/// Stage a binding in a compact row's bottom leading corner.
///
/// A collapsed row keeps its close control in its top trailing corner, so its
/// keys still show on the free corner instead of disappearing with the space
/// they would have used.
pub fn bottom_corner_anchor(
    window: &mut Window,
    cx: &mut App,
    key: ElementId,
    shortcut: Shortcut,
) -> AnyElement {
    staged_anchor(
        window,
        cx,
        key,
        shortcut,
        HintPlacement::BottomLeadingCorner,
    )
}

/// An in-flow hint that grows from zero width while its modifier is held.
/// Place it beside the label in a gapless row; it animates its own spacing.
pub fn suffix_anchor(
    window: &mut Window,
    cx: &mut App,
    key: ElementId,
    shortcut: Shortcut,
) -> AnyElement {
    use gds_uim::el;
    use gpui_kit::base::motion::{self, Transition};

    let held = layer(window, cx).and_then(|layer| layer.read(cx).staged.borrow().held);
    let binding = held.and_then(|held| {
        let focus = window.focused(cx)?;
        let handled = window.is_action_available(shortcut.action(), cx);
        visible_binding(
            handled,
            window.highest_precedence_binding_for_action_in(shortcut.action(), &focus),
            held,
        )
        .map(|binding| (binding, held))
    });
    // Keep the last label while its space closes, including on a quick reversal.
    let state = window.use_keyed_state((key.clone(), "suffix-label"), cx, |_, _| {
        None::<(KeyBinding, HeldModifier)>
    });
    if let Some(binding) = binding.as_ref() {
        state.update(cx, |value, _| *value = Some(binding.clone()));
    }
    let progress = motion::transition(
        (key.clone(), "suffix-reveal"),
        if binding.is_some() { 1.0_f32 } else { 0.0 },
        Transition::new(SUFFIX_DURATION),
        window,
        cx,
    );
    let Some((binding, held)) = state.read(cx).clone() else {
        return div().size_0().into_any_element();
    };
    let label = ShortcutBadge::path_label(&[binding.clone()], held).unwrap_or_default();
    let typography = cx.gds_ro().appearance().typography();
    let text_size = typography.scaled(LABEL_TEXT_SIZE);
    let run = window.text_style().to_run(label.len());
    let width = window
        .text_system()
        .shape_line(label, text_size, &[run], None)
        .width
        + px(LABEL_PADDING_X * 2. + 2.);
    // Text layout rounds glyph advances up; the clip must include the entire border.
    let width = width.ceil();
    let gap = typography.scaled(SUFFIX_GAP);
    let height = (text_size * LABEL_LINE_HEIGHT + px(LABEL_PADDING_Y * 2. + 2.)).ceil();
    let label = compact_label(key, &[binding], held, cx).w_full().h_full();
    el! {
        .div { flex; items_center; flex_shrink_0;
            when: $(progress < 1.0), { overflow_hidden };
            w: $((width + gap) * progress); h: height;
        } [
            .div { flex; flex_shrink_0; ml: gap; w: width; h: height; opacity: progress; } [label],
        ]
    }
    .into_any_element()
}

fn staged_anchor(
    window: &mut Window,
    cx: &mut App,
    key: ElementId,
    shortcut: Shortcut,
    placement: HintPlacement,
) -> AnyElement {
    let Some(layer) = layer(window, cx) else {
        return div().absolute().size_0().into_any_element();
    };
    let staged = layer.read(cx).staged.clone();
    let owner = layer.downgrade();
    let mounted = window.use_keyed_state(key.clone(), cx, |_, _| Rc::new(()));
    let mounted = Rc::downgrade(mounted.read(cx));
    canvas(
        move |bounds, _, cx| {
            let changed = staged
                .borrow_mut()
                .stage(key, shortcut, bounds, placement, mounted);
            if changed && let Some(layer) = owner.upgrade() {
                cx.notify(layer.entity_id());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

/// The window area a label may use: the content inside the client frame, which
/// a shadow-padded window keeps clear of its decorative border.
fn content_bounds(window: &Window) -> Bounds<Pixels> {
    let padding = window_paddings(window);
    let viewport = window.viewport_size();
    Bounds::new(
        point(padding.left, padding.top),
        size(
            (viewport.width - padding.left - padding.right).max(px(0.)),
            (viewport.height - padding.top - padding.bottom).max(px(0.)),
        ),
    )
}

/// The labels of one frame, positioned once their real sizes are known.
///
/// Measuring the rendered labels instead of assuming a text width keeps the
/// layout correct at every text size, including the largest one.
struct HintLayer {
    content: Bounds<Pixels>,
    slots: Vec<LabelSlot>,
    labels: Vec<AnyElement>,
}

impl HintLayer {
    fn new(content: Bounds<Pixels>, slots: Vec<LabelSlot>, labels: Vec<AnyElement>) -> Self {
        debug_assert_eq!(slots.len(), labels.len());
        Self {
            content,
            slots,
            labels,
        }
    }
}

impl IntoElement for HintLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for HintLayer {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layouts = self
            .labels
            .iter_mut()
            .map(|label| label.request_layout(window, cx))
            .collect::<Vec<_>>();
        let layout = window.request_layout(
            Style {
                position: Position::Absolute,
                display: Display::Flex,
                ..Style::default()
            },
            layouts.iter().copied(),
            cx,
        );
        (layout, layouts)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        layouts: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let measured = self
            .slots
            .iter()
            .zip(layouts.iter())
            .map(|(slot, layout)| LabelSlot {
                anchor: slot.anchor,
                placement: slot.placement,
                size: window.layout_bounds(*layout).size,
            })
            .collect::<Vec<_>>();
        let origins = place_labels(&measured, self.content);
        for ((label, layout), origin) in self.labels.iter_mut().zip(layouts.iter()).zip(origins) {
            let offset = origin - window.layout_bounds(*layout).origin;
            let offset = point(offset.x.round(), offset.y.round());
            window.with_element_offset(offset, |window| {
                label.prepaint(window, cx);
            });
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for label in &mut self.labels {
            label.paint(window, cx);
        }
    }
}

/// Positions every label on the control it belongs to.
///
/// `slots` must be ordered top to bottom, then left to right, so a control's
/// label never takes the space one above it is about to use. A label first
/// takes its control's own corner, tail or trailing space and nudges sideways
/// to clear a neighbour; it only leaves that spot for the control's edge when
/// the window has no room there or a neighbour already covers it. Labels keep
/// [`EDGE_MARGIN`] from `content`'s edges.
fn place_labels(slots: &[LabelSlot], content: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let mut placed: Vec<Bounds<Pixels>> = Vec::with_capacity(slots.len());
    let mut origins = Vec::with_capacity(slots.len());
    for slot in slots {
        let origin = candidates(*slot, content)
            .into_iter()
            .find(|origin| free(*origin, slot.size, &placed))
            .unwrap_or_else(|| overflow_origin(*slot, content, &placed));
        placed.push(Bounds::new(origin, slot.size));
        origins.push(origin);
    }
    origins
}

/// Every position a label may use, best first.
fn candidates(slot: LabelSlot, content: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let mut candidates = match slot.placement {
        // A label in a control's own corner or tail sits inside the control and
        // covers none of the space around it.
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
            candidates.extend(
                edge_columns(slot, content)
                    .into_iter()
                    .map(|left| point(left, top)),
            );
        }
    }
    candidates
}

/// A control's top trailing corner, just inside it, closest to the corner first.
fn corner_origins(slot: LabelSlot, content: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let top = clamp_axis(
        slot.anchor.top() + EDGE_INSET,
        slot.size.height,
        content.top(),
        content.bottom(),
    );
    let preferred = slot.anchor.right() - EDGE_INSET - slot.size.width;
    shifted_columns(preferred, slot, content)
        .into_iter()
        .map(|left| point(left, top))
        .collect()
}

/// The top of the label's `row`-th row on `side`, or `None` when that row is
/// already past the window edge.
///
/// Row zero straddles the control's edge, so only half the label covers the
/// control and its icon stays readable.
fn edge_top(slot: LabelSlot, content: Bounds<Pixels>, side: Side, row: usize) -> Option<Pixels> {
    let height = slot.size.height;
    let step = height + LABEL_SEPARATION;
    let top = match side {
        Side::Below if row == 0 => slot.anchor.bottom() - height.half(),
        Side::Below => slot.anchor.bottom() + height.half() + LABEL_GAP + step * (row - 1) as f32,
        Side::Above if row == 0 => slot.anchor.top() - height.half(),
        Side::Above => {
            slot.anchor.top() - height.half() - LABEL_GAP - height - step * (row - 1) as f32
        }
    };
    (top >= content.top() + EDGE_MARGIN && top + height <= content.bottom() - EDGE_MARGIN)
        .then_some(top)
}

/// Where the label may start horizontally, closest to its control first.
///
/// Its trailing edge sits just inside the control's, and the small nudges let a
/// neighbouring label pass without pushing this one off its own control.
fn edge_columns(slot: LabelSlot, content: Bounds<Pixels>) -> Vec<Pixels> {
    shifted_columns(
        slot.anchor.right() - EDGE_INSET - slot.size.width,
        slot,
        content,
    )
}

/// The same trailing alignment, nudged sideways until it fits the window.
fn shifted_columns(preferred: Pixels, slot: LabelSlot, content: Bounds<Pixels>) -> Vec<Pixels> {
    let mut columns = NUDGES
        .into_iter()
        .map(|nudge| preferred + px(nudge))
        .filter(|left| fits(*left, slot.size.width, content))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        // A control at the window edge has no room for any nudge.
        columns.push(clamp_axis(
            preferred,
            slot.size.width,
            content.left(),
            content.right(),
        ));
    }
    columns
}

/// Whether a label of `width` at `left` stays inside `content`.
fn fits(left: Pixels, width: Pixels, content: Bounds<Pixels>) -> bool {
    left >= content.left() + EDGE_MARGIN && left + width <= content.right() - EDGE_MARGIN
}

/// A row control's own trailing space, when the row has room for the label.
fn trailing_origin(slot: LabelSlot, content: Bounds<Pixels>) -> Option<Point<Pixels>> {
    if slot.anchor.size.width < slot.size.width + TRAILING_INSET * 2.
        || slot.anchor.size.height < slot.size.height
    {
        return None;
    }
    Some(point(
        clamp_axis(
            slot.anchor.right() - TRAILING_INSET - slot.size.width,
            slot.size.width,
            content.left(),
            content.right(),
        ),
        clamp_axis(
            slot.anchor.center().y - slot.size.height.half(),
            slot.size.height,
            content.top(),
            content.bottom(),
        ),
    ))
}

/// A row control's free trailing space in front of the control that owns its
/// trailing edge, closest to that control first.
///
/// `reserved` is that control's width from the row's trailing edge. Each further
/// candidate steps left by one label, so two labels for one row read side by
/// side instead of on top of each other; the step repeats to the row's leading
/// edge because a label already in the row may be wider or narrower than this
/// one. A row too narrow to hold a label in front of its control offers none
/// here and falls back to a row of its own.
fn trailing_before_origins(
    slot: LabelSlot,
    content: Bounds<Pixels>,
    reserved: Pixels,
) -> Vec<Point<Pixels>> {
    if slot.anchor.size.height < slot.size.height {
        return Vec::new();
    }
    let step = slot.size.width + LABEL_SEPARATION;
    let top = clamp_axis(
        slot.anchor.center().y - slot.size.height.half(),
        slot.size.height,
        content.top(),
        content.bottom(),
    );
    let mut origins = Vec::new();
    let mut left = slot.anchor.right() - reserved - slot.size.width;
    while left >= slot.anchor.left() && fits(left, slot.size.width, content) {
        origins.push(point(left, top));
        left -= step;
    }
    origins
}

/// A compact row's bottom leading corner, its close control being in the top
/// trailing one. Candidates only move inwards, so a label stays on its row.
fn bottom_leading_origins(slot: LabelSlot, content: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let top = clamp_axis(
        slot.anchor.bottom() - EDGE_INSET - slot.size.height,
        slot.size.height,
        content.top(),
        content.bottom(),
    );
    let preferred = slot.anchor.left() + EDGE_INSET;
    let mut columns = NUDGES
        .into_iter()
        .filter(|nudge| *nudge >= 0.)
        .map(|nudge| preferred + px(nudge))
        .filter(|left| fits(*left, slot.size.width, content))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        columns.push(clamp_axis(
            preferred,
            slot.size.width,
            content.left(),
            content.right(),
        ));
    }
    columns.into_iter().map(|left| point(left, top)).collect()
}

fn overflow_origin(
    slot: LabelSlot,
    content: Bounds<Pixels>,
    placed: &[Bounds<Pixels>],
) -> Point<Pixels> {
    let left = content.left() + EDGE_MARGIN;
    let right = content.right() - EDGE_MARGIN;
    let top = content.top() + EDGE_MARGIN;
    let bottom = content.bottom() - EDGE_MARGIN;
    let step_x = slot.size.width + LABEL_SEPARATION;
    let step_y = slot.size.height + LABEL_SEPARATION;
    let mut y = top;
    while y + slot.size.height <= bottom {
        let mut x = left;
        while x + slot.size.width <= right {
            let origin = point(x, y);
            if free(origin, slot.size, placed) {
                return origin;
            }
            x += step_x;
        }
        y += step_y;
    }
    // The window cannot hold another label: keep this one on screen instead of
    // dropping it, so a key is never silently advertised as missing.
    point(
        clamp_axis(
            slot.anchor.center().x - slot.size.width.half(),
            slot.size.width,
            content.left(),
            content.right(),
        ),
        clamp_axis(
            slot.anchor.bottom() + LABEL_GAP,
            slot.size.height,
            content.top(),
            content.bottom(),
        ),
    )
}

fn free(origin: Point<Pixels>, size: Size<Pixels>, placed: &[Bounds<Pixels>]) -> bool {
    let candidate = Bounds::new(origin, size).dilate(LABEL_SEPARATION);
    placed.iter().all(|bounds| !candidate.intersects(bounds))
}

/// Keeps `origin` far enough from both `content` edges to hold `length`.
fn clamp_axis(origin: Pixels, length: Pixels, start: Pixels, end: Pixels) -> Pixels {
    origin.clamp(
        start + EDGE_MARGIN,
        (end - EDGE_MARGIN - length).max(start + EDGE_MARGIN),
    )
}

/// One compact label holding the keys without the held modifier, such as `R`,
/// `Shift+Tab` or `Tab+3`.
fn compact_label(
    key: ElementId,
    bindings: &[KeyBinding],
    held: HeldModifier,
    cx: &App,
) -> gpui_kit::base::ObservedElement<Stateful<Div>> {
    let text_size = cx
        .gds_ro()
        .appearance()
        .typography()
        .scaled(LABEL_TEXT_SIZE);
    let label = ShortcutBadge::path_label(bindings, held).unwrap_or_default();
    div()
        .id(SharedString::from(format!("keytip-{key}")))
        .test_support()
        .role(Role::Label)
        .aria_label(label.clone())
        .flex()
        .items_center()
        .py(px(LABEL_PADDING_Y))
        .px(px(LABEL_PADDING_X))
        .rounded(px(LABEL_RADIUS))
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .text_size(text_size)
        // A tight line box keeps the label around 16px tall at the default
        // text size; GPUI's golden-ratio default would make it a small pill.
        .line_height(px(f32::from(text_size) * LABEL_LINE_HEIGHT))
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

#[cfg(test)]
mod tests;
