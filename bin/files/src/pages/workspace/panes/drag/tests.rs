use std::{
    cell::RefCell,
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use super::coordinator::{ACTIVE_DRAG_SOURCES, DragSnapshot, FileDrag};
use crate::{
    entities::file::directory::{Entry, EntryDetails, EntryKind},
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

    use super::coordinator::{
        DragRejectReason, REJECT_CANCEL_WINDOW, clear_pending_drag_rejection,
        clear_pending_drag_rejection_if_left, mark_pending_drag_rejection_left,
        seed_pending_drag_rejection, take_active_drag_rejection,
    };

    let window_key = 0x1234_usize;
    let t0 = Instant::now();

    let seed = || {
        seed_pending_drag_rejection(DragRejectReason::IntoSelf, window_key);
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
        take_active_drag_rejection(t0 + REJECT_CANCEL_WINDOW + Duration::from_millis(50)).is_none()
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
    use crate::pages::workspace::{shell::Browser, state::LaunchOptions};
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
        let _cancel = crate::pages::workspace::operations::Lifecycle(&browser)
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
        crate::pages::workspace::operations::Lifecycle(&browser).end_operation();

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
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::Chinese).expect("Chinese catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
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

    use super::coordinator::{clear_pending_drag_rejection, coordinator_for};
    use crate::entities::location::{places::Places, volumes};
    use crate::entities::preferences;
    use crate::pages::workspace::{shell::Browser, state::LaunchOptions};
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
            .folder_target_node(&folder_a)
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
        coord.begin_native_drag(&browser, session_1, vec![folder_a.clone()]);
        assert!(browser.0.native_drag_active.get());

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
        assert!(!browser.0.native_drag_active.get());
        assert_eq!(
            browser.0.notice.get_untracked(),
            Some(files_i18n::text_untracked("drop-into-self"))
        );

        // 2. Hovering FolderA (rejected) and then moving onto neutral chrome before release
        // must clear the rejection so normal cancellation stays silent.
        let session_2 = DragSessionId::new(12);
        let offer_2 = DragOfferId::new(22);
        browser.0.notice.set(None);
        coord.begin_native_drag(&browser, session_2, vec![folder_a.clone()]);

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
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::Chinese).expect("Chinese catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
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

    use super::{
        coordinator::TabDragPreview,
        sidebar::{DraggedTabGhostProps, last_tab_ghost_node},
    };
    use crate::pages::workspace::projection::geometry::TAB_HEIGHT;

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

        let ghost_ref = last_tab_ghost_node().expect("tab ghost node recorded");
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
            crate::pages::workspace::shell::chrome::install_chrome_stylesheets();

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

    use super::{coordinator::DragItem, files::render_file_drag_card_body};
    use crate::entities::file::ui::FileIcon;

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
