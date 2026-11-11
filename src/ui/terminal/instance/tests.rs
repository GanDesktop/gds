use super::{Preferences, Terminal};
use crate::appearance::theme;
use gds::app::AppExt as _;
use gpui_kit::{AppContext, TestAppContext, px, size};
use std::path::{Path, PathBuf};

#[gpui_kit::test]
fn installed_themes_reach_starting_and_hidden_terminals_without_paint(cx: &mut TestAppContext) {
    // This integration test intentionally receives real PTY worker notifications.
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("terminal query directory");
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds().tasks().init_test(false);
        theme::apply(&theme::Appearance::default(), cx).expect("initial theme");
    });
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|path| path.join("sh"))
        .chain([PathBuf::from("/bin/sh")])
        .find(|path| path.is_absolute() && path.is_file())
        .expect("POSIX shell");
    let preferences = Preferences {
        shell: Some(shell),
        shell_args: vec![
            "-c".into(),
            r"stty -echo -icanon min 1 time 0
read go
printf '\033]10;?\033\\'
dd bs=1 count=25 of=first.reply 2>/dev/null
: > first.done
read go
printf '\033]10;?\033\\'
dd bs=1 count=25 of=second.reply 2>/dev/null
: > second.done
"
            .into(),
        ],
        ..Default::default()
    };
    let mut terminal = None;
    let _window = cx.open_window(size(px(100.), px(100.)), |window, cx| {
        terminal =
            Some(cx.new(|cx| Terminal::new(directory.path().to_owned(), preferences, window, cx)));
        // The terminal is retained but deliberately never attached to the render tree.
        gpui_kit::Empty
    });
    let terminal = terminal.expect("hidden terminal");
    let first = install(theme::ModePreference::Light, cx);
    cx.run_until_parked();
    let ready = cx.update(|cx| terminal.read(cx).service.ready());
    let session = futures_lite::future::block_on(ready).expect("background launch completed");
    session.write(b"go\n").expect("first query");
    assert_reply(directory.path(), "first", &first);
    let second = install(theme::ModePreference::Dark, cx);
    cx.run_until_parked();
    assert_ne!(first, second, "fixture must change the queried theme role");
    session.write(b"go\n").expect("second query");
    assert_reply(directory.path(), "second", &second);
    let shutdown = cx.update(|cx| terminal.read(cx).service.shutdown());
    futures_lite::future::block_on(shutdown);
}

fn install(mode: theme::ModePreference, cx: &mut TestAppContext) -> String {
    cx.update(|cx| {
        theme::apply(
            &theme::Appearance {
                mode,
                ..Default::default()
            },
            cx,
        )
        .expect("install theme");
        let colors = crate::ui::terminal::view::palette::Palette::new(cx).query_colors();
        let color = colors[alacritty_terminal::vte::ansi::NamedColor::Foreground as usize];
        format!(
            "\x1b]10;rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}\x1b\\",
            color.r, color.g, color.b
        )
    })
}

fn assert_reply(directory: &Path, name: &str, expected: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !directory.join(format!("{name}.done")).exists() {
        assert!(std::time::Instant::now() < deadline, "query timed out");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let reply = std::fs::read_to_string(directory.join(format!("{name}.reply")))
        .expect("PTY color response");
    assert_eq!(reply, expected);
}
