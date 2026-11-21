use zgui::prelude::*;
use zgui::reactive::{Mounted, RwSignal, install};
use zgui::view;
use zgui::vocab::{Role, SemanticFlags, Toggled};

use super::{
    ApplicationMenuButton, ApplicationMenuButtonProps, Button, ButtonProps, ButtonSize,
    ButtonVariant, IconButton, IconButtonProps, apply_toggled, button_attrs,
};
use crate::ui::Shortcut;
use crate::ui::about::Application;
use crate::ui::actions::Command;
use crate::ui::icon::IconName;
use crate::ui::keytips::{KeytipHostProps, KeytipRegistry};

#[test]
fn button_variants_reexport_zgui_ui_types() {
    let _ = (
        ButtonVariant::Default,
        ButtonSize::Icon,
        Button,
        IconButton,
        ApplicationMenuButton,
    );
}

#[test]
fn untoggled_buttons_omit_a11y_toggled_while_toggled_buttons_set_it() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let push = apply_toggled(Attrs::new().a11y_from(A11yBinding::new(Role::Button)), None);
        assert_eq!(
            push.a11y().map(|a| {
                let lowered = a.lower();
                (lowered.role, lowered.toggled)
            }),
            Some((Role::Button, None))
        );

        let toggled_on = apply_toggled(
            Attrs::new().a11y_from(A11yBinding::new(Role::Button)),
            Some(Signal::stored_local(true)),
        );
        assert_eq!(
            toggled_on.a11y().map(|a| {
                let lowered = a.lower();
                (lowered.role, lowered.toggled)
            }),
            Some((Role::Button, Some(Toggled::True)))
        );

        let _push_view = view! { Button { "Cancel" } };
        let _toggle_view = view! { Button(toggled = true) { "Mute" } };
        let _icon_push = view! { IconButton(icon = IconName::Menu, label = "Menu") };
        let _icon_toggle =
            view! { IconButton(icon = IconName::Pin, label = "Pin", toggled = false) };
    });
    scope.unmount();
}

#[test]
fn button_attrs_expose_loading_busy_and_keyboard_shortcut() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let loading = RwSignal::new_local(false);
        let active = RwSignal::new_local(true);
        let shortcut = Shortcut::parse("Ctrl+S").expect("valid Ctrl+S shortcut");

        let attrs = button_attrs(
            Attrs::new().a11y_from(A11yBinding::new(Role::Button)),
            None,
            loading.into(),
            active.into(),
            Some(&shortcut),
        );

        let initial = attrs
            .a11y()
            .expect("button_attrs sets accessibility binding")
            .lower();
        assert_eq!(initial.role, Role::Button);
        assert_eq!(initial.toggled, None);
        assert!(!initial.flags.contains(SemanticFlags::BUSY));
        assert_eq!(initial.keyboard_shortcut.as_deref(), Some("Ctrl+S"));

        loading.set(true);
        let busy = attrs
            .a11y()
            .expect("button_attrs updates accessibility binding")
            .lower();
        assert!(busy.flags.contains(SemanticFlags::BUSY));
    });
    scope.unmount();
}

#[test]
fn button_command_and_shortcut_enabled_wire_cleanly() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let _views = view! {
            KeytipHost {
                {{
                    if let Some(reg) = KeytipRegistry::current() {
                        assert!(!reg.active().get_untracked());
                    }
                    view! {
                        column {
                            Button(
                                icon = IconName::Check,
                                loading = false,
                                active = true,
                                tooltip = "Save",
                                shortcut = Shortcut::parse("Ctrl+S").expect("valid shortcut"),
                            ) { "Save" }
                            Button(
                                command = Command::FocusNotifications,
                            ) { "Notifications" }
                            IconButton(
                                icon = IconName::Close,
                                label = "Close tab",
                                shortcut = Shortcut::parse("Ctrl+W").expect("Ctrl+W"),
                                shortcut_enabled = false,
                                size = ButtonSize::IconSm,
                            )
                            ApplicationMenuButton(
                                label = "Main menu",
                                application = Some(Application::cargo("gds-files")),
                                shortcut = Shortcut::parse("Alt+M").expect("valid Alt+M shortcut"),
                            )
                        }
                    }
                }}
            }
        };
    });
    scope.unmount();
}

#[test]
fn headless_buttons_verify_36px_32px_inline_keytip_and_push_vs_toggle_roles() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use zgui::geom::Point;
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::runtime::AppError;
    use zgui::vocab::{
        KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction,
        PointerButton, PointerEvent, Timestamp,
    };
    use zgui_platform_headless::Harness;

    #[derive(Debug, Default)]
    struct HeadlessSink {
        atlas: zgui::atlas::MemorySink,
        target: Option<zgui::render::RenderTarget>,
    }

    impl zgui::render::Renderer for HeadlessSink {
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
            _texture: zgui::render::ExternalTexture,
        ) -> zgui::render::TextureHandle {
            zgui::render::TextureHandle(0)
        }

        fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}

        fn memory(&self) -> zgui::render::MemoryReport {
            zgui::render::MemoryReport::default()
        }

        fn draw(
            &mut self,
            _scene: &zgui::scene::Scene,
            _damage: &zgui::bits::DamageSet,
        ) -> zgui::render::FrameOutcome {
            zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
        }

        fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
            &mut self.atlas
        }
    }

    let push_clicks = Rc::new(Cell::new(0usize));
    let loading_clicks = Rc::new(Cell::new(0usize));
    let push_ref_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
    let loading_ref_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
    let icon_ref_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));

    let push_clicks_view = Rc::clone(&push_clicks);
    let loading_clicks_view = Rc::clone(&loading_clicks);
    let push_slot_view = Rc::clone(&push_ref_slot);
    let loading_slot_view = Rc::clone(&loading_ref_slot);
    let icon_slot_view = Rc::clone(&icon_ref_slot);

    let push_clicks_check = Rc::clone(&push_clicks);
    let loading_clicks_check = Rc::clone(&loading_clicks);
    let push_slot_check = Rc::clone(&push_ref_slot);
    let loading_slot_check = Rc::clone(&loading_ref_slot);
    let icon_slot_check = Rc::clone(&icon_ref_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.pump();
        if let Some(surface) = harness.platform().offscreens().first() {
            surface.set_a11y_listener(true);
        }
        harness.settle(4);

        // Verify 36px standard Button height and 32x32px IconButton target size.
        let push_node = push_slot_check.borrow().expect("push node");
        let push_bounds = push_node.window_bounds().expect("push bounds");
        let scale = push_node.scale().max(1.0);
        assert_eq!(
            (push_bounds.size.height.0 / scale).round(),
            36.0,
            "standard ButtonSize::Md height must be 36px"
        );
        let initial_width = push_bounds.size.width.0 / scale;

        let icon_node = icon_slot_check.borrow().expect("icon node");
        let icon_bounds = icon_node.window_bounds().expect("icon bounds");
        assert_eq!(
            (icon_bounds.size.width.0 / scale).round(),
            32.0,
            "IconButton width must be 32px"
        );
        assert_eq!(
            (icon_bounds.size.height.0 / scale).round(),
            32.0,
            "IconButton height must be 32px"
        );

        // Holding Ctrl animates the inline keytip suffix wider over 140ms, and releasing Ctrl
        // returns width back to `initial_width`.
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::named(NamedKey::Control, PhysicalKey::Code(KeyCode::ControlLeft)),
            modifiers: Modifiers::CONTROL,
            timestamp: Timestamp::ORIGIN,
        });
        harness.run_for(Duration::from_millis(160), Duration::from_millis(16));
        let expanded_width = push_node
            .window_bounds()
            .expect("expanded bounds")
            .size
            .width
            .0
            / scale;
        assert!(
            expanded_width > initial_width,
            "holding Ctrl must expand inline button keytip suffix ({expanded_width} > {initial_width})"
        );

        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event: KeyEvent::named(NamedKey::Control, PhysicalKey::Code(KeyCode::ControlLeft)),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(160)),
        });
        harness.run_for(Duration::from_millis(160), Duration::from_millis(16));
        let restored_width = push_node
            .window_bounds()
            .expect("restored bounds")
            .size
            .width
            .0
            / scale;
        assert!(
            (restored_width - initial_width).abs() <= 0.5,
            "releasing Ctrl must collapse inline suffix back to initial width ({restored_width} vs {initial_width})"
        );

        let click_node = |harness: &mut Harness<Box<dyn AppHandler>>,
                          slot: &RefCell<Option<NodeRef>>| {
            let node = slot.borrow().expect("node ref populated");
            let bounds = node.window_bounds().expect("laid-out bounds");
            let scale = node.scale().max(1.0);
            let cx = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
            let cy = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;
            let at = Point::new(zgui::geom::CssPx(cx), zgui::geom::CssPx(cy));
            for (action, event) in [
                (PointerAction::Moved, PointerEvent::mouse(at)),
                (
                    PointerAction::Pressed,
                    PointerEvent::mouse(at).with_button(PointerButton::Primary),
                ),
                (
                    PointerAction::Released,
                    PointerEvent::mouse(at).with_button(PointerButton::Primary),
                ),
            ] {
                harness.deliver_to_first(SurfaceEvent::Pointer {
                    action,
                    event,
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.settle(4);
        };

        click_node(&mut harness, &push_slot_check);
        assert_eq!(push_clicks_check.get(), 1);

        click_node(&mut harness, &loading_slot_check);
        assert_eq!(
            loading_clicks_check.get(),
            0,
            "loading button must block clicks"
        );

        let offscreen = harness.platform().offscreens()[0].clone();
        let log = offscreen.a11y_log();
        assert!(!log.is_empty(), "accessibility updates emitted");
        let a11y_nodes: Vec<(Option<String>, String, Option<String>, bool, bool)> = log
            .iter()
            .flat_map(|update| update.nodes.iter())
            .map(|(_, node)| {
                (
                    node.label().map(str::to_owned),
                    format!("{:?}", node.role()),
                    node.toggled().map(|t| format!("{t:?}")),
                    node.is_disabled(),
                    node.is_busy(),
                )
            })
            .collect();
        assert!(
            a11y_nodes.iter().any(|(label, role, toggled, _, _)| {
                label.as_deref() == Some("Run") && role == "Button" && toggled.is_none()
            }),
            "expected Run push button with toggled == None in {a11y_nodes:?}"
        );
        assert!(
            a11y_nodes.iter().any(|(label, role, toggled, _, _)| {
                label.as_deref() == Some("Menu") && role == "Button" && toggled.is_none()
            }),
            "expected ApplicationMenuButton push button with toggled == None in {a11y_nodes:?}"
        );
        assert!(
            a11y_nodes.iter().any(|(label, role, _, disabled, busy)| {
                label.as_deref() == Some("Saving") && role == "Button" && *disabled && *busy
            }),
            "expected Saving loading button to expose disabled+busy in {a11y_nodes:?}"
        );
        assert!(
            a11y_nodes.iter().any(|(label, role, toggled, _, _)| {
                label.as_deref() == Some("Toggle pin")
                    && role == "Button"
                    && toggled.as_deref() == Some("True")
            }),
            "expected Toggle pin to set toggled == True in {a11y_nodes:?}"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_title("ui-controls-button-test")
        .with_size(480.0, 320.0)
        .with_renderer(Box::new(|_surface: &Arc<dyn Surface>, target| {
            let mut renderer = HeadlessSink::default();
            zgui::render::Renderer::configure(&mut renderer, target);
            Ok::<_, AppError>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
        }))
        .run_on(driver, move || {
            let push_ref = NodeRef::new();
            let loading_ref = NodeRef::new();
            let icon_ref = NodeRef::new();
            *push_slot_view.borrow_mut() = Some(push_ref);
            *loading_slot_view.borrow_mut() = Some(loading_ref);
            *icon_slot_view.borrow_mut() = Some(icon_ref);
            let on_push_clicks = Rc::clone(&push_clicks_view);
            let on_loading_clicks = Rc::clone(&loading_clicks_view);

            view! {
                KeytipHost {
                    column(style:align-items = "flex-start") {
                        Button(
                            node_ref = push_ref,
                            shortcut = Shortcut::parse("Ctrl+R").expect("Ctrl+R"),
                            on:click = move |_| on_push_clicks.set(on_push_clicks.get() + 1),
                        ) { "Run" }
                        Button(
                            node_ref = loading_ref,
                            loading = true,
                            on:click = move |_| on_loading_clicks.set(on_loading_clicks.get() + 1),
                        ) { "Saving" }
                        IconButton(
                            node_ref = icon_ref,
                            icon = IconName::Pin,
                            label = "Toggle pin",
                            toggled = true,
                        )
                        ApplicationMenuButton(label = "Menu")
                    }
                }
            }
        })
        .expect("headless button harness ran");
}
