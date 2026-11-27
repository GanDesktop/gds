//! Floating pointer-tracked drag preview card (`Portal`) and drop target helpers.

mod content;
mod footer;
mod header;
mod icon;

pub use content::DragCardContent;
pub use footer::DragCardFooter;
pub use header::DragCardHeader;
pub use icon::{MAX_PREVIEW_ITEMS, render_drag_card_icon};

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal, UnsyncCallback};
use zgui::view::{AttrName, OverlayLayer, Portal, events};
use zgui::vocab::PointerButton;
use zgui::{component, css, view};

/// Pointer offset in CSS pixels separating the cursor from the floating [`DragCard`].
pub const POINTER_GAP: f32 = 12.0;

/// Fixed width in CSS pixels of a floating file drag preview card (`DragCard` and native
/// `DragIcon`).
pub const CARD_WIDTH: f32 = 248.0;

/// Minimum width in CSS pixels of a floating [`DragCard`].
pub const MIN_WIDTH: f32 = CARD_WIDTH;

/// Maximum width in CSS pixels of a floating [`DragCard`].
pub const MAX_WIDTH: f32 = CARD_WIDTH;

/// Pointer movement distance in CSS pixels required before a press promotes to an active drag.
pub const DRAG_THRESHOLD: f32 = 5.0;

const SHEET: &str = css!(
    r#"
    .gds-drag-portal-overlay {
        position: fixed;
        inset: 0;
        pointer-events: none;
    }

    .gds-drag-card.gds-drag-card {
        display: flex;
        flex-direction: column;
        overflow: hidden;
        width: 248px;
        min-width: 248px;
        max-width: 248px;
        box-sizing: border-box;
        padding: 0px;
        gap: 0px;
        border-radius: 8px;
        border: 1px solid var(--gds-border);
        background: var(--gds-panel);
        color: var(--gds-panel-foreground);
        font-family: var(--gds-font-sans);
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.24);
        user-select: none;
    }

    .gds-drag-card.gds-drag-card--floating {
        position: fixed;
        pointer-events: none;
    }

    .gds-drag-card__header.gds-drag-card__header {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0px;
        padding: 6px 10px;
        border-bottom: 1px solid var(--gds-border);
        font-size: var(--gds-text-xs, 12px);
        font-weight: 600;
        color: var(--gds-panel-foreground);
    }

    .gds-drag-card__content.gds-drag-card__content {
        display: flex;
        flex-direction: column;
        gap: 3px;
        min-width: 0px;
        padding: 6px 10px;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-panel-foreground);
        overflow-wrap: anywhere;
    }

    .gds-drag-card__footer.gds-drag-card__footer {
        display: flex;
        flex-direction: row;
        align-items: center;
        min-width: 0px;
        padding: 0px 10px 6px 10px;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-drop-target.gds-drop-target[data-drop-active="true"] {
        background: var(--gds-drag-target);
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }
    "#
);

/// Installs the shared drag overlay and drop target stylesheet on the current window host.
pub fn install_drag_stylesheet() {
    if zgui::view::current_host().is_some() {
        crate::ui::styles::install_ui_stylesheets();
        zgui::view::install_stylesheet("gds-ui-drag", SHEET);
    }
}

/// Inline fixed-width drag preview card surface, shared between floating [`DragCard`] overlays and
/// offscreen native [`render_drag_card_icon`] snapshots.
#[component]
pub fn DragCardSurface(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_drag_stylesheet();
    view! {
        column(class = "gds-drag-card", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}

/// Floating preview surface displayed via [`Portal`] while an internal pointer drag is active.
#[component]
pub fn DragCard(
    #[prop(into, default = Signal::stored_local((0.0_f32, 0.0_f32)))] position: Signal<
        (f32, f32),
        LocalStorage,
    >,
    #[prop(optional)] offset: Option<(f32, f32)>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_drag_stylesheet();
    let coords = move || offset.unwrap_or_else(|| position.get());
    Portal::new(move || {
        AnyView::new(view! {
            box(class = "gds-drag-portal-overlay", a11y:hidden = true) {
                column(
                    class = "gds-drag-card gds-drag-card--floating",
                    class = class,
                    style:left = move || Some(format!("{}px", coords().0 + POINTER_GAP)),
                    style:top = move || Some(format!("{}px", coords().1 + POINTER_GAP)),
                    {..attrs}
                ) {
                    {children.into_view_once()}
                }
            }
        })
    })
    .layer(OverlayLayer::Popover)
}

/// Window-local reactive state controller coordinating a pointer-followed drag session.
pub struct DragOverlayState<T: Clone + 'static> {
    payload: RwSignal<Option<T>, LocalStorage>,
    position: RwSignal<(f32, f32), LocalStorage>,
    hovered_target: RwSignal<Option<String>, LocalStorage>,
}

impl<T: Clone + 'static> Clone for DragOverlayState<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Clone + 'static> Copy for DragOverlayState<T> {}

impl<T: Clone + 'static> Default for DragOverlayState<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + 'static> DragOverlayState<T> {
    /// Creates an idle [`DragOverlayState`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            payload: RwSignal::new_local(None),
            position: RwSignal::new_local((0.0, 0.0)),
            hovered_target: RwSignal::new_local(None),
        }
    }

    /// Signal indicating whether a drag operation is currently in flight.
    #[must_use]
    pub fn is_active(self) -> Signal<bool, LocalStorage> {
        Signal::derive_local(move || self.payload.with(Option::is_some))
    }

    /// Reactive handle to the dragged payload, when active.
    #[must_use]
    pub fn payload(self) -> Signal<Option<T>, LocalStorage> {
        self.payload.into()
    }

    /// Current pointer coordinates in window CSS pixels.
    #[must_use]
    pub fn position(self) -> Signal<(f32, f32), LocalStorage> {
        self.position.into()
    }

    /// Optional identifier of the currently hovered drop target.
    #[must_use]
    pub fn hovered_target(self) -> Signal<Option<String>, LocalStorage> {
        self.hovered_target.into()
    }

    /// Starts a drag session with `payload` at CSS pointer coordinate `(x, y)`.
    pub fn begin(self, payload: T, x: f32, y: f32) {
        self.position.set((x, y));
        self.hovered_target.set(None);
        self.payload.set(Some(payload));
    }

    /// Updates the pointer coordinates during an active drag session.
    pub fn move_to(self, x: f32, y: f32) {
        self.position.set((x, y));
    }

    /// Sets the identifier of the currently hovered drop target.
    pub fn set_hovered_target(self, target: Option<String>) {
        self.hovered_target.set(target);
    }

    /// Completes the active drag session and returns its payload, clearing transient state.
    pub fn end(self) -> Option<T> {
        self.hovered_target.set(None);
        self.payload.try_update(Option::take).flatten()
    }

    /// Cancels any active drag session without delivering a drop payload.
    pub fn cancel(self) {
        self.hovered_target.set(None);
        self.payload.set(None);
    }

    /// Builds [`Attrs`] wiring pointer drag capture (after [`DRAG_THRESHOLD`] `5px` movement),
    /// movement updates, and release completion on a draggable source element.
    #[must_use]
    pub fn source_attrs(
        self,
        make_payload: impl Fn() -> Option<T> + 'static,
        on_drop: Option<UnsyncCallback<(T, (f32, f32))>>,
    ) -> Attrs {
        let armed: Rc<Cell<Option<(f32, f32)>>> = Rc::new(Cell::new(None));
        let armed_down = Rc::clone(&armed);
        let armed_move = Rc::clone(&armed);
        let armed_up = Rc::clone(&armed);
        let armed_cancel = Rc::clone(&armed);
        let make_payload = Rc::new(make_payload);

        Attrs::new()
            .listener(
                events::POINTER_DOWN,
                ListenerOptions::DEFAULT,
                move |ev: &mut EventCx<'_, events::PointerDown>| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    armed_down.set(Some((ev.position.x.0, ev.position.y.0)));
                },
            )
            .listener(
                events::POINTER_MOVE,
                ListenerOptions::DEFAULT,
                move |ev: &mut EventCx<'_, events::PointerMove>| {
                    let pos = (ev.position.x.0, ev.position.y.0);
                    if !self.payload.with_untracked(Option::is_some) {
                        if let Some(start) = armed_move.get()
                            && (pos.0 - start.0).hypot(pos.1 - start.1) >= DRAG_THRESHOLD
                        {
                            armed_move.set(None);
                            if let Some(payload) = make_payload() {
                                ev.capture_pointer();
                                self.begin(payload, pos.0, pos.1);
                            }
                        }
                        return;
                    }
                    self.move_to(pos.0, pos.1);
                },
            )
            .listener(
                events::POINTER_UP,
                ListenerOptions::DEFAULT,
                move |ev: &mut EventCx<'_, events::PointerUp>| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    armed_up.set(None);
                    if self.payload.with_untracked(Option::is_some) {
                        ev.release_pointer();
                        let pos = (ev.position.x.0, ev.position.y.0);
                        self.move_to(pos.0, pos.1);
                        if let Some(payload) = self.end()
                            && let Some(on_drop) = on_drop
                        {
                            on_drop.run((payload, pos));
                        }
                    }
                },
            )
            .listener(
                events::POINTER_CANCEL,
                ListenerOptions::DEFAULT,
                move |ev: &mut EventCx<'_, events::PointerCancel>| {
                    armed_cancel.set(None);
                    if self.payload.with_untracked(Option::is_some) {
                        ev.release_pointer();
                        self.cancel();
                    }
                },
            )
    }
}

/// Returns `true` when window CSS pointer coordinate `(x, y)` lies inside `node`'s rendered box.
#[must_use]
pub fn drop_hit_test(node: &NodeRef, position: (f32, f32)) -> bool {
    let Some(bounds) = node.window_bounds() else {
        return false;
    };
    let scale = node.scale().max(0.01);
    let left = bounds.origin.x.0 / scale;
    let top = bounds.origin.y.0 / scale;
    let width = bounds.size.width.0 / scale;
    let height = bounds.size.height.0 / scale;
    position.0 >= left
        && position.0 <= left + width
        && position.1 >= top
        && position.1 <= top + height
}

/// One external OS file drop delivered onto a drop target, carrying the negotiated action and
/// protocol completion handle.
pub struct ExternalFileDrop {
    /// Local file paths decoded from `text/uri-list`.
    pub paths: Vec<PathBuf>,
    /// Final action settled by the compositor (`Copy`, `Move`, or `None` when unspecified).
    pub action: Option<DragAction>,
    /// Pointer coordinates in CSS pixels at release.
    pub position: (f32, f32),
    /// Protocol completion token; call [`DropHandle::complete`] only once the operation succeeds.
    pub handle: Option<DropHandle>,
}

/// Negotiation policy advertised by a file drop target during hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileDropOptions {
    /// Actions allowed when files are dropped onto this target.
    pub supported_actions: DragActions,
    /// Action preferred by default when keyboard modifiers do not override it.
    pub preferred_action: Option<DragAction>,
}

impl FileDropOptions {
    /// Accepts only copying (for example pasting file paths into a terminal).
    #[must_use]
    pub const fn copy_only() -> Self {
        Self {
            supported_actions: DragActions::COPY,
            preferred_action: Some(DragAction::Copy),
        }
    }

    /// Accepts both copying and moving, preferring `preferred` when no modifier overrides it.
    #[must_use]
    pub const fn copy_or_move(preferred: DragAction) -> Self {
        Self {
            supported_actions: DragActions::COPY.union(DragActions::MOVE),
            preferred_action: Some(preferred),
        }
    }
}

/// Why [`file_drop_target_attrs`] rejected an incoming `DROP` event before invoking `on_drop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDropRejection {
    /// The target's `can_accept` gate returned `false`.
    Disallowed,
    /// The dropped payload contained no readable local file paths.
    EmptyPaths,
}

/// Configuration for [`file_drop_target_attrs`].
pub struct FileDropTarget {
    /// Whether an in-window drag is hovering over this target.
    pub active: Signal<bool, LocalStorage>,
    /// Actions advertised to the compositor during `drag_enter` / `drag_over`.
    pub options: FileDropOptions,
    /// Optional dynamic gate checked on hover and drop (for example rejecting a folder dragged onto
    /// itself or a descendant within this process).
    pub can_accept: Option<Rc<dyn Fn() -> bool>>,
    /// Optional hover transition callback (`true` on accepted enter, `false` on leave/drop).
    pub on_hover_change: Option<UnsyncCallback<bool>>,
    /// Handler invoked on drop with decoded paths, negotiated action, and [`DropHandle`].
    pub on_drop: Option<UnsyncCallback<ExternalFileDrop>>,
    /// Optional callback invoked when an incoming `DROP` is rejected before `on_drop` runs.
    pub on_reject: Option<UnsyncCallback<FileDropRejection>>,
}

/// Attaches `.gds-drop-target` active state tracking, dynamic MIME/action negotiation, and OS
/// external file drop listeners onto a destination element.
#[must_use]
pub fn file_drop_target_attrs(target: FileDropTarget) -> Attrs {
    install_drag_stylesheet();
    let FileDropTarget {
        active,
        options,
        can_accept,
        on_hover_change,
        on_drop,
        on_reject,
    } = target;

    let external_hover = RwSignal::new_local(false);
    let is_active = Signal::derive_local(move || active.get() || external_hover.get());
    let set_hover = move |next: bool| {
        if external_hover.get_untracked() != next {
            external_hover.set(next);
            if let Some(cb) = on_hover_change {
                cb.run(next);
            }
        }
    };

    let enter_can_accept = can_accept.clone();
    let over_can_accept = can_accept.clone();
    let drop_can_accept = can_accept;

    Attrs::new()
        .class_toggle(zgui::view::ClassName::new("gds-drop-target"), true)
        .attribute(AttrName::new("data-drop-active"), move || {
            is_active.get().then(|| "true".to_owned())
        })
        .listener(
            events::DRAG_ENTER,
            ListenerOptions::DEFAULT,
            move |ev: &mut EventCx<'_, events::DragEnter>| {
                let allowed =
                    ev.offers_files() && enter_can_accept.as_ref().is_none_or(|gate| gate());
                if allowed {
                    set_hover(true);
                    ev.accept_drag(DragDecision::accept(
                        "text/uri-list",
                        options.supported_actions,
                        options.preferred_action,
                    ));
                    ev.prevent_default();
                } else {
                    set_hover(false);
                    ev.accept_drag(DragDecision::reject());
                }
            },
        )
        .listener(
            events::DRAG_OVER,
            ListenerOptions::DEFAULT,
            move |ev: &mut EventCx<'_, events::DragOver>| {
                let allowed =
                    ev.offers_files() && over_can_accept.as_ref().is_none_or(|gate| gate());
                if allowed {
                    set_hover(true);
                    ev.accept_drag(DragDecision::accept(
                        "text/uri-list",
                        options.supported_actions,
                        options.preferred_action,
                    ));
                    ev.prevent_default();
                } else {
                    set_hover(false);
                    ev.accept_drag(DragDecision::reject());
                }
                ev.stop_propagation();
            },
        )
        .listener(events::DRAG_LEAVE, ListenerOptions::DEFAULT, move |_| {
            set_hover(false);
        })
        .listener(
            events::DROP,
            ListenerOptions::DEFAULT,
            move |ev: &mut EventCx<'_, events::Drop>| {
                set_hover(false);
                let allowed = drop_can_accept.as_ref().is_none_or(|gate| gate());
                if !allowed {
                    if let Some(handle) = ev.take_drop_handle() {
                        handle.reject();
                    }
                    if let Some(on_reject) = on_reject {
                        on_reject.run(FileDropRejection::Disallowed);
                    }
                    ev.stop_propagation();
                    return;
                }
                let Some(on_drop) = on_drop else {
                    return;
                };
                let handle = ev.take_drop_handle();
                if ev.paths.is_empty() {
                    if let Some(handle) = handle {
                        handle.reject();
                    }
                    if let Some(on_reject) = on_reject {
                        on_reject.run(FileDropRejection::EmptyPaths);
                    }
                    ev.stop_propagation();
                    return;
                }
                let pos = (ev.position.x.0, ev.position.y.0);
                on_drop.run(ExternalFileDrop {
                    paths: ev.paths.clone(),
                    action: ev.action,
                    position: pos,
                    handle,
                });
                ev.prevent_default();
                ev.stop_propagation();
            },
        )
}

/// Attaches `.gds-drop-target` active state tracking and copy-only OS external file drop
/// listeners onto a destination element.
#[must_use]
pub fn drop_target_attrs(
    active: Signal<bool, LocalStorage>,
    on_external_drop: Option<UnsyncCallback<(Vec<PathBuf>, (f32, f32))>>,
) -> Attrs {
    let on_drop = on_external_drop.map(|cb| {
        UnsyncCallback::new(move |drop: ExternalFileDrop| {
            cb.run((drop.paths, drop.position));
            if let Some(handle) = drop.handle {
                handle.complete();
            }
        })
    });
    file_drop_target_attrs(FileDropTarget {
        active,
        options: FileDropOptions::copy_only(),
        can_accept: None,
        on_hover_change: None,
        on_drop,
        on_reject: None,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use zgui::app::Fonts;
    use zgui::geom::{CssPx, DevicePx, Point, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{Mounted, UnsyncCallback, install};
    use zgui::runtime::AppError;
    use zgui::vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Timestamp};
    use zgui_platform_headless::Harness;

    use super::DragOverlayState;

    #[test]
    fn drag_overlay_state_tracks_begin_move_and_completion() {
        install().ok();
        let scope = Mounted::new();
        scope.with(|| {
            let drag = DragOverlayState::<&'static str>::new();
            assert!(!drag.is_active().get_untracked());

            drag.begin("3 items", 120.0, 240.0);
            assert!(drag.is_active().get_untracked());
            assert_eq!(drag.payload().get_untracked(), Some("3 items"));
            assert_eq!(drag.position().get_untracked(), (120.0, 240.0));

            drag.move_to(180.0, 260.0);
            drag.set_hovered_target(Some("Documents".to_owned()));
            assert_eq!(drag.position().get_untracked(), (180.0, 260.0));
            assert_eq!(
                drag.hovered_target().get_untracked().as_deref(),
                Some("Documents")
            );

            assert_eq!(drag.end(), Some("3 items"));
            assert!(!drag.is_active().get_untracked());
            assert_eq!(drag.hovered_target().get_untracked(), None);

            drag.begin("other", 0.0, 0.0);
            drag.cancel();
            assert!(!drag.is_active().get_untracked());
        });
        scope.unmount();
    }

    #[test]
    fn source_attrs_enforces_five_pixel_threshold_before_starting_drag() {
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

        let dropped: Rc<RefCell<Vec<(&'static str, (f32, f32))>>> =
            Rc::new(RefCell::new(Vec::new()));
        let dropped_view = Rc::clone(&dropped);
        let dropped_driver = Rc::clone(&dropped);
        let drag_slot: Rc<RefCell<Option<DragOverlayState<&'static str>>>> =
            Rc::new(RefCell::new(None));
        let drag_slot_view = Rc::clone(&drag_slot);
        let drag_slot_driver = Rc::clone(&drag_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(200.0),
                DevicePx(100.0),
            )));
            harness.settle(8);
            let drag = drag_slot_driver.borrow().expect("drag state");

            // Sub-5px jitter (3px) must not begin a drag or emit a drop.
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(Point::new(CssPx(20.0), CssPx(20.0)))
                    .with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(Point::new(CssPx(23.0), CssPx(20.0))),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert!(!drag.is_active().get_untracked());
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(Point::new(CssPx(23.0), CssPx(20.0)))
                    .with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert!(dropped_driver.borrow().is_empty());

            // Crossing >= 5px threshold begins the drag, captures pointer, and delivers drop.
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(Point::new(CssPx(20.0), CssPx(20.0)))
                    .with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(Point::new(CssPx(26.0), CssPx(20.0))),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert!(drag.is_active().get_untracked());
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(Point::new(CssPx(140.0), CssPx(70.0)))
                    .with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(4);
            assert!(!drag.is_active().get_untracked());
            assert_eq!(
                *dropped_driver.borrow(),
                vec![("files-batch", (140.0, 70.0))]
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_size(200.0, 100.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = Sink::default();
                zgui::render::Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                let drag = DragOverlayState::<&'static str>::new();
                *drag_slot_view.borrow_mut() = Some(drag);
                let out = Rc::clone(&dropped_view);
                let attrs = drag.source_attrs(
                    || Some("files-batch"),
                    Some(UnsyncCallback::new(move |entry| {
                        out.borrow_mut().push(entry);
                    })),
                );
                zgui::view! {
                    box(style:width = "200px", style:height = "100px", {..attrs}) {
                        text { "Draggable row" }
                    }
                }
            })
            .expect("drag threshold test ran");
    }

    #[test]
    fn file_drop_target_negotiates_lazy_hover_and_defers_drop_handle() {
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU8, Ordering};
        use zgui::platform::DragEvent;

        use super::{
            ExternalFileDrop, FileDropOptions, FileDropRejection, FileDropTarget,
            file_drop_target_attrs,
        };

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

        let captured_drop: Rc<RefCell<Option<ExternalFileDrop>>> = Rc::new(RefCell::new(None));
        let drop_view = Rc::clone(&captured_drop);
        let drop_driver = Rc::clone(&captured_drop);
        let captured_rejections: Rc<RefCell<Vec<FileDropRejection>>> =
            Rc::new(RefCell::new(Vec::new()));
        let rejections_view = Rc::clone(&captured_rejections);
        let rejections_driver = Rc::clone(&captured_rejections);
        let allow_gate = Rc::new(std::cell::Cell::new(true));
        let gate_view = Rc::clone(&allow_gate);
        let gate_driver = Rc::clone(&allow_gate);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(200.0),
                DevicePx(100.0),
            )));
            harness.settle(8);

            let surface = harness.platform().offscreens()[0].clone();
            let offer = DragOfferId::new(1);

            // Lazy Wayland hover carries mime_types with empty paths and must negotiate Copy|Move.
            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Entered {
                offer,
                mime_types: vec!["text/uri-list".to_owned()],
                source_actions: DragActions::COPY | DragActions::MOVE,
                action: Some(DragAction::Move),
                paths: Vec::new(),
                position: Point::new(CssPx(40.0), CssPx(40.0)),
            }));
            harness.settle(4);
            assert_eq!(
                surface.take_drag_responses(),
                vec![(
                    offer,
                    DragDecision::accept(
                        "text/uri-list",
                        DragActions::COPY | DragActions::MOVE,
                        Some(DragAction::Move),
                    ),
                )]
            );

            // When the dynamic gate disallows dropping (e.g. self-directory), hover rejects.
            gate_driver.set(false);
            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Moved {
                offer,
                position: Point::new(CssPx(45.0), CssPx(40.0)),
            }));
            harness.settle(4);
            assert_eq!(
                surface.take_drag_responses(),
                vec![(offer, DragDecision::reject())]
            );

            // Dropping while gate disallows rejects the DropHandle and notifies on_reject.
            let disallowed_settled = Arc::new(AtomicU8::new(0));
            let disallowed_cb = Arc::clone(&disallowed_settled);
            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Dropped {
                offer,
                mime_types: vec!["text/uri-list".to_owned()],
                source_actions: DragActions::COPY | DragActions::MOVE,
                action: Some(DragAction::Move),
                paths: vec![PathBuf::from("/tmp/self-dir")],
                payload: None,
                position: Point::new(CssPx(45.0), CssPx(40.0)),
                handle: DropHandle::new(move |ok| {
                    disallowed_cb.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
                }),
            }));
            harness.settle(4);
            assert_eq!(disallowed_settled.load(Ordering::Relaxed), 2);
            assert!(drop_driver.borrow().is_none());

            // Re-enable gate and drop with empty paths -> rejects and notifies EmptyPaths.
            gate_driver.set(true);
            let empty_settled = Arc::new(AtomicU8::new(0));
            let empty_cb = Arc::clone(&empty_settled);
            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Dropped {
                offer,
                mime_types: vec!["text/uri-list".to_owned()],
                source_actions: DragActions::COPY | DragActions::MOVE,
                action: Some(DragAction::Move),
                paths: Vec::new(),
                payload: None,
                position: Point::new(CssPx(45.0), CssPx(40.0)),
                handle: DropHandle::new(move |ok| {
                    empty_cb.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
                }),
            }));
            harness.settle(4);
            assert_eq!(empty_settled.load(Ordering::Relaxed), 2);
            assert_eq!(
                *rejections_driver.borrow(),
                vec![FileDropRejection::Disallowed, FileDropRejection::EmptyPaths]
            );

            // Drop with valid paths -> DropHandle stays pending until caller completes it.
            let settled = Arc::new(AtomicU8::new(0));
            let settled_cb = Arc::clone(&settled);
            harness.deliver_to_first(SurfaceEvent::Drag(DragEvent::Dropped {
                offer,
                mime_types: vec!["text/uri-list".to_owned()],
                source_actions: DragActions::COPY | DragActions::MOVE,
                action: Some(DragAction::Move),
                paths: vec![PathBuf::from("/tmp/moved.txt")],
                payload: None,
                position: Point::new(CssPx(45.0), CssPx(40.0)),
                handle: DropHandle::new(move |ok| {
                    settled_cb.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
                }),
            }));
            harness.settle(4);
            assert_eq!(settled.load(Ordering::Relaxed), 0);

            let received = drop_driver
                .borrow_mut()
                .take()
                .expect("drop payload delivered");
            assert_eq!(received.action, Some(DragAction::Move));
            assert_eq!(received.paths, vec![PathBuf::from("/tmp/moved.txt")]);
            received.handle.expect("DropHandle forwarded").complete();
            assert_eq!(settled.load(Ordering::Relaxed), 1);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_size(200.0, 100.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = Sink::default();
                zgui::render::Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                let out = Rc::clone(&drop_view);
                let rej = Rc::clone(&rejections_view);
                let gate = Rc::clone(&gate_view);
                let attrs = file_drop_target_attrs(FileDropTarget {
                    active: Signal::stored_local(false),
                    options: FileDropOptions::copy_or_move(DragAction::Move),
                    can_accept: Some(Rc::new(move || gate.get())),
                    on_hover_change: None,
                    on_drop: Some(UnsyncCallback::new(move |drop| {
                        *out.borrow_mut() = Some(drop);
                    })),
                    on_reject: Some(UnsyncCallback::new(move |reason| {
                        rej.borrow_mut().push(reason);
                    })),
                });
                zgui::view! {
                    box(style:width = "200px", style:height = "100px", {..attrs}) {
                        text { "Folder target" }
                    }
                }
            })
            .expect("file drop target negotiation test ran");
    }
}
