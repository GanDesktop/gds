use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use zgui::app::app;
use zgui::geom::{CssPx, Point};
use zgui::platform::{Surface, SurfaceEvent, TextInputPurpose};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Signal, UnsyncCallback};
use zgui::runtime::Runtime;
use zgui::view;
use zgui::view::focused_node;
use zgui::vocab::{
    ImeEvent, Key, KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction,
    PointerButton, PointerEvent, Timestamp,
};
use zgui_platform_headless::Harness;

use super::{BLINK_INTERVAL, TERMINAL_CSS, TerminalViewProps, last_test_canvas_host_ref};
use crate::app::Gds;
use crate::service::task_manager::Supervisor;
use crate::service::terminal::{Session, SpawnOptions};
use crate::ui::drag::DragOverlayState;
use crate::ui::terminal::{Preferences, Terminal};

struct NullRenderer {
    target: Option<zgui::render::RenderTarget>,
    atlas: zgui::atlas::MemorySink,
    quads: Rc<RefCell<Vec<[f32; 4]>>>,
}

impl NullRenderer {
    fn new(quads: Rc<RefCell<Vec<[f32; 4]>>>) -> Self {
        Self {
            target: None,
            atlas: zgui::atlas::MemorySink::default(),
            quads,
        }
    }
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
        scene: &zgui::scene::Scene,
        _damage: &zgui::bits::DamageSet,
    ) -> zgui::render::FrameOutcome {
        self.quads
            .borrow_mut()
            .extend(scene.primitives.quads.iter().map(|quad| quad.bounds));
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

struct TestHostState {
    terminal: Terminal,
    outer_ref: NodeRef,
    next_ref: NodeRef,
    terminal_ref: NodeRef,
    focused: Signal<Option<NodeId>, LocalStorage>,
    drag: DragOverlayState<Vec<PathBuf>>,
    workspace_actions: Rc<Cell<usize>>,
    settings: Rc<Cell<usize>>,
    toggles: Rc<Cell<usize>>,
    exits: Rc<Cell<usize>>,
    last_quads: Rc<RefCell<Vec<[f32; 4]>>>,
    gds: Gds,
}

fn run_headless_terminal(
    options: SpawnOptions,
    preferences: Preferences,
    run_assertions: impl FnOnce(&mut Harness<Runtime>, &TestHostState),
) {
    run_headless_terminal_inner(options, preferences, true, false, run_assertions);
}

fn run_headless_terminal_with(
    options: SpawnOptions,
    preferences: Preferences,
    paint_surface: bool,
    run_assertions: impl FnOnce(&mut Harness<Runtime>, &TestHostState),
) {
    run_headless_terminal_inner(options, preferences, paint_surface, false, run_assertions);
}

fn run_headless_terminal_inner(
    options: SpawnOptions,
    preferences: Preferences,
    paint_surface: bool,
    focus_before_mount: bool,
    run_assertions: impl FnOnce(&mut Harness<Runtime>, &TestHostState),
) {
    let supervisor = Supervisor::new(false).expect("test Tokio supervisor");
    let scope = supervisor.scope();
    let runtime = scope.runtime_handle().expect("supervisor runtime").clone();
    let state_slot: Rc<RefCell<Option<TestHostState>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&state_slot);
    let options_cell = RefCell::new(Some((options, preferences)));
    let last_quads: Rc<RefCell<Vec<[f32; 4]>>> = Rc::new(RefCell::new(Vec::new()));
    let quads_for_renderer = Rc::clone(&last_quads);
    let quads_for_state = Rc::clone(&last_quads);

    let handler = app()
        .with_title("terminal-test")
        .with_application_id("dev.gds.TerminalTest")
        .with_size(800.0, 480.0)
        .with_stylesheet(":root { display: flex; width: 100%; height: 100%; }")
        .with_renderer(Box::new(
            move |_surface: &std::sync::Arc<dyn Surface>, target: zgui::render::RenderTarget| {
                let mut renderer = NullRenderer::new(Rc::clone(&quads_for_renderer));
                zgui::render::Renderer::configure(&mut renderer, target);
                Ok::<_, zgui::Error>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
            },
        ))
        .with_context(move || {
            let gds = Gds::install(scope.clone());
            gds.appearance()
                .theme()
                .apply(&crate::appearance::theme::Appearance {
                    source: crate::appearance::theme::Source::Builtin,
                    ..crate::appearance::theme::Appearance::default()
                })
                .expect("default test theme");
        })
        .into_handler(move || -> AnyView {
            crate::ui::styles::install_ui_stylesheets();
            let gds = Gds::current();
            let (opts, prefs) = options_cell
                .borrow_mut()
                .take()
                .expect("view factory called once");
            let terminal =
                Terminal::spawn(opts, prefs, &gds.tasks().scope()).expect("spawn test terminal");
            if focus_before_mount {
                terminal.focus();
            }
            let outer_ref = NodeRef::new();
            let next_ref = NodeRef::new();
            let terminal_ref = NodeRef::new();
            let focused = focused_node();
            let drag = DragOverlayState::<Vec<PathBuf>>::new();
            let workspace_actions = Rc::new(Cell::new(0));
            let settings = Rc::new(Cell::new(0));
            let toggles = Rc::new(Cell::new(0));
            let exits = Rc::new(Cell::new(0));

            *slot_for_view.borrow_mut() = Some(TestHostState {
                terminal: terminal.clone(),
                outer_ref,
                next_ref,
                terminal_ref,
                focused,
                drag,
                workspace_actions: Rc::clone(&workspace_actions),
                settings: Rc::clone(&settings),
                toggles: Rc::clone(&toggles),
                exits: Rc::clone(&exits),
                last_quads: Rc::clone(&quads_for_state),
                gds,
            });

            let on_host_key = {
                let workspace_actions = Rc::clone(&workspace_actions);
                let settings = Rc::clone(&settings);
                let toggles = Rc::clone(&toggles);
                move |ev: &mut EventCx<'_, zgui::view::events::KeyDown>| {
                    let inside_terminal = terminal_ref
                        .get_untracked()
                        .is_some_and(|id| focused.get_untracked() == Some(id));
                    if !inside_terminal
                        && ev.modifiers == Modifiers::CONTROL
                        && matches!(&ev.key_without_modifiers, Key::Character(c) if c.eq_ignore_ascii_case("w"))
                    {
                        workspace_actions.set(workspace_actions.get() + 1);
                        ev.stop_propagation();
                        ev.prevent_default();
                    } else if ev.modifiers == Modifiers::CONTROL
                        && matches!(&ev.key_without_modifiers, Key::Character(ch) if ch.as_str() == "," || ch.as_str() == "`")
                    {
                        if let Key::Character(ch) = &ev.key_without_modifiers
                            && ch.as_str() == ","
                        {
                            settings.set(settings.get() + 1);
                        } else {
                            toggles.set(toggles.get() + 1);
                        }
                        ev.stop_propagation();
                        ev.prevent_default();
                    }
                }
            };

            let on_terminal_key = UnsyncCallback::new({
                let settings = Rc::clone(&settings);
                let toggles = Rc::clone(&toggles);
                let exits = Rc::clone(&exits);
                move |(key, modifiers): (Key, Modifiers)| {
                    if modifiers == Modifiers::CONTROL
                        && matches!(&key, Key::Character(c) if c == ",")
                    {
                        settings.set(settings.get() + 1);
                        true
                    } else if modifiers == Modifiers::CONTROL
                        && matches!(&key, Key::Character(c) if c == "`")
                    {
                        toggles.set(toggles.get() + 1);
                        true
                    } else if modifiers == Modifiers::NONE && key == Key::Named(NamedKey::F6) {
                        exits.set(exits.get() + 1);
                        true
                    } else {
                        false
                    }
                }
            });

            let term_for_drop = terminal.clone();
            AnyView::new(view! {
                box(
                    style = "display: flex; flex-direction: column; width: 100%; height: 100%; min-height: 0;",
                    on:key_down = on_host_key,
                ) {
                    box(
                        node_ref = outer_ref,
                        tabindex = {Focus::Sequential},
                        style = "width: 20px; height: 10px;",
                    )
                    box(
                        node_ref = next_ref,
                        tabindex = {Focus::Sequential},
                        style = "width: 20px; height: 10px;",
                    )
                    box(
                        style = "flex: 1 1 0; width: 100%; min-height: 0; display: flex;",
                        on:pointer_up = move |_| {
                            if let Some(paths) = drag.end() {
                                term_for_drop.paste_paths(&paths);
                                term_for_drop.focus();
                            }
                        },
                    ) {
                        TerminalView(
                            terminal = terminal,
                            paint_surface = paint_surface,
                            on_key = on_terminal_key,
                            drag_active = drag.is_active(),
                            node_ref = terminal_ref,
                        )
                    }
                }
            })
        })
        .expect("build headless terminal test app");

    let runtime_app = handler.into_runtime();
    let failure = runtime_app.failure();
    let mut harness = Harness::new(runtime_app);
    pump_frames(&mut harness);
    harness.deliver_to_first(SurfaceEvent::Focused(true));
    pump_frames(&mut harness);
    let borrowed = state_slot.borrow();
    let host = borrowed.as_ref().expect("host test state initialized");
    run_assertions(&mut harness, host);
    runtime.block_on(host.terminal.shutdown());
    drop(borrowed);
    harness.shut_down();
    zgui::reactive::shutdown();
    if let Some(err) = failure.take() {
        panic!("headless terminal runtime error: {err}");
    }
}

fn is_node_focused(host: &TestHostState, node: NodeRef) -> bool {
    node.get_untracked()
        .is_some_and(|id| host.focused.get_untracked() == Some(id))
}

fn pump_frames(harness: &mut Harness<Runtime>) {
    for _ in 0..4 {
        harness.pump();
    }
}

fn press_key(harness: &mut Harness<Runtime>, event: KeyEvent, modifiers: Modifiers, stamp_ms: u64) {
    let timestamp = Timestamp::from_origin(Duration::from_millis(stamp_ms));
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Pressed,
        event: event.clone(),
        modifiers,
        timestamp,
    });
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Released,
        event,
        modifiers,
        timestamp: Timestamp::from_origin(Duration::from_millis(stamp_ms.saturating_add(5))),
    });
    pump_frames(harness);
}

fn type_text(harness: &mut Harness<Runtime>, text: &str, start_ms: u64) {
    for (idx, ch) in text.chars().enumerate() {
        let mut buf = [0_u8; 4];
        let s: &str = ch.encode_utf8(&mut buf);
        press_key(
            harness,
            KeyEvent::character(s),
            Modifiers::NONE,
            start_ms + idx as u64 * 10,
        );
    }
}

fn wait_for_pty(harness: &mut Harness<Runtime>, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready() {
        assert!(Instant::now() < deadline, "PTY response timed out");
        harness.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn screen(session: &Session) -> String {
    use alacritty_terminal::{
        grid::Dimensions,
        index::{Column, Line},
        term::cell::Flags,
    };
    session.with_term(|term| {
        let grid = term.grid();
        (0..term.screen_lines())
            .flat_map(|row| {
                (0..term.columns()).filter_map(move |column| {
                    let cell = &grid[Line(row as i32)][Column(column)];
                    (!cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER))
                    .then_some(cell.c)
                })
            })
            .collect()
    })
}

#[test]
fn single_terminal_respects_its_hosts_key_map() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec!["-c".into(), "exit 0".into()],
        ..SpawnOptions::default()
    };
    run_headless_terminal(options, Preferences::default(), |harness, host| {
        host.outer_ref.focus();
        pump_frames(harness);
        press_key(harness, KeyEvent::character("w"), Modifiers::CONTROL, 10);
        assert_eq!(host.workspace_actions.get(), 1);

        host.terminal.focus();
        pump_frames(harness);
        assert!(is_node_focused(host, host.terminal_ref));
        press_key(harness, KeyEvent::character("w"), Modifiers::CONTROL, 20);
        press_key(harness, KeyEvent::character(","), Modifiers::CONTROL, 30);
        press_key(harness, KeyEvent::character("`"), Modifiers::CONTROL, 40);
        press_key(
            harness,
            KeyEvent::named(NamedKey::F6, PhysicalKey::Code(KeyCode::F6)),
            Modifiers::NONE,
            50,
        );
        assert_eq!(
            host.workspace_actions.get(),
            1,
            "host close must not steal shell Ctrl+W"
        );
        assert_eq!(
            (host.settings.get(), host.toggles.get(), host.exits.get()),
            (1, 1, 1)
        );
    });
}

#[test]
fn tab_routes_to_pty_and_outside_terminal_traversal_still_works() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec![
            "-c".into(),
            "stty -echo -icanon min 1 time 0; printf '\\033]0;ready\\007'; dd bs=1 count=4 2>/dev/null | od -An -tx1".into(),
        ],
        ..SpawnOptions::default()
    };
    run_headless_terminal(options, Preferences::default(), |harness, host| {
        wait_for_pty(harness, || {
            host.terminal.session().and_then(|s| s.title()).as_deref() == Some("ready")
        });

        host.outer_ref.focus();
        pump_frames(harness);
        let tab = KeyEvent::named(NamedKey::Tab, PhysicalKey::Code(KeyCode::Tab));
        press_key(harness, tab.clone(), Modifiers::NONE, 10);
        assert!(
            is_node_focused(host, host.next_ref),
            "root Tab must still traverse outside the terminal"
        );
        press_key(harness, tab.clone(), Modifiers::SHIFT, 20);
        assert!(
            is_node_focused(host, host.outer_ref),
            "root reverse traversal must still work"
        );

        host.terminal.focus();
        pump_frames(harness);
        assert!(is_node_focused(host, host.terminal_ref));
        press_key(harness, tab.clone(), Modifiers::NONE, 30);
        assert!(
            is_node_focused(host, host.terminal_ref),
            "Tab must not leave terminal input"
        );
        press_key(harness, tab, Modifiers::SHIFT, 40);
        assert!(
            is_node_focused(host, host.terminal_ref),
            "Shift+Tab must not leave terminal input"
        );

        let session = host.terminal.session().expect("session running");
        wait_for_pty(harness, || screen(&session).contains("09 1b 5b 5a"));
    });
}

#[test]
fn tab_completes_a_filename_in_a_real_interactive_shell() {
    let account_shell = nix::unistd::User::from_uid(nix::unistd::Uid::current())
        .expect("test account lookup")
        .map(|user| user.shell);
    let Some(bash) = account_shell
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.join("bash")),
        )
        .find(|path| {
            path.file_name().is_some_and(|name| name == "bash")
                && std::process::Command::new(path)
                    .args(["--noprofile", "--norc", "-c", "type bind >/dev/null 2>&1"])
                    .status()
                    .is_ok_and(|status| status.success())
        })
    else {
        return;
    };

    let directory = tempfile::tempdir().expect("completion directory");
    std::fs::write(
        directory.path().join("gds-completion-token"),
        "COMPLETION_SUCCEEDED\n",
    )
    .expect("completion fixture");

    let options = SpawnOptions {
        cwd: directory.path().to_owned(),
        shell: Some(bash),
        args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
        ..SpawnOptions::default()
    };

    run_headless_terminal(options, Preferences::default(), |harness, host| {
        wait_for_pty(harness, || host.terminal.session().is_some());
        let session = host.terminal.session().expect("interactive bash session");
        session
            .write(b"PS1=''; stty -echo; bind '\"\\C-i\": complete'; printf '\\033]0;ready\\007'\r")
            .expect("initialize isolated readline session");
        wait_for_pty(harness, || session.title().as_deref() == Some("ready"));

        host.terminal.focus();
        pump_frames(harness);
        type_text(harness, "cat gds-completion-t", 20);
        press_key(
            harness,
            KeyEvent::named(NamedKey::Tab, PhysicalKey::Code(KeyCode::Tab)),
            Modifiers::NONE,
            300,
        );
        assert!(is_node_focused(host, host.terminal_ref));
        press_key(
            harness,
            KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
            Modifiers::NONE,
            320,
        );
        wait_for_pty(harness, || {
            screen(&session).contains("COMPLETION_SUCCEEDED")
        });
    });
}

#[test]
fn changing_terminal_typography_updates_existing_grid_metrics() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec!["-c".into(), "exit 0".into()],
        ..SpawnOptions::default()
    };
    run_headless_terminal(options, Preferences::default(), |harness, host| {
        pump_frames(harness);
        let before = host.terminal.test_geometry();
        host.terminal.apply_preferences(Preferences {
            font_family: ".SystemMonospace".into(),
            font_size: 24.0,
            ..Preferences::default()
        });
        pump_frames(harness);
        let after = host.terminal.test_geometry();
        assert!(after.cell_width > before.cell_width);
        assert!(after.cell_height > before.cell_height);
        assert!(
            after.columns < before.columns,
            "larger text must reduce grid columns"
        );
        assert!(
            after.rows < before.rows,
            "larger text must reduce grid rows"
        );
    });
}

#[test]
fn cursor_blink_only_runs_while_focused_and_allowed() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec!["-c".into(), "sleep 60".into()],
        ..SpawnOptions::default()
    };
    run_headless_terminal(options, Preferences::default(), |harness, host| {
        assert_eq!(host.terminal.test_cursor_state(), (false, false, true));

        host.terminal.focus();
        pump_frames(harness);
        assert_eq!(host.terminal.test_cursor_state(), (true, true, true));

        harness.advance(BLINK_INTERVAL);
        pump_frames(harness);
        assert_eq!(host.terminal.test_cursor_state(), (true, true, false));

        host.terminal.apply_preferences(Preferences {
            cursor_blink: false,
            ..Preferences::default()
        });
        pump_frames(harness);
        assert_eq!(host.terminal.test_cursor_state(), (true, false, true));

        host.terminal.apply_preferences(Preferences::default());
        pump_frames(harness);
        assert_eq!(host.terminal.test_cursor_state(), (true, true, true));

        harness.advance(BLINK_INTERVAL);
        pump_frames(harness);
        assert!(!host.terminal.test_cursor_state().2);

        host.gds.appearance().motion().set_enabled(false);
        pump_frames(harness);
        assert_eq!(host.terminal.test_cursor_state(), (true, false, true));

        host.gds.appearance().motion().set_enabled(true);
        pump_frames(harness);
        assert_eq!(host.terminal.test_cursor_state(), (true, true, true));

        host.next_ref.focus();
        pump_frames(harness);
        assert_eq!(host.terminal.test_cursor_state(), (false, false, true));
    });
}

#[test]
fn terminal_grid_renders_tabs_and_combining_marks() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec![
            "-c".into(),
            "printf '%s\\r\\n' 'A\te\u{0301}DONE'; sleep 60".into(),
        ],
        ..SpawnOptions::default()
    };
    run_headless_terminal(options, Preferences::default(), |harness, host| {
        wait_for_pty(harness, || {
            host.terminal
                .session()
                .is_some_and(|session| screen(&session).contains("DONE"))
        });
        host.terminal.focus();
        pump_frames(harness);
        assert!(host.terminal.test_geometry().columns > 0);
    });
}

#[test]
fn internal_file_drag_pointer_up_bubbles_past_terminal_to_paste_escaped_paths() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec![
            "-c".into(),
            "printf '\\033]0;ready\\007'; IFS= read -r line; printf 'DROPPED:%s\\n' \"$line\"; sleep 60".into(),
        ],
        ..SpawnOptions::default()
    };
    run_headless_terminal(options, Preferences::default(), |harness, host| {
        wait_for_pty(harness, || {
            host.terminal.session().and_then(|s| s.title()).as_deref() == Some("ready")
        });

        host.drag.begin(
            vec![
                PathBuf::from("/tmp/alpha file.txt"),
                PathBuf::from("/tmp/beta'quote.rs"),
            ],
            10.0,
            5.0,
        );
        let drop_pos = Point::new(CssPx(200.0), CssPx(200.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(drop_pos),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(10)),
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(drop_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(20)),
        });
        pump_frames(harness);

        assert!(
            !host.drag.is_active().get_untracked(),
            "PointerUp over TerminalView must bubble to host drop target during active drag"
        );
        assert!(is_node_focused(host, host.terminal_ref));

        press_key(
            harness,
            KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
            Modifiers::NONE,
            40,
        );
        let session = host.terminal.session().expect("session running");
        wait_for_pty(harness, || {
            let text = screen(&session);
            text.contains("DROPPED:'/tmp/alpha file.txt'") && text.contains("beta")
        });
    });
}

#[derive(Debug)]
struct TerminalDomInspection {
    root_tag: String,
    ime_attr: Option<String>,
    ime_x: Option<f32>,
    ime_y: Option<f32>,
    ime_h: Option<f32>,
    paint_surface: Option<String>,
    text_descendants: usize,
    editor_elements: usize,
}

fn inspect_terminal_dom(harness: &Harness<Runtime>, root_id: NodeId) -> TerminalDomInspection {
    let window = &harness.app().windows()[0];
    let root_idx = window.dom().index_of(root_id);
    let document = window.document().borrow();
    let store = document.store();
    let root_node = document.node(root_idx);

    let parse_attr_f32 = |name: &str| -> Option<f32> {
        root_node
            .attr(name)
            .and_then(|val| val.as_str().parse::<f32>().ok())
    };

    let mut text_descendants = 0;
    let mut subtree = vec![root_idx];
    while let Some(idx) = subtree.pop() {
        let core = store.core(idx);
        if format!("{:?}", core.kind()) == "Text" {
            text_descendants += 1;
        }
        let mut child = core.first_child();
        while let Some(child_idx) = child {
            subtree.push(child_idx);
            child = store.core(child_idx).next_sibling();
        }
    }

    let mut editor_elements = 0;
    let mut all_nodes = vec![document.document_index()];
    while let Some(idx) = all_nodes.pop() {
        let core = store.core(idx);
        if format!("{:?}", core.kind()) == "Element" && core.local_name().as_str() == "editor" {
            editor_elements += 1;
        }
        let mut child = core.first_child();
        while let Some(child_idx) = child {
            all_nodes.push(child_idx);
            child = store.core(child_idx).next_sibling();
        }
    }

    TerminalDomInspection {
        root_tag: root_node.tag_name().as_str().to_owned(),
        ime_attr: root_node
            .attr("data-zgui-ime")
            .map(|s| s.as_str().to_owned()),
        ime_x: parse_attr_f32("data-zgui-ime-x"),
        ime_y: parse_attr_f32("data-zgui-ime-y"),
        ime_h: parse_attr_f32("data-zgui-ime-h"),
        paint_surface: root_node
            .attr("data-paint-surface")
            .map(|s| s.as_str().to_owned()),
        text_descendants,
        editor_elements,
    }
}

fn css_rule_body<'a>(sheet: &'a str, selector: &str) -> Option<&'a str> {
    let start = sheet.find(selector)?;
    let after_selector = &sheet[start + selector.len()..];
    let brace_open = after_selector.find('{')?;
    let rest = &after_selector[brace_open + 1..];
    let brace_close = rest.find('}')?;
    Some(&rest[..brace_close])
}

#[test]
fn terminal_view_uses_custom_ime_box_without_projected_text_children_or_height_loss() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec![
            "-c".into(),
            "stty -echo; printf '\\033]0;ready\\007'; while IFS= read -r line; do printf 'ECHO:%s\\r\\n' \"$line\"; done".into(),
        ],
        ..SpawnOptions::default()
    };
    run_headless_terminal(options, Preferences::default(), |harness, host| {
        wait_for_pty(harness, || {
            host.terminal.title().get_untracked().as_deref() == Some("ready")
        });
        pump_frames(harness);

        let root_id = host
            .terminal_ref
            .get_untracked()
            .expect("terminal root bound");
        let canvas_host = last_test_canvas_host_ref().expect("canvas host bound");

        let initial_dom = inspect_terminal_dom(harness, root_id);
        assert_eq!(initial_dom.root_tag, "box");
        assert_eq!(
            initial_dom.editor_elements, 0,
            "TerminalView must not mount <editor>"
        );
        assert_eq!(initial_dom.ime_attr.as_deref(), Some("true"));
        assert!(initial_dom.ime_x.expect("data-zgui-ime-x") >= 0.0);
        assert!(initial_dom.ime_h.expect("data-zgui-ime-h") >= 1.0);
        assert_eq!(initial_dom.paint_surface.as_deref(), Some("true"));
        assert_eq!(initial_dom.text_descendants, 0);

        let root_bounds_before = host.terminal_ref.bounds().expect("terminal root bounds");
        let canvas_bounds_before = canvas_host.bounds().expect("canvas host bounds");
        assert!(root_bounds_before.size.height.0 > 400.0);
        assert_eq!(
            canvas_bounds_before.size, root_bounds_before.size,
            ".gds-terminal-canvas-host must occupy 100% of .gds-terminal-root when running"
        );

        // 1. Click inside TerminalView (`PointerDown` + `PointerUp`) — must focus, enable IME,
        // and append zero projected `<text>` children.
        let click_pos = Point::new(CssPx(120.0), CssPx(120.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(click_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(10)),
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(click_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(15)),
        });
        pump_frames(harness);
        assert!(is_node_focused(host, host.terminal_ref));

        let initial_ime = harness
            .platform()
            .offscreens()
            .first()
            .expect("offscreen surface")
            .last_text_input()
            .expect("surface notified of text input")
            .expect("custom IME enabled on terminal focus");
        assert_eq!(initial_ime.purpose, TextInputPurpose::Normal);
        let geom = host.terminal.test_geometry();
        let expected_cell_h = geom.cell_height / geom.scale.max(1.0);
        assert!(
            (initial_ime.caret_size.height.0 - expected_cell_h).abs() <= 0.5,
            "IME caret height ({}) must match terminal cell height ({expected_cell_h})",
            initial_ime.caret_size.height.0
        );

        // 2. Printable `KeyDown` + `Enter`.
        type_text(harness, "alpha", 30);
        press_key(
            harness,
            KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
            Modifiers::NONE,
            100,
        );
        let session = host.terminal.session().expect("session running");
        wait_for_pty(harness, || screen(&session).contains("ECHO:alpha"));
        pump_frames(harness);

        // 3. Full IME composition (`ImeStart` -> `ImePreedit` -> `ImeCommit` -> `ImeEnd` + `Enter`).
        harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Enabled));
        harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Preedit {
            text: "にほん".into(),
            cursor: Some(9..9),
        }));
        pump_frames(harness);
        harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Commit("日本".into())));
        harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Disabled));
        pump_frames(harness);
        press_key(
            harness,
            KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
            Modifiers::NONE,
            200,
        );
        wait_for_pty(harness, || screen(&session).contains("ECHO:日本"));
        host.terminal.request_redraw();
        pump_frames(harness);

        let after_dom = inspect_terminal_dom(harness, root_id);
        assert_eq!(
            after_dom.text_descendants, 0,
            "Editors must never append DOM <text> nodes inside .gds-terminal-root"
        );
        assert!(host.terminal_ref.text_content().is_empty());
        assert!(
            after_dom.ime_y.expect("data-zgui-ime-y") > initial_dom.ime_y.expect("initial ime-y"),
            "cursor cell y attribute must advance downward as terminal output advances rows"
        );

        let after_ime = harness
            .platform()
            .offscreens()
            .first()
            .expect("offscreen surface")
            .last_text_input()
            .expect("surface notified")
            .expect("IME remains enabled");
        assert!(
            after_ime.caret_origin.y.0 > initial_ime.caret_origin.y.0,
            "surface TextInput caret_origin.y must track terminal cursor row"
        );

        let root_bounds_after = host.terminal_ref.bounds().expect("terminal root bounds");
        let canvas_bounds_after = canvas_host.bounds().expect("canvas host bounds");
        assert_eq!(
            canvas_bounds_after.size, root_bounds_after.size,
            ".gds-terminal-canvas-host must retain 100% of .gds-terminal-root height after pointer, keyboard, and IME input"
        );
    });
}

#[test]
fn paint_surface_false_skips_canvas_fill_until_osc11_while_preserving_cell_cursor_and_selection_fills()
 {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec![
            "-c".into(),
            "stty -echo; printf '\\033[41mRED\\033[0m PLAIN\\r\\n\\033]0;ready\\007'; while IFS= read -r cmd; do case \"$cmd\" in osc) printf '\\033]11;#112233\\007\\033]0;osc11\\007' ;; rst) printf '\\033]111\\007\\033]0;reset\\007' ;; esac; done".into(),
        ],
        ..SpawnOptions::default()
    };

    run_headless_terminal_with(options, Preferences::default(), false, |harness, host| {
        wait_for_pty(harness, || {
            host.terminal.title().get_untracked().as_deref() == Some("ready")
        });
        host.last_quads.borrow_mut().clear();
        host.terminal.focus();
        host.terminal.request_redraw();
        pump_frames(harness);

        let root_id = host
            .terminal_ref
            .get_untracked()
            .expect("terminal root bound");
        let dom_info = inspect_terminal_dom(harness, root_id);
        assert_eq!(
            dom_info.paint_surface, None,
            "paint_surface = false must omit data-paint-surface=\"true\""
        );

        let root_bounds = host.terminal_ref.bounds().expect("terminal root bounds");
        let geom = host.terminal.test_geometry();
        let has_full_canvas_quad = |quads: &[[f32; 4]]| {
            quads.iter().any(|&[_x, _y, w, h]| {
                (w - root_bounds.size.width.0).abs() <= 1.0
                    && (h - root_bounds.size.height.0).abs() <= 1.0
            })
        };

        let quads_no_osc = host.last_quads.borrow().clone();
        assert!(
            !has_full_canvas_quad(&quads_no_osc),
            "paint_surface = false must skip redundant full-canvas background fill when no OSC 11 override is set: {quads_no_osc:?}"
        );

        let ansi_3cell_w = geom.cell_width * 3.0;
        let single_cell_w = geom.cell_width;
        assert!(
            quads_no_osc.iter().any(|&[_x, _y, w, h]| {
                (w - ansi_3cell_w).abs() <= 1.5 && (h - geom.cell_height).abs() <= 1.5
            }),
            "explicit ANSI cell background (3-col RED run) must still paint with paint_surface = false: {quads_no_osc:?}"
        );
        assert!(
            quads_no_osc.iter().any(|&[_x, _y, w, h]| {
                (w - single_cell_w).abs() <= 1.5 && (h - geom.cell_height).abs() <= 1.5
            }),
            "focused block cursor fill must still paint with paint_surface = false: {quads_no_osc:?}"
        );

        // Drag-select across "PLAIN" on row 0 and verify selection background fill paints.
        host.last_quads.borrow_mut().clear();
        let win_bounds = host
            .terminal_ref
            .window_bounds()
            .expect("terminal window bounds");
        let scale = geom.scale.max(1.0);
        let row0_y = (win_bounds.origin.y.0 + geom.origin_y + geom.cell_height * 0.5) / scale;
        let col4_x = (win_bounds.origin.x.0 + geom.origin_x + geom.cell_width * 4.2) / scale;
        let col9_x = (win_bounds.origin.x.0 + geom.origin_x + geom.cell_width * 8.8) / scale;
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(Point::new(CssPx(col4_x), CssPx(row0_y)))
                .with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(20)),
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(Point::new(CssPx(col9_x), CssPx(row0_y))),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(30)),
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(Point::new(CssPx(col9_x), CssPx(row0_y)))
                .with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::from_origin(Duration::from_millis(40)),
        });
        pump_frames(harness);

        let quads_selected = host.last_quads.borrow().clone();
        assert!(
            !has_full_canvas_quad(&quads_selected),
            "mouse selection must not trigger full-canvas fill when paint_surface = false"
        );
        assert!(
            quads_selected.iter().any(|&[_x, _y, w, h]| {
                w >= geom.cell_width * 4.5 && (h - geom.cell_height).abs() <= 1.5
            }),
            "selection highlight run must paint cell background quads when paint_surface = false: {quads_selected:?}"
        );

        // When an app inside the PTY sets an explicit OSC 11 background override, fill it!
        host.last_quads.borrow_mut().clear();
        host.terminal.write(b"osc\n");
        wait_for_pty(harness, || {
            host.terminal.title().get_untracked().as_deref() == Some("osc11")
        });
        pump_frames(harness);
        let quads_osc = host.last_quads.borrow().clone();
        assert!(
            has_full_canvas_quad(&quads_osc),
            "explicit OSC 11 background override must paint full-canvas fill even when paint_surface = false"
        );

        // Resetting OSC 11 via OSC 111 returns to single-layer host translucency.
        host.last_quads.borrow_mut().clear();
        host.terminal.write(b"rst\n");
        wait_for_pty(harness, || {
            host.terminal.title().get_untracked().as_deref() == Some("reset")
        });
        pump_frames(harness);
        let quads_reset = host.last_quads.borrow().clone();
        assert!(
            !has_full_canvas_quad(&quads_reset),
            "resetting OSC 11 override must restore skipped canvas fill when paint_surface = false"
        );
    });

    // Standalone default (`paint_surface = true`) still paints its own surface background.
    let standalone_opts = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec![
            "-c".into(),
            "printf '\\033]0;standalone\\007'; sleep 60".into(),
        ],
        ..SpawnOptions::default()
    };
    run_headless_terminal(standalone_opts, Preferences::default(), |harness, host| {
        wait_for_pty(harness, || {
            host.terminal.session().and_then(|s| s.title()).as_deref() == Some("standalone")
        });
        pump_frames(harness);
        let root_bounds = host.terminal_ref.bounds().expect("terminal root bounds");
        let quads = host.last_quads.borrow().clone();
        assert!(
            quads.iter().any(|&[_x, _y, w, h]| {
                (w - root_bounds.size.width.0).abs() <= 1.0
                    && (h - root_bounds.size.height.0).abs() <= 1.0
            }),
            "standalone TerminalView (paint_surface = true) must paint its surface background"
        );
    });
}

#[test]
fn terminal_stylesheet_keeps_status_and_error_lines_transparent_when_host_paints_surface() {
    let status = css_rule_body(TERMINAL_CSS, ".gds-terminal-status-line ")
        .expect(".gds-terminal-status-line rule");
    assert!(
        status.contains("background-color: transparent;"),
        ".gds-terminal-status-line must default to transparent when paint_surface = false: {status}"
    );
    let error = css_rule_body(TERMINAL_CSS, ".gds-terminal-error-line ")
        .expect(".gds-terminal-error-line rule");
    assert!(
        error.contains("background-color: transparent;"),
        ".gds-terminal-error-line must default to transparent when paint_surface = false: {error}"
    );
    let standalone_status = css_rule_body(
        TERMINAL_CSS,
        ".gds-terminal-root[data-paint-surface=\"true\"] .gds-terminal-status-line ",
    )
    .expect("standalone status rule");
    assert!(standalone_status.contains("var(--gds-content-bg, var(--gds-content))"));
    let standalone_error = css_rule_body(
        TERMINAL_CSS,
        ".gds-terminal-root[data-paint-surface=\"true\"] .gds-terminal-error-line ",
    )
    .expect("standalone error rule");
    assert!(standalone_error.contains("var(--gds-content-bg, var(--gds-content))"));
}

#[test]
fn focus_requested_before_terminal_view_mounts_takes_effect_once_node_binds() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec!["-c".into(), "sleep 60".into()],
        ..SpawnOptions::default()
    };
    run_headless_terminal_inner(
        options,
        Preferences::default(),
        true,
        true,
        |harness, host| {
            pump_frames(harness);
            let terminal_id = host
                .terminal_ref
                .get_untracked()
                .expect("terminal node bound");
            assert_eq!(
                host.focused.get_untracked(),
                Some(terminal_id),
                "deferred terminal.focus() called before TerminalView mounted must focus the node once bound"
            );
            assert!(host.terminal.focused_signal().get_untracked());
        },
    );
}

#[test]
fn reactive_signal_types_remain_unsync() {
    let owner = zgui::reactive::Owner::new();
    owner.with(|| {
        let sig: zgui::reactive::RwSignal<usize, LocalStorage> =
            zgui::reactive::RwSignal::new_local(1);
        assert_eq!(sig.get_untracked(), 1);
    });
    owner.cleanup();
}
