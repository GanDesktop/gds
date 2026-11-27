use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gds::app::Gds;
use gds::i18n::Language;
use gds::service::task_manager::Supervisor;
use zgui::{
    app::Fonts,
    geom::{DevicePx, Size},
    platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
    prelude::*,
    runtime::AppError,
};
use zgui_platform_headless::Harness;

use super::{ConflictKind, take_conflict_test_hooks};
use crate::entities::location::{places::Places, volumes};
use crate::entities::preferences;
use crate::features::file_operations::{paste::PasteJob, transfer};
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

fn wait_until<A: AppHandler>(
    harness: &mut Harness<A>,
    timeout: Duration,
    mut pred: impl FnMut() -> bool,
) {
    let deadline = Instant::now() + timeout;
    while !pred() {
        assert!(Instant::now() < deadline, "timed out waiting for condition");
        harness.advance(Duration::from_millis(15));
        harness.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    harness.settle(4);
}

#[test]
fn initial_selection_end_selects_stem_before_simple_and_compound_extensions() {
    use super::{EntryKind, initial_selection_end};

    assert_eq!(
        initial_selection_end("report (2).pdf", EntryKind::File),
        "report (2)".len()
    );
    assert_eq!(
        initial_selection_end("archive (2).tar.gz", EntryKind::File),
        "archive (2)".len()
    );
    assert_eq!(
        initial_selection_end(".env (2)", EntryKind::File),
        ".env (2)".len()
    );
    assert_eq!(
        initial_selection_end("release.v2 (2)", EntryKind::Directory),
        "release.v2 (2)".len()
    );
}

#[test]
fn conflict_dialog_progressive_rename_live_validation_folder_merge_and_abort() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");
    fs::create_dir_all(src.join("Folder/nested")).expect("create src Folder");
    fs::create_dir_all(dst.join("Folder/nested")).expect("create dst Folder");
    fs::create_dir_all(dst.join("clash_dir")).expect("create dst clash_dir");

    fs::write(src.join("doc.txt"), b"new-doc").expect("src doc");
    fs::write(dst.join("doc.txt"), b"old-doc").expect("dst doc");
    fs::write(src.join("Folder/only_src.txt"), b"from-src").expect("src only_src");
    fs::write(src.join("Folder/shared.txt"), b"new-shared").expect("src shared");
    fs::write(dst.join("Folder/only_dst.txt"), b"from-dst").expect("dst only_dst");
    fs::write(dst.join("Folder/shared.txt"), b"old-shared").expect("dst shared");
    fs::write(src.join("clash_dir"), b"file-vs-dir").expect("src clash_dir file");

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = dst.clone();

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(960.0),
            DevicePx(640.0),
        )));
        harness.settle(16);
        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let _ = take_conflict_test_hooks();

        // 1. Same-directory copy auto-renames without opening any conflict dialog.
        browser.inspect_paste(
            PasteJob::new(dst.clone(), transfer::Mode::Copy, vec![dst.join("doc.txt")]),
            false,
            true,
            None,
        );
        wait_until(&mut harness, Duration::from_secs(3), || {
            !browser.read_untracked(|state| state.operation.busy())
        });
        assert!(take_conflict_test_hooks().is_none());
        assert_eq!(
            fs::read(dst.join("doc (2).txt")).expect("auto-renamed copy"),
            b"old-doc"
        );

        // 2. Multi-item cross-folder copy exercising progressive rename, live collision check,
        //    Back button, Folder Merge + Apply-to-all, nested relative_dir, Skip, TypeMismatch.
        browser.inspect_paste(
            PasteJob::new(
                dst.clone(),
                transfer::Mode::Copy,
                vec![
                    src.join("doc.txt"),
                    src.join("Folder"),
                    src.join("clash_dir"),
                ],
            ),
            false,
            true,
            None,
        );

        // Prompt 1: doc.txt (FileIntoFile) -> starts with Rename collapsed!
        let mut hooks = None;
        wait_until(&mut harness, Duration::from_secs(3), || {
            hooks = take_conflict_test_hooks();
            hooks.is_some()
        });
        let h1 = hooks.expect("doc.txt conflict hooks");
        assert_eq!(h1.kind, ConflictKind::FileIntoFile);
        assert_eq!(h1.remaining_after_current, 2);
        assert!(h1.has_apply_all);
        assert!(!h1.rename_mode.get_untracked());
        assert!(h1.merge.is_none());

        // Enter Rename mode -> live-rejects existing `doc.txt` and `doc (2).txt`, accepts free name.
        (h1.enter_or_auto_rename)();
        harness.settle(8);
        assert!(h1.rename_mode.get_untracked());
        assert!(!h1.rename_invalid.get_untracked());
        h1.rename_value.set("doc.txt".to_owned());
        harness.settle(8);
        assert!(h1.rename_invalid.get_untracked());
        h1.rename_value.set("doc (2).txt".to_owned());
        harness.settle(8);
        assert!(h1.rename_invalid.get_untracked());
        h1.rename_value.set("doc-fresh.txt".to_owned());
        harness.settle(8);
        assert!(!h1.rename_invalid.get_untracked());

        // Return from Rename mode cleanly, then choose Replace!
        (h1.exit_rename.expect("back from rename"))();
        harness.settle(8);
        assert!(!h1.rename_mode.get_untracked());
        (h1.replace.expect("replace button available"))();

        // Prompt 2: Folder (DirectoryIntoDirectory) -> check Apply-to-all + Merge.
        let mut hooks = None;
        wait_until(&mut harness, Duration::from_secs(3), || {
            hooks = take_conflict_test_hooks();
            hooks.is_some()
        });
        let h2 = hooks.expect("Folder conflict hooks");
        assert_eq!(h2.kind, ConflictKind::DirectoryIntoDirectory);
        assert!(!h2.rename_mode.get_untracked());
        assert!(h2.replace.is_none());
        h2.apply_all.set(true);
        (h2.merge.expect("merge button available"))();

        // Prompt 3: Folder/shared.txt (FileIntoFile) -> carries relative_dir == Some("Folder")!
        let mut hooks = None;
        wait_until(&mut harness, Duration::from_secs(3), || {
            hooks = take_conflict_test_hooks();
            hooks.is_some()
        });
        let h3 = hooks.expect("shared.txt conflict hooks");
        assert_eq!(h3.kind, ConflictKind::FileIntoFile);
        assert_eq!(h3.relative_dir, Some(PathBuf::from("Folder")));
        (h3.skip)();

        // Prompt 4: clash_dir (TypeMismatch) -> starts with Rename ALREADY open!
        let mut hooks = None;
        wait_until(&mut harness, Duration::from_secs(3), || {
            hooks = take_conflict_test_hooks();
            hooks.is_some()
        });
        let h4 = hooks.expect("clash_dir conflict hooks");
        assert_eq!(h4.kind, ConflictKind::TypeMismatch);
        assert!(h4.rename_mode.get_untracked());
        assert!(h4.exit_rename.is_none());
        assert!(h4.replace.is_none());
        assert!(h4.merge.is_none());
        h4.rename_value.set("clash_file".to_owned());
        harness.settle(8);
        assert!(!h4.rename_invalid.get_untracked());
        (h4.confirm_rename)();

        wait_until(&mut harness, Duration::from_secs(3), || {
            !browser.read_untracked(|state| state.operation.busy())
        });

        assert_eq!(
            fs::read(dst.join("doc.txt")).expect("replaced doc"),
            b"new-doc"
        );
        assert_eq!(
            fs::read(dst.join("Folder/only_src.txt")).expect("merged new file"),
            b"from-src"
        );
        assert_eq!(
            fs::read(dst.join("Folder/only_dst.txt")).expect("existing dst file kept"),
            b"from-dst"
        );
        assert_eq!(
            fs::read(dst.join("Folder/shared.txt")).expect("skipped file kept"),
            b"old-shared"
        );
        assert!(dst.join("clash_dir").is_dir());
        assert_eq!(
            fs::read(dst.join("clash_file")).expect("renamed mismatch file"),
            b"file-vs-dir"
        );

        // 3. Aborting a single-file conflict prompt cancels cleanly without changing files.
        fs::write(dst.join("doc.txt"), b"keep-before-abort").expect("reset doc.txt");
        browser.0.notice.set(None);
        browser.inspect_paste(
            PasteJob::new(dst.clone(), transfer::Mode::Copy, vec![src.join("doc.txt")]),
            false,
            true,
            None,
        );
        let mut hooks = None;
        wait_until(&mut harness, Duration::from_secs(3), || {
            hooks = take_conflict_test_hooks();
            hooks.is_some()
        });
        let h_abort = hooks.expect("abort prompt hooks");
        assert!(!h_abort.has_apply_all);
        assert_eq!(h_abort.remaining_after_current, 0);
        (h_abort.abort)();
        wait_until(&mut harness, Duration::from_secs(3), || {
            !browser.read_untracked(|state| state.operation.busy())
        });
        assert_eq!(
            fs::read(dst.join("doc.txt")).expect("aborted target untouched"),
            b"keep-before-abort"
        );
        assert_eq!(
            browser.0.notice.get_untracked(),
            Some(files_i18n::text_untracked("paste-cancelled"))
        );

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
        .expect("conflict dialog end-to-end test ran");
}

#[cfg(feature = "files-e2e")]
#[test]
fn conflict_dialog_gpu_layout_keeps_footer_single_row_across_states() {
    use std::path::Path;

    use zgui::render::{RenderTarget, Renderer};
    use zgui_render_wgpu::{SharedGraphics, wgpu};

    let graphics = SharedGraphics::new();
    if graphics.open_gpu().is_err() {
        return;
    }
    let window_graphics = graphics;

    let temp = tempfile::tempdir().expect("tempdir");
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");
    fs::create_dir_all(src.join("Project/sub")).expect("create src");
    fs::create_dir_all(dst.join("Project/sub")).expect("create dst");
    fs::write(src.join("Project/sub/config.json"), vec![b'x'; 2400]).expect("write src file");
    fs::write(dst.join("Project/sub/config.json"), vec![b'y'; 1200]).expect("write dst file");
    fs::write(src.join("extra.txt"), b"extra").expect("write extra");

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let renderer_slot: Rc<RefCell<Option<Arc<std::sync::Mutex<()>>>>> = Rc::default();
    let _ = renderer_slot;
    let start = dst.clone();
    let preview_dir = std::env::var_os("GDS_CONFLICT_PREVIEW_DIR").map(PathBuf::from);
    let handle_slot: Rc<RefCell<Option<WindowHandle>>> = Rc::default();
    let handle_view = Rc::clone(&handle_slot);
    let handle_driver = Rc::clone(&handle_slot);

    let save_png = |dir: &Path, name: &str, rgba: &[u8], width: u32, height: u32| {
        let _ = fs::create_dir_all(dir);
        let img =
            image::RgbaImage::from_raw(width, height, rgba.to_vec()).expect("valid rgba frame");
        img.save(dir.join(name)).expect("save conflict preview png");
    };

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(960.0),
            DevicePx(640.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let _ = take_conflict_test_hooks();

        browser.inspect_paste(
            PasteJob::new(
                dst.clone(),
                transfer::Mode::Copy,
                vec![src.join("Project/sub/config.json"), src.join("extra.txt")],
            ),
            false,
            true,
            None,
        );
        // Trigger direct file conflict inside dst/Project/sub
        wait_until(&mut harness, Duration::from_secs(3), || {
            !browser.read_untracked(|state| state.operation.busy())
        });
        browser.inspect_paste(
            PasteJob::new(
                dst.join("Project/sub"),
                transfer::Mode::Copy,
                vec![src.join("Project/sub/config.json"), src.join("extra.txt")],
            ),
            false,
            true,
            None,
        );

        let mut hooks = None;
        wait_until(&mut harness, Duration::from_secs(3), || {
            hooks = take_conflict_test_hooks();
            hooks.is_some()
        });
        let h = hooks.expect("conflict dialog mounted");
        harness.settle(8);

        let check_single_row = |label: &str, footer_ref: NodeRef, body_ref: NodeRef| {
            let body_rect = body_ref.window_bounds().expect("dialog body bounds");
            let footer_rect = footer_ref.window_bounds().expect("footer row bounds");
            assert!(
                footer_rect.size.height.0 <= 44.0,
                "{label}: footer actions wrapped onto multiple rows (height = {:.1}px, body width = {:.1}px)",
                footer_rect.size.height.0,
                body_rect.size.width.0,
            );
        };

        let render_preview = |filename: &str, check_apply_all: bool, open_rename: bool| {
            let Some(dir) = &preview_dir else {
                return;
            };
            let window_guard = handle_driver.borrow();
            let Some(window) = window_guard.as_ref() else {
                return;
            };
            let snap_browser = browser.clone();
            if let Ok(rendered) = window.render_view(
                zgui::prelude::ViewSnapshotOptions::new().with_max_size(620.0, 620.0),
                move || {
                    if let crate::pages::workspace::state::model::Prompt::Conflict {
                        mode,
                        conflict,
                        resolve,
                        ..
                    } = snap_browser.0.prompt.get_untracked()
                    {
                        let body = super::render_conflict_dialog(
                            &snap_browser,
                            snap_browser.0.prompt_generation.get_untracked(),
                            mode,
                            conflict,
                            resolve,
                            &snap_browser.0.prompt_decisions,
                        );
                        if let Some(snap_hooks) = take_conflict_test_hooks() {
                            if check_apply_all {
                                snap_hooks.apply_all.set(true);
                            }
                            if open_rename {
                                snap_hooks.rename_mode.set(true);
                            }
                        }
                        zgui::view! {
                            box(
                                class = "zui-dialog gds-dialog-content gds-files-conflict-modal",
                                attr:data-size = "md",
                                style:padding = "20px",
                            ) {
                                {body}
                            }
                        }
                    } else {
                        zgui::view! { box() {} }
                    }
                },
            ) {
                save_png(
                    dir,
                    filename,
                    rendered.image.rgba(),
                    rendered.image.width(),
                    rendered.image.height(),
                );
            }
        };

        // 1. Default FileIntoFile state (unchecked)
        check_single_row("file-default", h.footer_row_node, h.body_node);
        assert!(
            h.batch_node.window_bounds().is_some(),
            "apply-all checkbox must be visible in decision state"
        );
        render_preview("conflict-file-default.png", false, false);

        // 2. Apply-to-all checked (batch labels + single-line Keep-both hint stay compact!)
        h.apply_all.set(true);
        harness.settle(8);
        check_single_row("file-apply-all", h.footer_row_node, h.body_node);
        assert!(
            h.batch_node.window_bounds().is_some(),
            "apply-all checkbox + hint must stay visible when checked"
        );
        render_preview("conflict-file-apply-all.png", true, false);

        // 3. Uncheck Apply-to-all and enter Rename mode -> batch checkbox must hide!
        h.apply_all.set(false);
        harness.settle(8);
        (h.enter_or_auto_rename)();
        harness.settle(8);
        check_single_row("file-rename-mode", h.footer_row_node, h.body_node);
        assert!(
            h.batch_node.window_bounds().is_none(),
            "apply-all checkbox must hide while manually renaming a single file"
        );
        render_preview("conflict-file-rename.png", false, true);

        (h.abort)();
        wait_until(&mut harness, Duration::from_secs(3), || {
            !browser.read_untracked(|state| state.operation.busy())
        });

        harness.shut_down();
        Ok(())
    };

    let offscreen_graphics = window_graphics.clone();
    zgui::app()
        .with_fonts(Fonts::system())
        .with_size(960.0, 640.0)
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
            let scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::Chinese).expect("Chinese catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            *handle_view.borrow_mut() = Some(use_window());
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
        .expect("conflict dialog GPU layout test ran");
}
