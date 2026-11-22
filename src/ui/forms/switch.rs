//! Proportional GDS toggle switch with frame-driven `140ms` off-to-on motion.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback};
use zgui::view::AttrName;
use zgui::vocab::{Key, NamedKey, Role, UiState};
use zgui::{component, css, view};
use zgui_ui_primitives::{Binding, Controllable};

use crate::appearance::motion::{INLINE_DURATION_MS, use_transition};

const SWITCH_SHEET: &str = "gds-ui-switch";
const ENDPOINT_EPSILON: f32 = 1e-4;

const SWITCH_CSS: &str = css!(
    ".gds-toggle-switch.gds-toggle-switch {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        justify-content: flex-start;
        box-sizing: border-box;
        flex-shrink: 0;
        width: 44px;
        height: 24px;
        padding: 2px;
        border-width: 1px;
        border-style: solid;
        border-color: var(--gds-toggle-track-border, var(--gds-outline));
        border-radius: 9999px;
        background-color: var(--gds-toggle-track-bg, var(--gds-switch-off));
        cursor: pointer;
        user-select: none;
        outline: none;
        box-shadow: none;
        transition: none;
    }
    .gds-toggle-switch.gds-toggle-switch[data-size=\"sm\"] {
        width: 36px;
        height: 20px;
    }
    .gds-toggle-switch.gds-toggle-switch:hover:not([data-disabled=\"true\"]) {
        background-color: color-mix(
            in srgb,
            var(--gds-foreground) 8%,
            var(--gds-toggle-track-bg, var(--gds-switch-off))
        );
        border-color: color-mix(
            in srgb,
            var(--gds-foreground) 16%,
            var(--gds-toggle-track-border, var(--gds-outline))
        );
    }
    .gds-toggle-switch.gds-toggle-switch:active:not([data-disabled=\"true\"]) {
        background-color: color-mix(
            in srgb,
            var(--gds-foreground) 14%,
            var(--gds-toggle-track-bg, var(--gds-switch-off))
        );
        border-color: color-mix(
            in srgb,
            var(--gds-foreground) 22%,
            var(--gds-toggle-track-border, var(--gds-outline))
        );
    }
    .gds-toggle-switch.gds-toggle-switch:focus:not(:focus-visible) {
        outline: none;
        box-shadow: none;
    }
    .gds-toggle-switch.gds-toggle-switch:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: 2px;
        box-shadow: none;
    }
    .gds-toggle-switch.gds-toggle-switch[data-disabled=\"true\"],
    .gds-toggle-switch.gds-toggle-switch:disabled {
        cursor: not-allowed;
        opacity: 0.5;
    }
    .gds-toggle-switch.gds-toggle-switch > .gds-toggle-switch__thumb {
        width: 18px;
        height: 18px;
        flex: none;
        box-sizing: border-box;
        border-radius: 9999px;
        background-color: var(--gds-toggle-thumb-bg, var(--gds-switch-thumb-off));
        pointer-events: none;
        transform: translateX(0px);
        transition: none;
    }
    .gds-toggle-switch.gds-toggle-switch[data-size=\"sm\"] > .gds-toggle-switch__thumb {
        width: 14px;
        height: 14px;
    }
    .gds-toggle-switch.gds-toggle-switch:active:not([data-disabled=\"true\"]) > .gds-toggle-switch__thumb {
        background-color: color-mix(
            in srgb,
            var(--gds-foreground) 10%,
            var(--gds-toggle-thumb-bg, var(--gds-switch-thumb-off))
        );
    }"
);

pub(crate) fn install_switch_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(SWITCH_SHEET, SWITCH_CSS);
}

/// Geometry presets for [`ToggleSwitch`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToggleSwitchSize {
    /// Compact `36px × 20px` pill (`14px` circular thumb, `3px` inset, `16px` travel).
    Sm,
    /// Standard `44px × 24px` pill (`18px` circular thumb, `3px` inset, `20px` travel) paired with
    /// `36px` single-line controls and `32px` icon buttons in Settings rows.
    #[default]
    Md,
}

impl ToggleSwitchSize {
    #[must_use]
    pub const fn track_width(self) -> f32 {
        match self {
            Self::Sm => 36.0,
            Self::Md => 44.0,
        }
    }

    #[must_use]
    pub const fn track_height(self) -> f32 {
        match self {
            Self::Sm => 20.0,
            Self::Md => 24.0,
        }
    }

    #[must_use]
    pub const fn thumb_size(self) -> f32 {
        match self {
            Self::Sm => 14.0,
            Self::Md => 18.0,
        }
    }

    #[must_use]
    pub const fn travel(self) -> f32 {
        match self {
            Self::Sm => 16.0,
            Self::Md => 20.0,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
        }
    }
}

fn mix_css_tokens(off_token: &'static str, on_token: &'static str, progress: f32) -> String {
    let p = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if p <= ENDPOINT_EPSILON {
        format!("var({off_token})")
    } else if p >= 1.0 - ENDPOINT_EPSILON {
        format!("var({on_token})")
    } else {
        let pct = (p * 100.0).clamp(0.01, 99.99);
        format!("color-mix(in srgb, var({on_token}) {pct:.2}%, var({off_token}))")
    }
}

struct SwitchConfig {
    checked: Binding<bool>,
    default_checked: bool,
    on_change: Option<UnsyncCallback<bool>>,
    disabled: Signal<bool, LocalStorage>,
    size: ToggleSwitchSize,
    label: Option<String>,
    labelled_by: Option<NodeRef>,
    node_ref: Option<NodeRef>,
    thumb_ref: Option<NodeRef>,
    class: Classes,
    attrs: Attrs,
}

fn render_toggle_switch(config: SwitchConfig) -> impl IntoView {
    install_switch_stylesheets();

    let SwitchConfig {
        checked,
        default_checked,
        on_change,
        disabled,
        size,
        label,
        labelled_by,
        node_ref,
        thumb_ref,
        class,
        attrs,
    } = config;

    let element = node_ref.unwrap_or_default();
    let thumb = thumb_ref.unwrap_or_default();
    let value = Controllable::new(checked, default_checked, on_change);
    let travel = size.travel();

    let target = Signal::derive_local(move || if value.get() { 1.0_f32 } else { 0.0_f32 });
    let progress = use_transition(target, INLINE_DURATION_MS);

    let mut semantics = A11yBinding::new(Role::Switch)
        .toggled_on(move || value.get())
        .disabled(move || disabled.get());
    if let Some(label_text) = label {
        semantics = semantics.label(label_text);
    }
    if let Some(label_node) = labelled_by {
        semantics = semantics.labelled_by(label_node);
    }

    let own = Attrs::new()
        .attribute(AttrName::new("data-size"), Some(size.as_str().to_owned()))
        .attribute(AttrName::new("data-state"), move || {
            Some(if value.get() { "checked" } else { "unchecked" }.to_owned())
        })
        .attribute(AttrName::new("data-disabled"), move || {
            disabled.get().then(|| "true".to_owned())
        })
        .state(UiState::CHECKED, move || value.get())
        .state(UiState::DISABLED, move || disabled.get())
        .a11y_from(semantics);

    view! {
        control(
            node_ref = element,
            class = "gds-toggle-switch",
            tabindex = move || {
                if disabled.get() {
                    Focus::Programmatic
                } else {
                    Focus::Sequential
                }
            },
            var:--gds-toggle-track-bg = move || {
                Some(mix_css_tokens("--gds-switch-off", "--gds-primary", progress.get()))
            },
            var:--gds-toggle-track-border = move || {
                Some(mix_css_tokens("--gds-outline", "--gds-primary", progress.get()))
            },
            var:--gds-toggle-thumb-bg = move || {
                Some(mix_css_tokens(
                    "--gds-switch-thumb-off",
                    "--gds-primary-foreground",
                    progress.get(),
                ))
            },
            on:click = move |ev: &mut EventCx<'_, events::Click>| {
                ev.stop_propagation();
                if !disabled.get_untracked() {
                    value.toggle();
                }
            },
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                if ev.repeat || !ev.modifiers.is_empty() {
                    return;
                }
                if matches!(ev.key, Key::Named(NamedKey::Space | NamedKey::Enter)) {
                    ev.prevent_default();
                    ev.stop_propagation();
                    if !disabled.get_untracked() {
                        value.toggle();
                    }
                }
            },
            {..own},
            {..attrs},
            class = class,
        ) {
            box(
                node_ref = thumb,
                class = "gds-toggle-switch__thumb",
                style:transform = move || Some(format!("translateX({:.2}px)", travel * progress.get())),
                a11y:hidden = true,
            )
        }
    }
}

/// Immediate boolean switch with proportional `44px × 24px` desktop sizing and `140ms` frame-driven
/// thumb & color transitions.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn ToggleSwitch(
    #[prop(into, optional)] checked: Binding<bool>,
    #[prop(default = false)] default_checked: bool,
    #[prop(optional)] on_change: Option<UnsyncCallback<bool>>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(default = ToggleSwitchSize::Md)] size: ToggleSwitchSize,
    #[prop(into, optional)] label: Option<String>,
    #[prop(optional)] labelled_by: Option<NodeRef>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    render_toggle_switch(SwitchConfig {
        checked,
        default_checked,
        on_change,
        disabled,
        size,
        label,
        labelled_by,
        node_ref,
        thumb_ref: None,
        class,
        attrs,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use zgui::app::Fonts;
    use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
    use zgui::geom::{CssPx, Device, DevicePx, Point, Rect, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{LocalStorage, RwSignal, UnsyncCallback};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::view;
    use zgui::vocab::{
        KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction,
        PointerButton, PointerEvent, Role, Timestamp,
    };
    use zgui_platform_headless::Harness;

    use super::{SwitchConfig, ToggleSwitchProps, ToggleSwitchSize, render_toggle_switch};
    use crate::app::Gds;
    use crate::service::task_manager::Scope;
    use crate::ui::forms::field::FieldLabelProps;

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

    fn space_key_event() -> KeyEvent {
        KeyEvent::named(NamedKey::Space, PhysicalKey::Code(KeyCode::Space))
    }

    fn enter_key_event() -> KeyEvent {
        KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter))
    }

    #[test]
    fn toggle_switch_exposes_its_label_and_ignores_input_when_disabled() {
        let disabled_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
        let disabled_val_slot: Rc<RefCell<Option<RwSignal<bool, LocalStorage>>>> =
            Rc::new(RefCell::new(None));

        let disabled_driver = Rc::clone(&disabled_slot);
        let disabled_val_driver = Rc::clone(&disabled_val_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(400.0),
                DevicePx(300.0),
            )));
            if let Some(surface) = harness.platform().offscreens().first() {
                surface.set_a11y_listener(true);
            }
            harness.settle(16);

            // Verify a11y Role::Switch + labelled_by wiring.
            let surface = harness.platform().offscreens()[0].clone();
            let switch_nodes: Vec<_> = surface
                .a11y_log()
                .into_iter()
                .flat_map(|u| u.nodes)
                .filter(|(_, n)| n.role() == Role::Switch)
                .collect();
            assert!(
                switch_nodes
                    .iter()
                    .any(|(_, n)| !n.labelled_by().is_empty()),
                "ToggleSwitch must expose Role::Switch with labelled_by"
            );

            // Clicking or pressing Space on a disabled ToggleSwitch must not mutate `checked`.
            let dis_node = disabled_driver.borrow().expect("disabled ref");
            let scale = dis_node.scale().max(1.0);
            let dis_bounds = dis_node.window_bounds().expect("disabled bounds");
            let dis_pt = Point::new(
                CssPx((dis_bounds.origin.x.0 + dis_bounds.size.width.0 * 0.5) / scale),
                CssPx((dis_bounds.origin.y.0 + dis_bounds.size.height.0 * 0.5) / scale),
            );
            for action in [PointerAction::Pressed, PointerAction::Released] {
                harness.deliver_to_first(SurfaceEvent::Pointer {
                    action,
                    event: PointerEvent::mouse(dis_pt).with_button(PointerButton::Primary),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: space_key_event(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.settle(8);

            let dis_val = disabled_val_driver.borrow().expect("disabled signal");
            assert!(
                !dis_val.get_untracked(),
                "disabled ToggleSwitch must ignore click and Space activation"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(400.0, 300.0)
            .run_on(driver, move || {
                let gds = Gds::install(Scope::default());
                let _ = gds.appearance().install_window_sheet();

                let title_ref = NodeRef::new();
                let dis_track = NodeRef::new();
                let dis_checked = RwSignal::new_local(false);

                *disabled_slot.borrow_mut() = Some(dis_track);
                *disabled_val_slot.borrow_mut() = Some(dis_checked);

                view! {
                    column(style:gap = "12px", style:padding = "16px") {
                        FieldLabel(node_ref = title_ref) {"Rounded corners"}
                        ToggleSwitch(labelled_by = title_ref)
                        ToggleSwitch(
                            checked = dis_checked,
                            disabled = true,
                            label = "Disabled switch",
                            node_ref = dis_track,
                        )
                    }
                }
            })
            .expect("labelled and disabled switches");
    }

    #[test]
    fn toggle_switch_click_and_space_animate_smoothly_across_140ms_and_reverse_cleanly() {
        let track_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
        let thumb_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
        let checked_slot: Rc<RefCell<Option<RwSignal<bool, LocalStorage>>>> =
            Rc::new(RefCell::new(None));
        let changes_slot: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let gds_slot: Rc<RefCell<Option<Gds>>> = Rc::new(RefCell::new(None));

        let track_driver = Rc::clone(&track_slot);
        let thumb_driver = Rc::clone(&thumb_slot);
        let checked_driver = Rc::clone(&checked_slot);
        let changes_driver = Rc::clone(&changes_slot);
        let gds_driver = Rc::clone(&gds_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(400.0),
                DevicePx(300.0),
            )));
            harness.settle(16);
            harness.advance(Duration::from_millis(32));
            harness.settle(8);

            let track = track_driver.borrow().expect("track ref");
            let thumb = thumb_driver.borrow().expect("thumb ref");
            let checked = checked_driver.borrow().expect("checked signal");
            let scale = track.scale().max(1.0);

            let thumb_offset_x = || -> f32 {
                let tr = track.window_bounds().expect("track bounds");
                let th = thumb.window_bounds().expect("thumb bounds");
                (th.origin.x.0 - tr.origin.x.0) / scale - 3.0
            };

            assert!(!checked.get_untracked());
            assert!(
                thumb_offset_x().abs() < 0.05,
                "initial off thumb travel must be 0px, got {}",
                thumb_offset_x()
            );

            // 1. Click ToggleSwitch -> flips false -> true and animates 0px -> 20px over 140ms.
            let tr = track.window_bounds().expect("track bounds");
            let click_pt = Point::new(
                CssPx((tr.origin.x.0 + tr.size.width.0 * 0.5) / scale),
                CssPx((tr.origin.y.0 + tr.size.height.0 * 0.5) / scale),
            );
            for action in [PointerAction::Pressed, PointerAction::Released] {
                harness.deliver_to_first(SurfaceEvent::Pointer {
                    action,
                    event: PointerEvent::mouse(click_pt).with_button(PointerButton::Primary),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.pump();

            assert!(checked.get_untracked(), "click must flip checked to true");
            assert_eq!(*changes_driver.borrow(), vec![true]);

            // Mid-flight at t = 48ms: thumb MUST be strictly between 0px and 20px!
            harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
            let mid_on_x = thumb_offset_x();
            assert!(
                mid_on_x > 4.0 && mid_on_x < 19.5,
                "thumb translateX at t=48ms must be mid-flight (0 < x < 20), got {mid_on_x}"
            );

            // Settled at t >= 160ms: thumb MUST rest cleanly at 20px!
            harness.run_for(Duration::from_millis(140), Duration::from_millis(16));
            let settled_on_x = thumb_offset_x();
            assert!(
                (settled_on_x - 20.0).abs() < 0.05,
                "settled checked thumb translateX must equal 20px, got {settled_on_x}"
            );

            // 2. Press Space while focused -> flips true -> false ONCE and animates 20px -> 0px!
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: space_key_event(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.pump();

            assert!(
                !checked.get_untracked(),
                "Space KeyDown must flip checked from true to false without double-activating"
            );
            assert_eq!(*changes_driver.borrow(), vec![true, false]);

            harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
            let mid_off_x = thumb_offset_x();
            assert!(
                mid_off_x > 0.5 && mid_off_x < 16.0,
                "reverse thumb translateX at t=48ms must be mid-flight (0 < x < 20), got {mid_off_x}"
            );

            harness.run_for(Duration::from_millis(140), Duration::from_millis(16));
            let settled_off_x = thumb_offset_x();
            assert!(
                settled_off_x.abs() < 0.05,
                "settled unchecked thumb translateX must return to 0px, got {settled_off_x}"
            );

            // 3. Mid-flight reversal via Enter key: start off -> on for 32ms, then reverse mid-flight!
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: enter_key_event(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.pump();
            assert!(checked.get_untracked());

            harness.run_for(Duration::from_millis(32), Duration::from_millis(16));
            let before_reverse_x = thumb_offset_x();
            assert!(before_reverse_x > 2.0 && before_reverse_x < 19.0);

            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: space_key_event(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.pump();
            assert!(!checked.get_untracked());

            harness.run_for(Duration::from_millis(16), Duration::from_millis(16));
            let reversed_step_x = thumb_offset_x();
            assert!(
                reversed_step_x < before_reverse_x && reversed_step_x > 0.0,
                "mid-flight reversal must smoothly head back toward 0px from {before_reverse_x}, got {reversed_step_x}"
            );

            harness.run_for(Duration::from_millis(160), Duration::from_millis(16));
            assert!(thumb_offset_x().abs() < 0.05);

            // 4. Mid-frame `gds-appearance` stylesheet re-installation (e.g. toggling
            // `rounded_corners` on Page::Appearance) must NOT snap or interrupt 140ms motion!
            let gds = gds_driver.borrow().clone().expect("gds");
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: space_key_event(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.pump();
            let next_appearance = crate::appearance::theme::Appearance {
                source: crate::appearance::theme::Source::Builtin,
                rounded_corners: true,
                ..Default::default()
            };
            gds.appearance()
                .theme()
                .apply(&next_appearance)
                .expect("apply rounded_corners");
            let _ = gds.appearance().install_window_sheet();

            harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
            let mid_after_sheet_x = thumb_offset_x();
            assert!(
                mid_after_sheet_x > 4.0 && mid_after_sheet_x < 19.5,
                "mid-frame gds-appearance stylesheet update must preserve smooth mid-flight travel, got {mid_after_sheet_x}"
            );
            harness.run_for(Duration::from_millis(140), Duration::from_millis(16));
            assert!((thumb_offset_x() - 20.0).abs() < 0.05);

            // 5. Reduced motion snaps immediately in 0ms.
            gds.appearance().motion().set_enabled(false);
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: space_key_event(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.settle(8);
            assert!(!checked.get_untracked());
            assert!(
                thumb_offset_x().abs() < 0.05,
                "reduced motion must snap immediately to 0px, got {}",
                thumb_offset_x()
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(400.0, 300.0)
            .run_on(driver, move || {
                let gds = Gds::install(Scope::default());
                let _ = gds.appearance().install_window_sheet();
                *gds_slot.borrow_mut() = Some(gds);

                let track = NodeRef::new();
                let thumb = NodeRef::new();
                let checked = RwSignal::new_local(false);
                let changes = Rc::clone(&changes_slot);

                *track_slot.borrow_mut() = Some(track);
                *thumb_slot.borrow_mut() = Some(thumb);
                *checked_slot.borrow_mut() = Some(checked);

                view! {
                    box(style:padding = "24px") {
                        {render_toggle_switch(SwitchConfig {
                            checked: checked.into(),
                            default_checked: false,
                            on_change: Some(UnsyncCallback::new(move |next| {
                                changes.borrow_mut().push(next);
                            })),
                            disabled: Signal::stored_local(false),
                            size: ToggleSwitchSize::Md,
                            label: Some("Enable animations".to_owned()),
                            labelled_by: None,
                            node_ref: Some(track),
                            thumb_ref: Some(thumb),
                            class: Classes::default(),
                            attrs: Attrs::new(),
                        })}
                    }
                }
            })
            .expect("ToggleSwitch 140ms motion test ran");
    }
}
