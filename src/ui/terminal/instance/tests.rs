use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zgui::reactive::Owner;

use super::{Preferences, Terminal};
use crate::app::Gds;
use crate::appearance::theme;
use crate::service::task_manager::Supervisor;
use crate::service::terminal::SpawnOptions;

#[test]
fn installed_themes_reach_starting_and_hidden_terminals_without_paint() {
    zgui::reactive::install().expect("install zgui reactive runtime");
    let supervisor = Supervisor::new(false).expect("test Tokio supervisor");
    let directory = tempfile::tempdir().expect("terminal query directory");
    let owner = Owner::new();
    let runtime = supervisor
        .scope()
        .runtime_handle()
        .expect("supervisor runtime handle")
        .clone();

    owner.with(|| {
        let gds = Gds::install(supervisor.scope());
        gds.appearance()
            .theme()
            .apply(&theme::Appearance {
                source: theme::Source::Builtin,
                ..theme::Appearance::default()
            })
            .expect("initial theme");

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

        let terminal = Terminal::spawn(
            SpawnOptions {
                cwd: directory.path().to_owned(),
                ..SpawnOptions::default()
            },
            preferences,
            &gds.tasks().scope(),
        )
        .expect("spawn hidden terminal");

        let first = install_and_expect(&gds, theme::ModePreference::Light);
        let session = runtime
            .block_on(terminal.ready())
            .expect("background launch completed");
        session.write(b"go\n").expect("first query");
        assert_reply(directory.path(), "first", &first);

        let second = install_and_expect(&gds, theme::ModePreference::Dark);
        assert_ne!(first, second, "fixture must change the queried theme role");
        session.write(b"go\n").expect("second query");
        assert_reply(directory.path(), "second", &second);

        runtime.block_on(terminal.shutdown());
    });
    owner.cleanup();
}

fn install_and_expect(gds: &Gds, mode: theme::ModePreference) -> String {
    gds.appearance()
        .theme()
        .apply(&theme::Appearance {
            source: theme::Source::Builtin,
            mode,
            ..Default::default()
        })
        .expect("install theme");
    zgui::reactive::flush();
    let colors = crate::ui::terminal::view::palette::Palette::current().query_colors();
    let color = colors[alacritty_terminal::vte::ansi::NamedColor::Foreground as usize];
    format!(
        "\x1b]10;rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}\x1b\\",
        color.r, color.g, color.b
    )
}

fn assert_reply(directory: &Path, name: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !directory.join(format!("{name}.done")).exists() {
        assert!(Instant::now() < deadline, "query timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    let reply = std::fs::read_to_string(directory.join(format!("{name}.reply")))
        .expect("PTY color response");
    assert_eq!(reply, expected);
}
