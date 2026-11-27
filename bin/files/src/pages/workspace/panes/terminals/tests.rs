#[test]
fn headless_window_teardown_with_disposed_child_owner_and_pending_failed_shell_does_not_panic() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use gds::{
        app::Gds,
        i18n::Language,
        service::task_manager::{Scope, Supervisor},
    };
    use zgui::{
        app::Fonts,
        atlas::{MemorySink, TextureSink},
        bits::DamageSet,
        geom::{DevicePx, Size},
        platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
        prelude::GetUntracked,
        render::{
            ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities,
            RenderTarget, Renderer, TextureHandle,
        },
        runtime::AppError,
        scene::Scene,
    };
    use zgui_platform_headless::Harness;

    use super::TerminalDock;
    use crate::{
        entities::{
            location::{places::Places, volumes},
            preferences::{self, Preferences},
        },
        pages::workspace::{LaunchOptions, shell::Browser},
        shared::i18n as files_i18n,
    };

    #[derive(Default)]
    struct NoopRenderer {
        atlas: MemorySink,
        target: Option<RenderTarget>,
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

        fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
            TextureHandle(0)
        }

        fn release_external(&mut self, _: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::default()
        }

        fn draw(&mut self, _: &Scene, _: &DamageSet) -> FrameOutcome {
            FrameOutcome::Presented(FrameStats::default())
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            &mut self.atlas
        }
    }

    fn noop_renderer(
        _: &Arc<dyn Surface>,
        target: RenderTarget,
    ) -> Result<Box<dyn Renderer>, AppError> {
        let mut renderer = NoopRenderer::default();
        renderer.configure(target);
        Ok(Box::new(renderer))
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let driver_start = home.clone();

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(8);

        let browser = browser_slot.borrow().clone().expect("browser constructed");

        // Spawn inside a transient child Owner (mirroring a `<Show>` footer/header button),
        // then immediately dispose that child Owner before window teardown runs.
        let transient_button_owner = browser.0.owner.child();
        transient_button_owner.with(|| {
            TerminalDock(browser.context()).new_terminal_at(driver_start.clone());
        });
        transient_button_owner.cleanup();
        assert!(browser.0.terminal.get_untracked());
        assert_eq!(browser.0.terminals.inner.sessions.borrow().len(), 1);

        // Spawn a second session whose `missing-shell` transition (`Starting -> Failed`) stays
        // pending on the background service without settling UI frames before `shut_down()`.
        let second_transient_owner = browser.0.owner.child();
        second_transient_owner.with(|| {
            TerminalDock(browser.context()).new_terminal_at(driver_start.clone());
        });
        second_transient_owner.cleanup();
        assert_eq!(browser.0.terminals.inner.sessions.borrow().len(), 2);

        std::thread::sleep(Duration::from_millis(30));

        harness.shut_down();
        assert!(
            browser.0.terminals.inner.sealed.get(),
            "window teardown must seal the terminal dock before shutting down sessions"
        );
        assert!(browser.0.terminals.inner.sessions.borrow().is_empty());
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::English).expect("English catalog"));
            let mut prefs = Preferences::default();
            prefs.terminal.shell = Some(home.join("missing-shell"));
            let _prefs = preferences::Service::install(prefs, &scope).expect("preferences");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: home.clone(),
                    home: home.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("headless terminal teardown harness");
}
