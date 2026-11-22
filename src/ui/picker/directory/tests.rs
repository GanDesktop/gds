use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use zgui::app::app;
use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::{Mounted, RwSignal, UnsyncCallback, flush, install};
use zgui::view;
use zgui::vocab::{KeyEvent, KeyState, Modifiers, Timestamp};
use zgui_platform_headless::Harness;

use super::{
    DirectoryPicker, DirectoryPickerDialogProps, Entry, Location, MIN_SCROLLABLE_ROWS, Page,
    Request,
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

struct PendingReply {
    send: Option<tokio::sync::oneshot::Sender<Result<Page, String>>>,
}

fn controlled_loader() -> (super::Loader, Rc<RefCell<Vec<(Request, PendingReply)>>>) {
    let queue: Rc<RefCell<Vec<(Request, PendingReply)>>> = Rc::new(RefCell::new(Vec::new()));
    let q_clone = Rc::clone(&queue);
    let loader: super::Loader = Rc::new(
        move |req: Request| -> Pin<Box<dyn Future<Output = Result<Page, String>>>> {
            let (tx, rx) = tokio::sync::oneshot::channel();
            q_clone
                .borrow_mut()
                .push((req, PendingReply { send: Some(tx) }));
            Box::pin(async move {
                match rx.await {
                    Ok(res) => res,
                    Err(_) => Err("cancelled".to_owned()),
                }
            })
        },
    );
    (loader, queue)
}

fn make_page(
    path: &str,
    parent: Option<&str>,
    names: &[(&str, bool)],
    next_cursor: Option<&str>,
) -> Page {
    Page {
        location: Location {
            path: path.to_owned(),
            label: path.rsplit('/').next().unwrap_or(path).to_owned(),
        },
        parent: parent.map(str::to_owned),
        ancestors: parent
            .map(|p| {
                vec![Location {
                    path: p.to_owned(),
                    label: "root".to_owned(),
                }]
            })
            .unwrap_or_default(),
        entries: names
            .iter()
            .map(|(name, hidden)| Entry {
                location: Location {
                    path: format!("{path}/{name}"),
                    label: (*name).to_owned(),
                },
                hidden: *hidden,
            })
            .collect(),
        next_cursor: next_cursor.map(str::to_owned),
    }
}

#[test]
fn navigation_history_label_filter_and_acceptance_guards_work_end_to_end() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let (loader, pending) = controlled_loader();
        let picker = DirectoryPicker::with_path(loader, "/remote");
        flush();

        assert!(picker.is_loading().get_untracked());
        assert_eq!(
            picker
                .pending_request()
                .get_untracked()
                .and_then(|r| r.path)
                .as_deref(),
            Some("/remote")
        );
        // Pending load must never be accepted.
        assert_eq!(picker.selected_path(), None);

        let (_, mut reply) = pending.borrow_mut().remove(0);
        reply
            .send
            .take()
            .expect("sender")
            .send(Ok(make_page(
                "/remote",
                Some("/"),
                &[
                    ("Documents", false),
                    ("Downloads", false),
                    (".config", true),
                ],
                None,
            )))
            .ok();
        flush();

        assert!(!picker.is_loading().get_untracked());
        assert_eq!(picker.current_path().as_deref(), Some("/remote"));
        assert_eq!(picker.selected_path().as_deref(), Some("/remote"));
        assert_eq!(picker.rows().get_untracked().len(), 2);

        // Filtering matches strictly against `location.label`, never parent segments in `location.path`.
        picker.set_filter("remote");
        assert_eq!(
            picker.rows().get_untracked().len(),
            0,
            "parent path segment must not match child labels"
        );

        picker.toggle_hidden();
        picker.set_filter("conf");
        assert_eq!(picker.rows().get_untracked().len(), 1);
        picker.select_row(0);
        assert_eq!(picker.selected_path().as_deref(), Some("/remote/.config"));

        // Unsubmitted location editing disables acceptance until applied or cancelled.
        picker.begin_edit_location();
        assert_eq!(picker.selected_path(), None);
        picker.cancel_edit_location();
        assert_eq!(picker.selected_path().as_deref(), Some("/remote/.config"));

        // Navigating to a failing path disables acceptance; refresh retries the failed target.
        picker.navigate("/remote/Documents");
        flush();
        assert_eq!(picker.selected_path(), None);
        let (_, mut fail_reply) = pending.borrow_mut().remove(0);
        fail_reply
            .send
            .take()
            .expect("sender")
            .send(Err("permission denied".to_owned()))
            .ok();
        flush();
        assert_eq!(picker.selected_path(), None);
        assert_eq!(
            picker.error().get_untracked().as_deref(),
            Some("permission denied")
        );

        picker.refresh();
        flush();
        let (retry_req, mut ok_reply) = pending.borrow_mut().remove(0);
        assert_eq!(retry_req.path.as_deref(), Some("/remote/Documents"));
        ok_reply
            .send
            .take()
            .expect("sender")
            .send(Ok(make_page(
                "/remote/Documents",
                Some("/remote"),
                &[],
                None,
            )))
            .ok();
        flush();

        assert!(picker.can_go_back().get_untracked());
        assert!(!picker.can_go_forward().get_untracked());
        assert_eq!(picker.selected_path().as_deref(), Some("/remote/Documents"));

        picker.back();
        flush();
        let (req, mut reply_back) = pending.borrow_mut().remove(0);
        assert_eq!(req.path.as_deref(), Some("/remote"));
        reply_back
            .send
            .take()
            .expect("sender")
            .send(Ok(make_page(
                "/remote",
                Some("/"),
                &[("Documents", false)],
                None,
            )))
            .ok();
        flush();

        assert!(!picker.can_go_back().get_untracked());
        assert!(picker.can_go_forward().get_untracked());
    });
    scope.unmount();
}

#[test]
fn stale_async_load_is_cancelled_when_user_navigates_away() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let (loader, pending) = controlled_loader();
        let picker = DirectoryPicker::with_path(loader, "/slow");
        flush();

        let (_, mut slow_reply) = pending.borrow_mut().remove(0);
        picker.navigate("/fast");
        flush();

        assert!(slow_reply.send.take().expect("sender").is_closed());
        let (req, mut fast_reply) = pending.borrow_mut().remove(0);
        assert_eq!(req.path.as_deref(), Some("/fast"));
        fast_reply
            .send
            .take()
            .expect("sender")
            .send(Ok(make_page("/fast", Some("/"), &[("Work", false)], None)))
            .ok();
        flush();

        assert_eq!(picker.current_path().as_deref(), Some("/fast"));
    });
    scope.unmount();
}

#[test]
fn pagination_prefetches_while_below_min_scrollable_rows_and_stops_at_threshold() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let calls = Rc::new(Cell::new(0));
        let calls_ref = Rc::clone(&calls);
        let loader: super::Loader = Rc::new(move |req: Request| {
            let count = calls_ref.get() + 1;
            calls_ref.set(count);
            Box::pin(async move {
                match req.cursor.as_deref() {
                    None => {
                        let initial: Vec<(String, bool)> =
                            (0..8).map(|i| (format!("a-{i:02}"), false)).collect();
                        let refs: Vec<(&str, bool)> =
                            initial.iter().map(|(s, h)| (s.as_str(), *h)).collect();
                        Ok(make_page("/root", None, &refs, Some("c2")))
                    }
                    Some("c2") => {
                        let extra: Vec<(String, bool)> =
                            (0..15).map(|i| (format!("b-{i:02}"), false)).collect();
                        let refs: Vec<(&str, bool)> =
                            extra.iter().map(|(s, h)| (s.as_str(), *h)).collect();
                        Ok(make_page("/root", None, &refs, Some("c3")))
                    }
                    _ => Ok(make_page("/root", None, &[("c-00", false)], None)),
                }
            })
        });

        let picker = DirectoryPicker::with_path(loader, "/root");
        for _ in 0..8 {
            flush();
        }

        assert_eq!(picker.rows().get_untracked().len(), 23);
        assert!(picker.rows().get_untracked().len() >= MIN_SCROLLABLE_ROWS);
        assert_eq!(
            calls.get(),
            2,
            "prefetch must stop once >= MIN_SCROLLABLE_ROWS (20) rows are loaded without draining page 3"
        );
    });
    scope.unmount();
}

#[test]
fn directory_picker_dialog_handles_alt_a_confirm_and_alt_c_cancel() {
    let selected_out: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let cancelled = Rc::new(Cell::new(0usize));
    let open_slot: Rc<RefCell<Option<RwSignal<bool, zgui::reactive::LocalStorage>>>> =
        Rc::new(RefCell::new(None));
    let picker_slot: Rc<RefCell<Option<DirectoryPicker>>> = Rc::new(RefCell::new(None));

    let sel_for_view = Rc::clone(&selected_out);
    let cancel_for_view = Rc::clone(&cancelled);
    let open_for_view = Rc::clone(&open_slot);
    let picker_for_view = Rc::clone(&picker_slot);

    let handler = app()
        .with_title("picker-dialog-test")
        .with_application_id("dev.gds.PickerDialogTest")
        .with_size(800.0, 640.0)
        .with_stylesheet(":root { display: flex; width: 100%; height: 100%; }")
        .with_renderer(Box::new(
            move |_surface: &std::sync::Arc<dyn Surface>, target: zgui::render::RenderTarget| {
                let mut renderer = NullRenderer::default();
                zgui::render::Renderer::configure(&mut renderer, target);
                Ok::<_, zgui::Error>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
            },
        ))
        .into_handler(move || {
            let loader: super::Loader = Rc::new(|req: Request| {
                let path = req.path.unwrap_or_else(|| "/srv".to_owned());
                Box::pin(async move {
                    Ok(make_page(
                        &path,
                        Some("/"),
                        &[("projects", false), (".secret", true)],
                        None,
                    ))
                })
            });
            let picker = DirectoryPicker::with_path(loader, "/srv");
            let open = RwSignal::new_local(true);
            *open_for_view.borrow_mut() = Some(open);
            *picker_for_view.borrow_mut() = Some(picker.clone());

            let sel = Rc::clone(&sel_for_view);
            let can = Rc::clone(&cancel_for_view);
            view! {
                DirectoryPickerDialog(
                    open = open,
                    picker = picker,
                    on_select = UnsyncCallback::new(move |loc: Location| {
                        sel.borrow_mut().push(loc.path);
                    }),
                    on_cancel = UnsyncCallback::new(move |()| {
                        can.set(can.get() + 1);
                    }),
                )
            }
        })
        .expect("build headless directory picker dialog test");

    handler
        .drive(|app| -> Result<(), PlatformError> {
            let mut harness: Harness<Box<dyn AppHandler>> = Harness::new(app);
            for _ in 0..6 {
                harness.pump();
            }
            harness.deliver_to_first(SurfaceEvent::Focused(true));
            harness.pump();

            let open = open_slot.borrow().expect("open signal");
            let picker = picker_slot.borrow().clone().expect("picker instance");
            picker.select_row(0);
            harness.pump();

            // Alt+A confirms `/srv/projects`.
            let alt_a = KeyEvent::character("a");
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: alt_a.clone(),
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::from_origin(Duration::from_millis(10)),
            });
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Released,
                event: alt_a,
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::from_origin(Duration::from_millis(15)),
            });
            for _ in 0..4 {
                harness.pump();
            }
            assert!(!open.get_untracked());
            assert_eq!(*selected_out.borrow(), vec!["/srv/projects".to_owned()]);

            // Re-open and cancel via Alt+C.
            open.set(true);
            for _ in 0..4 {
                harness.pump();
            }
            let alt_c = KeyEvent::character("c");
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Pressed,
                event: alt_c.clone(),
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::from_origin(Duration::from_millis(30)),
            });
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Released,
                event: alt_c,
                modifiers: Modifiers::ALT,
                timestamp: Timestamp::from_origin(Duration::from_millis(35)),
            });
            for _ in 0..4 {
                harness.pump();
            }
            assert!(!open.get_untracked());
            assert_eq!(cancelled.get(), 1);

            harness.shut_down();
            Ok(())
        })
        .expect("drive headless directory picker dialog test");
}
