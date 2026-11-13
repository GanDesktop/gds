use super::View;
use crate::app::AppExt as _;
use crate::service::terminal::{Session, SpawnOptions};
use crate::ui::terminal::Preferences;
use gpui_kit::{
    AppContext, Context, Entity, FocusHandle, IntoElement, KeyBinding, Render, TestAppContext,
    Window, actions, component::Root, div, prelude::*, px, size, test::TestWindowExt,
};

actions!(
    terminal_focus_test,
    [WorkspaceOnly, Settings, Toggle, ReturnToHost]
);
struct Host {
    view: Entity<View>,
    focus: FocusHandle,
    next_focus: FocusHandle,
    workspace_actions: usize,
    settings: usize,
    toggles: usize,
    exits: usize,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .key_context("TerminalFocusTest")
            .track_focus(&self.focus)
            .tab_group()
            .tab_index(0)
            .on_action(cx.listener(|host, _: &WorkspaceOnly, _, _| host.workspace_actions += 1))
            .on_action(cx.listener(|host, _: &Settings, _, _| host.settings += 1))
            .on_action(cx.listener(|host, _: &Toggle, _, _| host.toggles += 1))
            .on_action(cx.listener(|host, _: &ReturnToHost, _, _| host.exits += 1))
            .child(
                div()
                    .track_focus(&self.next_focus)
                    .tab_index(0)
                    .size(px(10.)),
            )
            .child(self.view.clone())
    }
}

#[gpui_kit::test]
fn single_terminal_respects_its_hosts_key_map(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        cx.gds_mut().ui().init_terminal();
        cx.gds_mut()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("test messages");
        // The host, not the reusable terminal, chooses all these bindings.
        cx.bind_keys([
            KeyBinding::new(
                "ctrl-w",
                WorkspaceOnly,
                Some("TerminalFocusTest && !GdsTerminal"),
            ),
            KeyBinding::new("ctrl-,", Settings, Some("TerminalFocusTest")),
            KeyBinding::new("ctrl-`", Toggle, Some("TerminalFocusTest")),
            KeyBinding::new("f6", ReturnToHost, Some("TerminalFocusTest > GdsTerminal")),
        ]);
    });
    let tasks = cx.update(|cx| cx.gds().tasks().scope());
    let session = Session::spawn(
        &tasks,
        SpawnOptions {
            cwd: std::env::temp_dir(),
            args: vec!["-c".into(), "exit 0".into()],
            ..SpawnOptions::default()
        },
    )
    .expect("local shell for terminal focus test");
    let mut host = None;
    let window = cx.open_window(size(px(800.), px(480.)), |window, cx| {
        let view = cx.new(|cx| View::new(session.clone(), Preferences::default(), window, cx));
        let entity = cx.new(|cx| Host {
            view,
            focus: cx.focus_handle().tab_stop(true),
            next_focus: cx.focus_handle().tab_stop(true),
            workspace_actions: 0,
            settings: 0,
            toggles: 0,
            exits: 0,
        });
        host = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let host = host.expect("host entity");
    cx.update_window(window.into(), |_, window, cx| {
        host.read(cx).focus.clone().focus(window, cx);
        window.render_frame(cx);
        window.press("ctrl-w", cx);
        assert_eq!(host.read(cx).workspace_actions, 1);
        let inside = host.read(cx).view.read(cx).focus_handle();
        inside.focus(window, cx);
        window.render_frame(cx);
        window.press("ctrl-w", cx);
        window.press("ctrl-,", cx);
        window.press("ctrl-`", cx);
        window.press("f6", cx);
        let host = host.read(cx);
        assert_eq!(
            host.workspace_actions, 1,
            "host close must not steal shell Ctrl+W"
        );
        assert_eq!((host.settings, host.toggles, host.exits), (1, 1, 1));
    })
    .expect("terminal key dispatch");
    session.terminate();
}

fn wait_for_pty(mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "PTY response timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn screen(session: &Session) -> String {
    use alacritty_terminal::{
        grid::Dimensions,
        index::{Column, Line},
    };
    session.with_term(|term| {
        let grid = term.grid();
        (0..term.screen_lines())
            .flat_map(|row| {
                (0..term.columns()).map(move |column| grid[Line(row as i32)][Column(column)].c)
            })
            .collect()
    })
}

#[gpui_kit::test]
fn tab_routes_to_pty_and_outside_terminal_traversal_still_works(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        cx.gds_mut().ui().init_terminal();
        cx.gds_mut()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("test messages");
    });
    let tasks = cx.update(|cx| cx.gds().tasks().scope());
    let session = Session::spawn(&tasks, SpawnOptions {
        cwd: std::env::temp_dir(),
        args: vec!["-c".into(),
            "stty -echo -icanon min 1 time 0; printf '\\033]0;ready\\007'; dd bs=1 count=4 2>/dev/null | od -An -tx1".into()],
        ..SpawnOptions::default()
    }).expect("raw PTY for focus routing");
    wait_for_pty(|| session.title().as_deref() == Some("ready"));
    let mut host = None;
    let window = cx.open_window(size(px(800.), px(480.)), |window, cx| {
        let view = cx.new(|cx| View::new(session.clone(), Preferences::default(), window, cx));
        let entity = cx.new(|cx| Host {
            view,
            focus: cx.focus_handle().tab_stop(true),
            next_focus: cx.focus_handle().tab_stop(true),
            workspace_actions: 0,
            settings: 0,
            toggles: 0,
            exits: 0,
        });
        host = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let host = host.expect("host entity");
    cx.update_window(window.into(), |_, window, cx| {
        let outside = host.read(cx).focus.clone();
        let next = host.read(cx).next_focus.clone();
        outside.focus(window, cx);
        window.render_frame(cx);
        window.press("tab", cx);
        assert!(
            next.is_focused(window),
            "root Tab must still traverse outside the terminal"
        );
        window.press("shift-tab", cx);
        assert!(
            outside.is_focused(window),
            "root reverse traversal must still work"
        );
        let inside = host.read(cx).view.read(cx).focus_handle();
        inside.focus(window, cx);
        window.render_frame(cx);
        window.press("tab", cx);
        assert!(
            inside.is_focused(window),
            "Tab must not leave terminal input"
        );
        window.press("shift-tab", cx);
        assert!(
            inside.is_focused(window),
            "Shift+Tab must not leave terminal input"
        );
    })
    .expect("native key dispatch");
    wait_for_pty(|| screen(&session).contains("09 1b 5b 5a"));
    session.terminate();
}

#[gpui_kit::test]
fn tab_completes_a_filename_in_a_real_interactive_shell(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        cx.gds_mut().ui().init_terminal();
        cx.gds_mut()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("test messages");
    });
    let account_shell = nix::unistd::User::from_uid(nix::unistd::Uid::current())
        .expect("test account lookup")
        .map(|user| user.shell);
    let bash = account_shell
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
        .expect("shell completion integration test requires readline-enabled bash");
    let directory = tempfile::tempdir().expect("completion directory");
    std::fs::write(
        directory.path().join("gds-completion-token"),
        "COMPLETION_SUCCEEDED\n",
    )
    .expect("completion fixture");
    let tasks = cx.update(|cx| cx.gds().tasks().scope());
    let session = Session::spawn(
        &tasks,
        SpawnOptions {
            cwd: directory.path().to_owned(),
            shell: Some(bash),
            args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
            ..SpawnOptions::default()
        },
    )
    .expect("interactive bash for completion");
    session
        .write(b"PS1=''; stty -echo; bind '\"\\C-i\": complete'; printf '\\033]0;ready\\007'\r")
        .expect("initialize isolated readline session");
    wait_for_pty(|| session.title().as_deref() == Some("ready"));
    let mut terminal = None;
    let window = cx.open_window(size(px(800.), px(480.)), |window, cx| {
        let view = cx.new(|cx| View::new(session.clone(), Preferences::default(), window, cx));
        terminal = Some(view.clone());
        Root::new(view, window, cx)
    });
    let terminal = terminal.expect("terminal view");
    cx.update_window(window.into(), |_, window, cx| {
        let focus = terminal.read(cx).focus_handle();
        focus.focus(window, cx);
        window.render_frame(cx);
        window.input("cat gds-completion-t", cx);
        window.press("tab", cx);
        assert!(focus.is_focused(window));
        window.press("enter", cx);
    })
    .expect("shell completion key dispatch");
    wait_for_pty(|| screen(&session).contains("COMPLETION_SUCCEEDED"));
    session.terminate();
}

#[gpui_kit::test]
fn changing_terminal_typography_updates_existing_grid_metrics(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        cx.gds_mut().ui().init_terminal();
        cx.gds_mut()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("test messages");
    });
    let tasks = cx.update(|cx| cx.gds().tasks().scope());
    let session = Session::spawn(
        &tasks,
        SpawnOptions {
            cwd: std::env::temp_dir(),
            args: vec!["-c".into(), "exit 0".into()],
            ..SpawnOptions::default()
        },
    )
    .expect("local shell for typography test");
    let mut terminal = None;
    let window = cx.open_window(size(px(800.), px(480.)), |window, cx| {
        let view = cx.new(|cx| View::new(session.clone(), Preferences::default(), window, cx));
        terminal = Some(view.clone());
        Root::new(view, window, cx)
    });
    let terminal = terminal.expect("terminal view");
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let before = terminal.read(cx).geometry;
        let preferences = Preferences {
            font_family: ".SystemMonospace".into(),
            font_size: 24.,
            ..Preferences::default()
        };
        terminal.update(cx, |view, cx| view.set_preferences(preferences.clone(), cx));
        window.render_frame(cx);
        let view = terminal.read(cx);
        let after = view.geometry;
        assert!(after.cell.width > before.cell.width);
        assert!(after.cell.height > before.cell.height);
        assert!(
            after.bounds.size.width / after.cell.width
                < before.bounds.size.width / before.cell.width,
            "larger text must reduce grid columns"
        );
        assert!(
            after.bounds.size.height / after.cell.height
                < before.bounds.size.height / before.cell.height,
            "larger text must reduce grid rows"
        );
        assert!(view.fonts.as_ref().expect("resolved font").matches(
            &preferences.font_family,
            preferences.font_size,
            f32::from(window.scale_factor()),
        ));
        assert_eq!(view.preferences, preferences);
    })
    .expect("terminal typography update");
    session.terminate();
}

#[gpui_kit::test]
fn cursor_blink_task_tracks_focus_preferences_and_reduced_motion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        cx.gds_mut().ui().init_terminal();
        cx.gds_mut()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("test messages");
    });
    let tasks = cx.update(|cx| cx.gds().tasks().scope());
    let session = Session::spawn(
        &tasks,
        SpawnOptions {
            cwd: std::env::temp_dir(),
            args: vec!["-c".into(), "sleep 60".into()],
            ..SpawnOptions::default()
        },
    )
    .expect("local shell for blink lifecycle test");
    let mut host = None;
    let window = cx.open_window(size(px(800.), px(480.)), |window, cx| {
        let view = cx.new(|cx| View::new(session.clone(), Preferences::default(), window, cx));
        let entity = cx.new(|cx| Host {
            view,
            focus: cx.focus_handle().tab_stop(true),
            next_focus: cx.focus_handle().tab_stop(true),
            workspace_actions: 0,
            settings: 0,
            toggles: 0,
            exits: 0,
        });
        host = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let host = host.expect("host entity");
    cx.update_window(window.into(), |_, window, _| window.activate_window())
        .expect("activate terminal window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        let terminal = host.read(cx).view.clone();
        window.render_frame(cx);
        {
            let view = terminal.read(cx);
            assert!(!view.focused);
            assert!(view._blink.is_none());
            assert!(view.cursor_on);
        }

        let inside = terminal.read(cx).focus_handle();
        inside.focus(window, cx);
        window.render_frame(cx);
        {
            let view = terminal.read(cx);
            assert!(view.focused);
            assert!(view._blink.is_some());
            assert!(view.cursor_on);
        }

        terminal.update(cx, |view, cx| {
            view.cursor_on = false;
            view.set_preferences(
                Preferences {
                    cursor_blink: false,
                    ..Preferences::default()
                },
                cx,
            );
        });
        {
            let view = terminal.read(cx);
            assert!(view._blink.is_none());
            assert!(view.cursor_on);
        }

        terminal.update(cx, |view, cx| {
            view.set_preferences(Preferences::default(), cx);
        });
        assert!(terminal.read(cx)._blink.is_some());

        terminal.update(cx, |view, _| view.cursor_on = false);
        cx.set_reduce_motion(true);
        window.refresh();
        window.render_frame(cx);
        {
            let view = terminal.read(cx);
            assert!(view._blink.is_none());
            assert!(view.cursor_on);
        }

        cx.set_reduce_motion(false);
        window.refresh();
        window.render_frame(cx);
        assert!(terminal.read(cx)._blink.is_some());

        terminal.update(cx, |view, _| view.cursor_on = false);
        let outside = host.read(cx).next_focus.clone();
        outside.focus(window, cx);
        window.render_frame(cx);
        {
            let view = terminal.read(cx);
            assert!(!view.focused);
            assert!(view._blink.is_none());
            assert!(view.cursor_on);
        }
    })
    .expect("cursor blink lifecycle assertions");
    session.terminate();
}

#[gpui_kit::test]
fn terminal_grid_renders_tabs_and_combining_marks(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        cx.gds_mut().ui().init_terminal();
        cx.gds_mut()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("test messages");
    });
    let tasks = cx.update(|cx| cx.gds().tasks().scope());
    let session = Session::spawn(
        &tasks,
        SpawnOptions {
            cwd: std::env::temp_dir(),
            args: vec![
                "-c".into(),
                "printf '%s\\r\\n' 'A\te\u{0301}DONE'; sleep 60".into(),
            ],
            ..SpawnOptions::default()
        },
    )
    .expect("local shell for combining glyph test");
    wait_for_pty(|| screen(&session).contains("DONE"));
    let mut terminal = None;
    let window = cx.open_window(size(px(800.), px(480.)), |window, cx| {
        let view = cx.new(|cx| View::new(session.clone(), Preferences::default(), window, cx));
        terminal = Some(view.clone());
        Root::new(view, window, cx)
    });
    let terminal = terminal.expect("terminal view");
    cx.update_window(window.into(), |_, window, cx| {
        terminal.read(cx).focus_handle().focus(window, cx);
        window.render_frame(cx);
        assert!(terminal.read(cx).geometry.columns > 0);
    })
    .expect("combining glyph render");
    session.terminate();
}
