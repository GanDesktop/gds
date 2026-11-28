use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gds::{
    app::Gds,
    appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH},
    i18n::Language,
    service::task_manager::Scope,
    ui::window::WIDE_WIDTH,
};
use zgui::reactive::{
    Mounted, RenderEffect, RwSignal, Signal, flush, install, on_cleanup_local, prelude::*,
};

use super::{Browser, Target, selection_has_dotfile};
use crate::{
    entities::{
        location::{Location, places::Places, volumes},
        preferences::{self, Preferences},
    },
    pages::workspace::state::{
        context::{
            DEFAULT_WINDOW_WIDTH, DragSources, LaunchOptions, observe_window_title,
            resolve_initial_widths, sample_workspace_sidebar, should_notify_for_window_focus,
        },
        model::Pane,
    },
    shared::i18n,
};

#[test]
fn initial_window_width_defaults_to_desktop_width_and_collapses_only_in_micro_tier() {
    let (default_w, default_content_w) = resolve_initial_widths(None);
    assert_eq!(default_w, DEFAULT_WINDOW_WIDTH);
    assert!(default_w >= WIDE_WIDTH);
    assert_eq!(default_content_w, DEFAULT_WINDOW_WIDTH - SIDEBAR_WIDTH);

    let (pending_w, pending_content_w) = resolve_initial_widths(Some(0.0));
    assert_eq!(pending_w, DEFAULT_WINDOW_WIDTH);
    assert_eq!(pending_content_w, DEFAULT_WINDOW_WIDTH - SIDEBAR_WIDTH);

    let (compact_w, compact_content_w) = resolve_initial_widths(Some(860.0));
    assert_eq!(compact_w, 860.0);
    assert_eq!(compact_content_w, 860.0 - SIDEBAR_WIDTH);

    let (micro_w, micro_content_w) = resolve_initial_widths(Some(540.0));
    assert_eq!(micro_w, 540.0);
    assert_eq!(micro_content_w, 540.0 - COLLAPSED_SIDEBAR_WIDTH);
}

#[test]
fn workspace_sidebar_signal_synchronizes_manual_toggle_and_micro_breakpoint() {
    let _ = install();
    let root = Mounted::new();
    root.with(|| {
        let app = Gds::install(Scope::default());
        app.appearance().motion().set_enabled(false);

        let sidebar = RwSignal::new_local(true);
        let window_width = RwSignal::new_local(DEFAULT_WINDOW_WIDTH);
        let (presentation, width) = sample_workspace_sidebar(sidebar, window_width);

        assert_eq!(width.get_untracked(), SIDEBAR_WIDTH);
        assert!(!presentation.get_untracked().collapsed());

        // Compact width (600..1200) keeps user-expanded sidebar open.
        window_width.set(960.0);
        flush();
        assert_eq!(width.get_untracked(), SIDEBAR_WIDTH);
        assert!(!presentation.get_untracked().collapsed());

        // Micro width (< 600) locks the 56px collapsed rail without mutating `sidebar`.
        window_width.set(540.0);
        flush();
        assert_eq!(width.get_untracked(), COLLAPSED_SIDEBAR_WIDTH);
        assert!(presentation.get_untracked().collapsed());

        window_width.set(1280.0);
        flush();
        assert_eq!(width.get_untracked(), SIDEBAR_WIDTH);

        sidebar.set(false);
        flush();
        assert_eq!(width.get_untracked(), COLLAPSED_SIDEBAR_WIDTH);
    });
    root.unmount();
}

#[test]
fn live_window_title_effect_tracks_language_changes_without_redundant_writes() {
    let _ = install();
    let root = Mounted::new();
    let titles: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let titles_out = Rc::clone(&titles);

    root.with(|| {
        let app = Gds::install(Scope::default());
        app.i18n()
            .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
        let prefs = RwSignal::new_local(Preferences::default());
        let ready = RwSignal::new_local(false);

        observe_window_title(
            prefs.read_only(),
            move || ready.get(),
            move |title| titles_out.borrow_mut().push(title.to_owned()),
        );
        flush();
        assert!(titles.borrow().is_empty());

        ready.set(true);
        flush();
        let en_title = gds::i18n::text_untracked("app-title");
        assert_eq!(titles.borrow().as_slice(), std::slice::from_ref(&en_title));

        prefs.update(|current| current.files.show_hidden = true);
        flush();
        assert_eq!(titles.borrow().len(), 1);

        app.i18n()
            .install_catalog(i18n::prepare(Language::Chinese).expect("Chinese catalog"));
        prefs.update(|current| current.language = Some(Language::Chinese));
        flush();
        let zh_title = gds::i18n::text_untracked("app-title");
        assert_ne!(en_title, zh_title);
        assert_eq!(titles.borrow().as_slice(), [en_title.clone(), zh_title]);
    });
    root.unmount();
}

#[test]
fn active_drag_sources_reactively_track_dragged_paths_without_spurious_wakes() {
    let _ = install();
    let root = Mounted::new();
    root.with(|| {
        let sources = DragSources::new();
        let target = PathBuf::from("/tmp/demo/alpha.txt");
        let bystander = PathBuf::from("/tmp/demo/beta.txt");
        let target_for_sig = target.clone();
        let ghosted = Signal::derive_local(move || sources.contains(&target_for_sig));
        let wakes = Rc::new(Cell::new(0usize));
        let wakes_fx = Rc::clone(&wakes);
        let fx = RenderEffect::new(move |_| {
            let _ = ghosted.get();
            wakes_fx.set(wakes_fx.get() + 1);
        });
        on_cleanup_local(move || drop(fx));
        flush();

        assert!(!ghosted.get_untracked());
        assert_eq!(wakes.get(), 1);

        sources.set([target.clone()]);
        flush();
        assert!(ghosted.get_untracked());
        assert!(!sources.contains_untracked(&bystander));
        assert_eq!(wakes.get(), 2);

        sources.set([target.clone()]);
        flush();
        assert_eq!(wakes.get(), 2);

        sources.clear();
        flush();
        assert!(!ghosted.get_untracked());
        assert!(sources.is_empty_untracked());
        assert_eq!(wakes.get(), 3);
    });
    root.unmount();
}

#[test]
fn desktop_notifications_only_fire_when_a_real_window_is_unfocused() {
    assert!(!should_notify_for_window_focus(false, false));
    assert!(!should_notify_for_window_focus(false, true));
    assert!(!should_notify_for_window_focus(true, true));
    assert!(should_notify_for_window_focus(true, false));
}

#[test]
fn item_targets_group_by_parent_and_deduplicate_selections() {
    let targets = Target::items(
        ["/tmp/a", "/", "/tmp/folder", "/tmp/a"]
            .map(PathBuf::from)
            .to_vec(),
    );
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].directory, PathBuf::from("/tmp"));
    assert_eq!(
        targets[0].selection,
        [PathBuf::from("/tmp/a"), PathBuf::from("/tmp/folder")]
    );
    assert_eq!(targets[1].directory, PathBuf::from("/"));
    assert_eq!(targets[1].selection, [PathBuf::from("/")]);
}

#[test]
fn folder_targets_open_each_directory_once_without_selection() {
    let targets = Target::folders(
        ["/tmp/folder", "/", "/tmp/folder"]
            .map(PathBuf::from)
            .to_vec(),
    );
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].directory, PathBuf::from("/tmp/folder"));
    assert!(targets[0].selection.is_empty());
    assert_eq!(targets[1].directory, PathBuf::from("/"));
    assert!(targets[1].selection.is_empty());
}

#[test]
fn selection_has_dotfile_detects_hidden_targets_only_on_leaf_filename() {
    assert!(!selection_has_dotfile(&[
        PathBuf::from("/home/tester/.config/visible.toml"),
        PathBuf::from("/tmp/report.txt"),
    ]));
    assert!(selection_has_dotfile(&[
        PathBuf::from("/tmp/report.txt"),
        PathBuf::from("/tmp/.gitignore"),
    ]));
}

fn with_headless_browser(start: PathBuf, body: impl FnOnce(&Browser) + 'static) {
    use std::sync::Arc;

    use gds::service::task_manager::Supervisor;
    use zgui::{
        app::Fonts,
        atlas::{MemorySink, TextureSink},
        bits::DamageSet,
        geom::{DevicePx, Size},
        platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
        render::{
            ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities,
            RenderTarget, Renderer, TextureHandle,
        },
        runtime::AppError,
        scene::Scene,
    };
    use zgui_platform_headless::Harness;

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

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let body_slot = RefCell::new(Some(body));

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(8);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        if let Some(body) = body_slot.borrow_mut().take() {
            browser.0.owner.with(|| body(&browser));
        }
        harness.settle(4);
        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
            let _prefs =
                preferences::Service::install(Preferences::default(), &scope).expect("preferences");
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
        .expect("headless browser harness");
    supervisor.shutdown();
}

#[test]
fn show_properties_command_has_no_browser_tab_page_or_details_side_effects() {
    use gds::service::desktop::file_manager::{Command, Request};

    let dir_a = tempfile::tempdir().expect("dir_a");
    let dir_b = tempfile::tempdir().expect("dir_b");
    let target_file = dir_b.path().join("report.txt");
    std::fs::write(&target_file, b"demo").expect("write target file");
    let dir_a_path = dir_a.path().to_path_buf();
    let dir_b_path = dir_b.path().to_path_buf();

    with_headless_browser(dir_a_path.clone(), move |browser| {
        browser.0.pages.update(|pages| pages.open(Pane::Settings));
        assert!(!browser.0.details.get_untracked());

        browser.apply_desktop_command(Command {
            request: Request::ShowProperties {
                paths: vec![target_file.clone()],
            },
            startup_id: "wayland-activation-token".to_owned(),
        });

        let (tab_count, active_loc, dir_a_details, dir_b_details) =
            browser.read_untracked(|state| {
                (
                    state.tabs.len(),
                    state.tab().navigation.location().clone(),
                    state
                        .places
                        .details_visible(Location::from(dir_a_path.as_path())),
                    state
                        .places
                        .details_visible(Location::from(dir_b_path.as_path())),
                )
            });
        assert_eq!(tab_count, 1, "ShowProperties must not open a new tab");
        assert_eq!(
            active_loc,
            Location::from(dir_a_path.as_path()),
            "ShowProperties must not change the active tab location"
        );
        assert!(
            browser.0.pages.with_untracked(|pages| pages.current()) == Pane::Settings,
            "ShowProperties must not call show_files or exit settings"
        );
        assert!(
            !browser.0.details.get_untracked(),
            "ShowProperties must not enable the main window details pane"
        );
        assert!(
            !dir_a_details && !dir_b_details,
            "ShowProperties must not persist details visibility in Places"
        );
    });
}

#[test]
fn show_folders_command_opens_virtual_and_local_folder_uris() {
    use gds::service::desktop::file_manager::{Command, Request};

    let dir_a = tempfile::tempdir().expect("dir_a");
    let dir_b = tempfile::tempdir().expect("dir_b");
    let dir_b_path = dir_b.path().to_path_buf();
    let dir_b_uri = gds::service::desktop::recent_files::encode_local_file_uri(&dir_b_path);

    with_headless_browser(dir_a.path().to_path_buf(), move |browser| {
        browser.apply_desktop_command(Command {
            request: Request::ShowFolders {
                uris: vec!["trash:///".to_owned(), dir_b_uri],
            },
            startup_id: String::new(),
        });

        let locations = browser.read_untracked(|state| {
            state
                .tabs
                .iter()
                .map(|tab| tab.navigation.location().clone())
                .collect::<Vec<_>>()
        });
        assert!(locations.contains(&Location::Trash));
        assert!(locations.contains(&Location::from(dir_b_path.as_path())));
    });
}

#[test]
fn activate_command_preserves_tabs_and_returns_to_files_view() {
    use gds::service::desktop::file_manager::{Command, Request};

    let dir_a = tempfile::tempdir().expect("dir_a");
    let dir_b = tempfile::tempdir().expect("dir_b");
    let dir_a_path = dir_a.path().to_path_buf();
    let dir_b_path = dir_b.path().to_path_buf();
    let dir_b_uri = gds::service::desktop::recent_files::encode_local_file_uri(&dir_b_path);

    with_headless_browser(dir_a_path, move |browser| {
        browser.apply_desktop_command(Command {
            request: Request::ShowFolders {
                uris: vec![dir_b_uri],
            },
            startup_id: String::new(),
        });
        let expected_tabs = browser.read_untracked(|state| {
            (
                state.active,
                state
                    .tabs
                    .iter()
                    .map(|tab| tab.navigation.location().clone())
                    .collect::<Vec<_>>(),
            )
        });
        assert_eq!(expected_tabs.1.len(), 2);

        browser.0.pages.update(|pages| pages.open(Pane::Settings));
        assert!(browser.pane_untracked() == Pane::Settings);

        browser.apply_desktop_command(Command {
            request: Request::Activate { new_window: false },
            startup_id: "xdg-activation-token".to_owned(),
        });
        assert!(
            browser.pane_untracked() == Pane::Files,
            "Activate with token must return browser to files view"
        );
        let actual_tabs = browser.read_untracked(|state| {
            (
                state.active,
                state
                    .tabs
                    .iter()
                    .map(|tab| tab.navigation.location().clone())
                    .collect::<Vec<_>>(),
            )
        });
        assert_eq!(
            actual_tabs, expected_tabs,
            "Activate must not alter existing tabs or active selection"
        );

        browser.0.pages.update(|pages| pages.open(Pane::Settings));
        browser.apply_desktop_command(Command {
            request: Request::Activate { new_window: false },
            startup_id: String::new(),
        });
        assert!(
            browser.pane_untracked() == Pane::Files,
            "Activate without token must return browser to files view"
        );
        let actual_tabs = browser.read_untracked(|state| {
            (
                state.active,
                state
                    .tabs
                    .iter()
                    .map(|tab| tab.navigation.location().clone())
                    .collect::<Vec<_>>(),
            )
        });
        assert_eq!(
            actual_tabs, expected_tabs,
            "Activate without token must leave existing tabs intact"
        );
    });
}
