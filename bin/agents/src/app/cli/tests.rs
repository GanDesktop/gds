use super::*;
use bas::service::Target;
use clap::{CommandFactory, FromArgMatches};

#[test]
fn logs_are_opt_in_for_every_window_mode() {
    for flags in [vec![], vec!["--window"], vec!["--compact"]] {
        for enabled in [false, true] {
            let mut args = vec!["gds-agents"];
            args.extend(flags.iter().copied());
            if enabled {
                args.push("--enable-logs");
            }
            let matches = Args::command()
                .mut_arg("server", |arg| arg.env(None::<&str>))
                .try_get_matches_from(args)
                .expect("valid GUI arguments");
            assert_eq!(
                Args::from_arg_matches(&matches)
                    .expect("typed options")
                    .enable_logs,
                enabled
            );
        }
    }
}

#[test]
fn server_uses_saved_selection_unless_explicitly_overridden() {
    let parse = |args: Vec<&str>| {
        let matches = Args::command()
            .mut_arg("server", |arg| arg.env(None::<&str>))
            .try_get_matches_from(args)
            .expect("valid daemon target");
        Args::from_arg_matches(&matches)
            .expect("typed arguments")
            .server
    };
    assert_eq!(parse(vec!["gds-agents"]), None);
    for (value, expected) in [
        ("local", Target::Local),
        ("unix:///tmp/bas.sock", Target::Unix("/tmp/bas.sock".into())),
        ("pipe://bas-test", Target::Pipe("bas-test".into())),
        (
            "tcp://127.0.0.1",
            Target::Tcp("127.0.0.1".parse().expect("TCP")),
        ),
        ("[::1]", Target::Tcp("[::1]".parse().expect("IPv6"))),
    ] {
        assert_eq!(
            parse(vec!["gds-agents", "--server", value]),
            Some(expected.into())
        );
    }
}
