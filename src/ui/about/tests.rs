use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::app::app;
use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::{Mounted, RwSignal, install};
use zgui::view;
use zgui::vocab::{KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, Timestamp};
use zgui_platform_headless::Harness;

use crate::app::Gds;
use crate::i18n::Language;
use crate::service::task_manager::Scope;

use super::{
    APPLICATIONS, AboutDialogProps, AboutProps, AboutViewProps, Application, HEIGHT, Section,
    WIDTH, display_url, is_back_key, is_http_url, localized, open_about_dialog, test_probe,
};

#[derive(Default)]
struct NullRenderer {
    target: Option<zgui::render::RenderTarget>,
    atlas: zgui::atlas::MemorySink,
}

impl zgui::render::Renderer for NullRenderer {
    fn capabilities(&self) -> zgui::render::RenderCapabilities {
        zgui::render::RenderCapabilities::MINIMAL
    }

    fn configure(&mut self, target: zgui::render::RenderTarget) {
        self.target = Some(target);
    }

    fn target(&self) -> Option<zgui::render::RenderTarget> {
        self.target
    }

    fn draw(
        &mut self,
        _scene: &zgui::scene::Scene,
        _damage: &zgui::bits::DamageSet,
    ) -> zgui::render::FrameOutcome {
        zgui::render::FrameOutcome::Presented(zgui::render::FrameStats {
            vector_passes: 0,
            draw_calls: 0,
            damage_px: 0,
            bytes_uploaded: 0,
            memory: zgui::render::MemoryReport::ZERO,
        })
    }

    fn register_external(
        &mut self,
        _texture: zgui::render::ExternalTexture,
    ) -> zgui::render::TextureHandle {
        zgui::render::TextureHandle(0)
    }

    fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}

    fn memory(&self) -> zgui::render::MemoryReport {
        zgui::render::MemoryReport::ZERO
    }

    fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
        &mut self.atlas
    }
}

fn press_key(
    harness: &mut Harness<Box<dyn AppHandler>>,
    event: KeyEvent,
    modifiers: Modifiers,
    ms: u64,
) {
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Pressed,
        event: event.clone(),
        modifiers,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Released,
        event,
        modifiers,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms + 5)),
    });
    for _ in 0..4 {
        harness.pump();
    }
}

#[test]
fn registered_files_application_metadata_is_present() {
    let app_meta = Application::cargo("gds-files");
    assert_eq!(app_meta.name, "GDS Files");
    assert_eq!(app_meta.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(app_meta.build_tag, env!("GDS_BUILD_TAG"));
    #[cfg(debug_assertions)]
    assert!(
        app_meta.build_tag.ends_with("-debug"),
        "non-release builds must annotate build_tag, got {}",
        app_meta.build_tag
    );
    assert!(
        app_meta
            .artwork_svg()
            .is_some_and(|svg| svg.contains("<svg"))
    );
    assert!(
        app_meta
            .desktop_icon_svg()
            .is_some_and(|svg| svg.contains("<svg"))
    );
    assert!(
        app_meta
            .desktop_icon_image()
            .is_some_and(|img| img.url().starts_with("zgui-bytes:"))
    );
    assert!(app_meta.license_text.contains("Apache License"));
    assert_eq!(WIDTH, 420.0);
    assert_eq!(HEIGHT, 408.0);
    assert!(!APPLICATIONS.is_empty());
}

#[test]
fn about_url_guard_back_keys_and_localized_strings_work_with_and_without_gds() {
    assert!(is_http_url("https://example.com"));
    assert!(is_http_url(" http://localhost:8080 "));
    assert!(!is_http_url("file:///etc/passwd"));
    assert!(!is_http_url("javascript:alert(1)"));
    assert!(!is_http_url(""));

    assert_eq!(
        display_url("https://github.com/zortax/zgui/"),
        "github.com/zortax/zgui"
    );
    assert_eq!(display_url(" http://localhost:8080/ "), "localhost:8080");

    assert!(is_back_key(
        &zgui::vocab::Key::Named(NamedKey::Escape),
        Modifiers::NONE
    ));
    assert!(is_back_key(
        &zgui::vocab::Key::Named(NamedKey::ArrowLeft),
        Modifiers::ALT
    ));
    assert!(!is_back_key(
        &zgui::vocab::Key::Named(NamedKey::ArrowLeft),
        Modifiers::NONE
    ));

    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        assert_eq!(localized("gds-about-back", "Back"), "Back");
        let gds = Gds::install(Scope::default());
        let credits = Signal::derive_local(|| localized("gds-about-credits", "Fallback"));
        let license = Signal::derive_local(|| localized("gds-about-license", "Fallback"));
        let copied = Signal::derive_local(|| localized("gds-about-copied", "Fallback"));

        gds.i18n()
            .install(Language::English)
            .expect("English locale");
        assert_eq!(credits.get_untracked(), "Credits");
        assert_eq!(license.get_untracked(), "License");
        assert_eq!(copied.get_untracked(), "Copied");

        gds.i18n()
            .install(Language::Chinese)
            .expect("Chinese locale");
        assert_eq!(credits.get_untracked(), "致谢");
        assert_eq!(license.get_untracked(), "许可证");
        assert_eq!(copied.get_untracked(), "已复制");

        let app_meta = Application::cargo("gds-files");
        let _about_view = view! { About(application = app_meta) };
        let _alias_view = view! { AboutView(application = app_meta) };
        let open_sig = RwSignal::new_local(false);
        let _dialog_helper = open_about_dialog(open_sig, app_meta);
    });
    scope.unmount();
}

#[test]
fn escape_and_alt_left_return_from_subpages_before_dismissing_about_dialog() {
    let open_slot: Rc<RefCell<Option<RwSignal<bool, zgui::reactive::LocalStorage>>>> =
        Rc::new(RefCell::new(None));
    let open_for_view = Rc::clone(&open_slot);

    let handler = app()
        .with_title("about-dialog-test")
        .with_application_id("dev.gds.AboutTest")
        .with_size(640.0, 480.0)
        .with_stylesheet(":root { display: flex; width: 100%; height: 100%; }")
        .with_renderer(Box::new(
            move |_surface: &std::sync::Arc<dyn Surface>, target: zgui::render::RenderTarget| {
                let mut renderer = NullRenderer::default();
                zgui::render::Renderer::configure(&mut renderer, target);
                Ok::<_, zgui::Error>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
            },
        ))
        .with_context(|| {
            let gds = Gds::install(Scope::default());
            gds.i18n().install(Language::English).expect("English");
        })
        .into_handler(move || {
            let open = RwSignal::new_local(true);
            *open_for_view.borrow_mut() = Some(open);
            view! {
                AboutDialog(
                    open = open,
                    application = Application::cargo("gds-files"),
                )
            }
        })
        .expect("build headless about dialog test");

    handler
        .drive(|app| -> Result<(), PlatformError> {
            let mut harness = Harness::new(app);
            for _ in 0..4 {
                harness.pump();
            }
            harness.deliver_to_first(SurfaceEvent::Focused(true));
            harness.pump();

            let open = open_slot.borrow().expect("open signal");
            assert!(open.get_untracked());

            test_probe::PROBE.with(|slot| {
                let borrowed = slot.borrow();
                let probe = borrowed.as_ref().expect("about probe");
                assert_eq!(probe.section.get_untracked(), Section::Overview);
                probe.section.set(Section::License);
                probe.root_ref.focus();
            });
            for _ in 0..4 {
                harness.pump();
            }

            // First Escape pops License -> Overview while keeping AboutDialog open.
            press_key(
                &mut harness,
                KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                Modifiers::NONE,
                10,
            );
            test_probe::PROBE.with(|slot| {
                let borrowed = slot.borrow();
                let probe = borrowed.as_ref().expect("about probe");
                assert_eq!(probe.section.get_untracked(), Section::Overview);
            });
            assert!(
                open.get_untracked(),
                "Escape on License subpage must return to Overview without closing AboutDialog"
            );

            // Alt+Left from Credits also returns to Overview while keeping dialog open.
            test_probe::PROBE.with(|slot| {
                let borrowed = slot.borrow();
                let probe = borrowed.as_ref().expect("about probe");
                probe.section.set(Section::Credits);
                probe.root_ref.focus();
            });
            for _ in 0..4 {
                harness.pump();
            }
            press_key(
                &mut harness,
                KeyEvent::named(NamedKey::ArrowLeft, PhysicalKey::Code(KeyCode::ArrowLeft)),
                Modifiers::ALT,
                30,
            );
            test_probe::PROBE.with(|slot| {
                let borrowed = slot.borrow();
                let probe = borrowed.as_ref().expect("about probe");
                assert_eq!(probe.section.get_untracked(), Section::Overview);
            });
            assert!(open.get_untracked());

            // Alt+C dismisses AboutDialog cleanly from Overview.
            press_key(&mut harness, KeyEvent::character("c"), Modifiers::ALT, 50);
            assert!(!open.get_untracked(), "Alt+C must dismiss AboutDialog");

            harness.shut_down();
            Ok(())
        })
        .expect("drive headless about dialog test");
}
