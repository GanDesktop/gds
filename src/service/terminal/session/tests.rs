#![cfg(unix)]

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
    term::cell::Flags,
    vte::ansi::{Color, NamedColor},
};

use super::{Error, Session, SpawnOptions, Status};

fn shell() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("sh"))
        .chain([PathBuf::from("/bin/sh")])
        .find(|path| path.is_absolute() && path.is_file())
        .expect("PTY integration tests require a POSIX sh")
}

fn options(cwd: PathBuf, script: &str) -> SpawnOptions {
    SpawnOptions {
        cwd,
        shell: Some(shell()),
        args: vec!["-c".into(), script.into()],
        ..SpawnOptions::default()
    }
}

#[track_caller]
fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "PTY condition did not become true within 8 seconds"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn screen(session: &Session) -> String {
    session.with_term(|term| {
        let grid = term.grid();
        (0..grid.screen_lines())
            .map(|row| {
                (0..grid.columns())
                    .map(|column| grid[Line(row as i32)][Column(column)].c)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn wait_for_exit(session: &Session) -> Status {
    wait_until(|| !session.is_running());
    session.status()
}

#[test]
fn real_pty_preserves_split_utf8_wide_cells_ansi_and_exit_status() {
    let directory = tempfile::tempdir().expect("test directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        r"printf '\033[31m\344'; sleep 0.03; printf '\275\240\345\245\275\033[0m\r\n'; exit 7",
    ))
    .expect("spawn PTY");
    assert_eq!(
        wait_for_exit(&session),
        Status::Exited {
            code: 7,
            signal: None
        }
    );
    session.with_term(|term| {
        let row = &term.grid()[Line(0)];
        assert_eq!(row[Column(0)].c, '你');
        assert_eq!(row[Column(2)].c, '好');
        assert!(row[Column(0)].flags.contains(Flags::WIDE_CHAR));
        assert!(row[Column(1)].flags.contains(Flags::WIDE_CHAR_SPACER));
        assert_eq!(row[Column(0)].fg, Color::Named(NamedColor::Red));
    });
    assert!(matches!(session.write(b"ignored"), Err(Error::Closed)));
    session.terminate();
    session.terminate();
    assert_eq!(
        session.status(),
        Status::Exited {
            code: 7,
            signal: None
        }
    );
}

#[test]
fn real_pty_starts_in_literal_directory_and_resizes_kernel_and_grid() {
    let parent = tempfile::tempdir().expect("test directory");
    let directory = parent.path().join("space ' ; $ directory");
    std::fs::create_dir(&directory).expect("literal directory");
    let session = Session::spawn(options(
        directory.clone(),
        "stty -echo; printf 'READY\\n'; read value; printf 'SIZE:'; stty size; printf 'CWD:'; pwd",
    ))
    .expect("spawn PTY");
    wait_until(|| screen(&session).contains("READY"));
    session
        .resize_with_pixels(103, 37, 8, 16)
        .expect("queue resize");
    wait_until(|| session.with_term(|term| term.columns() == 103 && term.screen_lines() == 37));
    session.write(b"go\n").expect("write to resized PTY");
    assert_eq!(
        wait_for_exit(&session),
        Status::Exited {
            code: 0,
            signal: None
        }
    );
    let output = screen(&session);
    assert!(output.contains("SIZE:37 103"), "{output}");
    assert!(
        output.contains(directory.to_str().expect("UTF-8 fixture")),
        "{output}"
    );
}

#[test]
fn real_pty_query_responses_return_through_writer() {
    let directory = tempfile::tempdir().expect("test directory");
    let session = Session::spawn(options(directory.path().to_owned(),
        r"stty -echo -icanon min 1 time 0; printf '\033[6n'; dd bs=1 count=6 2>/dev/null | od -An -tx1"))
        .expect("spawn PTY");
    assert_eq!(
        wait_for_exit(&session),
        Status::Exited {
            code: 0,
            signal: None
        }
    );
    assert!(
        screen(&session).contains("1b 5b 31 3b 31 52"),
        "DSR must be answered on the PTY, not merely sent to UI"
    );
}

#[test]
fn real_pty_control_c_is_a_terminal_signal_not_a_text_command() {
    let directory = tempfile::tempdir().expect("test directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        "stty -echo; trap 'printf SIGNAL; exit 42' INT; printf READY; while :; do read value; done",
    ))
    .expect("spawn PTY");
    wait_until(|| screen(&session).contains("READY"));
    session.write(&[3]).expect("send VINTR");
    assert_eq!(
        wait_for_exit(&session),
        Status::Exited {
            code: 42,
            signal: None
        }
    );
    assert!(screen(&session).contains("SIGNAL"));
}

#[test]
fn invalid_cwd_and_shell_fail_instead_of_falling_back() {
    let directory = tempfile::tempdir().expect("test directory");
    for cwd in [
        PathBuf::new(),
        PathBuf::from("relative"),
        directory.path().join("missing"),
    ] {
        assert!(matches!(
            Session::spawn(options(cwd, "exit")),
            Err(Error::InvalidDirectory(_))
        ));
    }
    let file = directory.path().join("not-directory");
    std::fs::write(&file, b"data").expect("fixture");
    assert!(matches!(
        Session::spawn(options(file, "exit")),
        Err(Error::InvalidDirectory(_))
    ));
    let mut invalid = options(directory.path().to_owned(), "exit");
    invalid.shell = Some(directory.path().join("missing-shell"));
    assert!(matches!(
        Session::spawn(invalid),
        Err(Error::InvalidShell(_))
    ));
}

#[test]
fn explicit_invalid_executable_reports_spawn_failure() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().expect("test directory");
    let executable = directory.path().join("bad-interpreter");
    std::fs::write(&executable, b"#!/definitely/missing/interpreter\n").expect("fixture");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("executable fixture");
    let mut invalid = options(directory.path().to_owned(), "");
    invalid.shell = Some(executable);
    match Session::spawn(invalid) {
        Err(Error::Pty(_)) => {}
        Err(error) => panic!("Expected an exec/PTY failure, got {error:?}"),
        Ok(session) => {
            let status = wait_for_exit(&session);
            panic!(
                "The invalid interpreter unexpectedly launched: {status:?}; output={:?}",
                screen(&session)
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn final_handle_drop_stops_jobs_and_reaps_direct_child_without_worker_arc_cycle() {
    use nix::{
        errno::Errno,
        sys::wait::{WaitPidFlag, waitpid},
        unistd::Pid,
    };
    let directory = tempfile::tempdir().expect("test directory");
    let child_pid_file = directory.path().join("job.pid");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        "trap '' HUP; echo $$ > shell.pid; sleep 60 & echo $! > job.pid; printf READY; wait",
    ))
    .expect("spawn PTY");
    wait_until(|| screen(&session).contains("READY"));
    let pid: i32 = std::fs::read_to_string(directory.path().join("shell.pid"))
        .expect("shell pid")
        .trim()
        .parse()
        .expect("numeric shell pid");
    let job: i32 = std::fs::read_to_string(&child_pid_file)
        .expect("background job pid")
        .trim()
        .parse()
        .expect("numeric background job pid");
    let clone = session.clone();
    let updates = session.updates();
    drop(session);
    assert!(
        clone.is_running(),
        "one surviving handle retains the session"
    );
    let before_drop = Instant::now();
    drop(clone);
    assert!(
        before_drop.elapsed() < Duration::from_millis(100),
        "Drop must not wait for termination grace"
    );
    wait_until(|| *updates.borrow());
    assert_eq!(
        waitpid(Pid::from_raw(pid), Some(WaitPidFlag::WNOHANG)),
        Err(Errno::ECHILD),
        "worker must reap its direct child"
    );
    wait_until(|| {
        let stat = std::fs::read_to_string(format!("/proc/{job}/stat"));
        match stat {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Ok(stat) => stat
                .rsplit_once(") ")
                .is_some_and(|(_, fields)| fields.starts_with("Z ")),
            _ => false,
        }
    });
}

#[test]
fn output_flood_preserves_tail_and_bounds_scrollback() {
    let directory = tempfile::tempdir().expect("test directory");
    let mut spawn = options(
        directory.path().to_owned(),
        r#"i=0; while [ "$i" -lt 12000 ]; do printf 'line-%s\n' "$i"; i=$((i+1)); done; printf FINAL-TAIL"#,
    );
    spawn.scrollback = 64;
    let session = Session::spawn(spawn).expect("spawn PTY");
    assert_eq!(
        wait_for_exit(&session),
        Status::Exited {
            code: 0,
            signal: None
        }
    );
    assert!(screen(&session).contains("FINAL-TAIL"));
    session.with_term(|term| assert!(term.grid().history_size() <= 64));
}

#[cfg(target_os = "linux")]
#[test]
fn application_quit_waits_for_workers_and_rejects_later_spawns() {
    const CHILD_ENV: &str = "GDS_TERMINAL_QUIT_BARRIER_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        // The registry's shutdown is intentionally irreversible; isolate it
        // from the test harness's other concurrently running session tests.
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "service::terminal::session::tests::application_quit_waits_for_workers_and_rejects_later_spawns", "--nocapture"])
            .env(CHILD_ENV, "1").status().expect("isolated quit test");
        assert!(status.success());
        return;
    }
    let directory = tempfile::tempdir().expect("test directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        "trap '' HUP; echo $$ > shell.pid; printf READY; while :; do read value; done",
    ))
    .expect("spawn PTY");
    wait_until(|| screen(&session).contains("READY"));
    let pid: i32 = std::fs::read_to_string(directory.path().join("shell.pid"))
        .expect("shell pid")
        .trim()
        .parse()
        .expect("numeric shell pid");
    let updates = session.updates();
    // Weak registry entries must still find workers after UI handles are gone.
    drop(session);
    futures_lite::future::block_on(super::shutdown_all());
    assert!(*updates.borrow());
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "quit must wait for reap"
    );
    assert!(matches!(
        Session::spawn(options(directory.path().to_owned(), "exit")),
        Err(Error::Closed)
    ));
    futures_lite::future::block_on(super::shutdown_all());
}

#[cfg(target_os = "linux")]
#[test]
fn in_place_directory_change_is_rejected_but_manual_cd_and_jobs_work() {
    let parent = tempfile::tempdir().expect("test directory");
    let target = parent.path().join("新 folder ' $HOME");
    std::fs::create_dir(&target).expect("target directory");
    let session = Session::spawn(SpawnOptions {
        cwd: parent.path().to_owned(),
        shell: Some(shell()),
        args: vec!["-i".into()],
        columns: 180,
        ..SpawnOptions::default()
    })
    .expect("interactive shell");
    session
        .write(b"stty -echo; PS1=''; original_pid=$$; printf 'READY_%s\\n' switch\r")
        .expect("prepare prompt");
    wait_until(|| screen(&session).contains("READY_switch"));
    assert!(matches!(
        futures_lite::future::block_on(session.change_directory(&target)),
        Err(Error::ShellBusy)
    ));
    // Explicit terminal typing still works; the application does not inject it.
    session
        .write(&super::super::directory::command(&target).unwrap())
        .expect("manual cd");
    session
        .write(b"[ \"$original_pid\" = \"$$\" ] && printf '\\033]2;%s\\007' \"$PWD\"\r")
        .expect("inspect cwd through process title");
    wait_until(|| session.title().as_deref() == target.to_str());
    assert_eq!(session.title().as_deref(), target.to_str());

    session
        .write(b"sh -c 'printf BUSY_%s\\n foreground; sleep 30'; printf JOB_FINISHED\r")
        .expect("start foreground job");
    wait_until(|| screen(&session).contains("BUSY_foreground"));
    assert!(matches!(
        futures_lite::future::block_on(session.change_directory(parent.path())),
        Err(Error::ShellBusy)
    ));
    assert_eq!(session.status(), Status::Running);
    assert!(
        !screen(&session).contains("JOB_FINISHED"),
        "folder switching must not interrupt jobs"
    );
    session.terminate();
    futures_lite::future::block_on(session.wait_stopped());
}

#[cfg(target_os = "linux")]
#[test]
fn directory_change_does_not_submit_unconfirmed_shell_input() {
    let parent = tempfile::tempdir().expect("test directory");
    let target = parent.path().join("target");
    std::fs::create_dir(&target).expect("target directory");
    let marker = parent.path().join("UNCONFIRMED_EXECUTED");
    let session = Session::spawn(SpawnOptions {
        cwd: parent.path().to_owned(),
        shell: Some(shell()),
        args: vec!["-i".into()],
        columns: 180,
        ..SpawnOptions::default()
    })
    .expect("interactive shell");
    session
        .write(b"PS1=''; printf 'READY_%s\\n' pending\r")
        .unwrap();
    wait_until(|| screen(&session).contains("READY_pending"));
    // No Enter: this command belongs to the user, not to the folder switch.
    session
        .write(b"printf EXECUTED > UNCONFIRMED_EXECUTED #")
        .unwrap();
    wait_until(|| screen(&session).contains("UNCONFIRMED_EXECUTED #"));
    wait_until(|| super::lock(&session.0.shared.state.control).input_bytes == 0);
    let result = futures_lite::future::block_on(session.change_directory(&target));
    // Cancel the unconfirmed line, then use a shell round-trip as a barrier.
    // If switching injected Enter, cancelling is already too late.
    session
        .write(b"\x15printf 'BARRIER_%s\\n' pending\r")
        .unwrap();
    wait_until(|| screen(&session).contains("BARRIER_pending"));
    let executed = marker.exists();
    session.terminate();
    futures_lite::future::block_on(session.wait_stopped());
    assert!(
        !executed,
        "directory switch submitted the user's unconfirmed command"
    );
    assert!(
        result.is_err(),
        "no shell integration proves the input line empty"
    );
}

#[test]
fn theme_changes_during_startup_preserve_osc_overrides_until_reset() {
    use alacritty_terminal::{term::color::COUNT, vte::ansi::Rgb};
    let directory = tempfile::tempdir().expect("query working directory");
    let mut options = options(
        directory.path().to_owned(),
        r"stty -echo -icanon min 1 time 0
printf '\033]10;?\033\\'
dd bs=1 count=25 of=start.reply 2>/dev/null
printf '\033]10;#112233\007\033]4;1;#445566\007\033]2;OVERRIDDEN\007'
read go
printf '\033]10;?\033\\\033]4;1;?\033\\'
dd bs=1 count=51 of=override.reply 2>/dev/null
printf '\033]110\007\033]104;1\007\033]10;?\033\\\033]4;1;?\033\\'
dd bs=1 count=51 of=reset.reply 2>/dev/null
",
    );
    options.palette = Some([Rgb { r: 1, g: 2, b: 3 }; COUNT]);
    let palette = super::QueryPalette::new(options.palette);
    // Retain exactly the handle already handed to the spawning thread. A later
    // update must be visible even though its SpawnOptions snapshot is now stale.
    let launch_palette = palette.clone();
    palette.set(
        [Rgb {
            r: 0x78,
            g: 0x9a,
            b: 0xbc,
        }; COUNT],
    );
    let session = Session::spawn_with_palette(options, launch_palette).expect("spawn query PTY");
    wait_until(|| session.title().as_deref() == Some("OVERRIDDEN"));
    assert_eq!(
        std::fs::read(directory.path().join("start.reply")).expect("startup palette reply"),
        b"\x1b]10;rgb:7878/9a9a/bcbc\x1b\\"
    );
    palette.set(
        [Rgb {
            r: 0xab,
            g: 0xcd,
            b: 0xef,
        }; COUNT],
    );
    session.write(b"go\n").expect("release query barrier");
    assert!(matches!(
        wait_for_exit(&session),
        Status::Exited { code: 0, .. }
    ));
    assert_eq!(
        std::fs::read(directory.path().join("override.reply")).expect("OSC overrides reply"),
        b"\x1b]10;rgb:1111/2222/3333\x1b\\\x1b]4;1;rgb:4444/5555/6666\x1b\\"
    );
    assert_eq!(
        std::fs::read(directory.path().join("reset.reply")).expect("OSC reset reply"),
        b"\x1b]10;rgb:abab/cdcd/efef\x1b\\\x1b]4;1;rgb:abab/cdcd/efef\x1b\\"
    );
    futures_lite::future::block_on(session.wait_stopped());
}

#[cfg(target_os = "linux")]
#[test]
fn shutdown_cleans_foreground_background_and_stopped_job_groups_before_completion() {
    use nix::{
        errno::Errno,
        sys::wait::{WaitPidFlag, waitpid},
        unistd::Pid,
    };

    let directory = tempfile::tempdir().expect("job-control directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        r#"
set -m
trap '' HUP TERM
echo $$ > shell.pid
"$0" -c 'trap "" HUP TERM; echo $$ > stopped.pid; kill -STOP $$; while :; do sleep 60; done' &
"$0" -c 'trap "" HUP TERM; echo $$ > background.pid; while :; do sleep 60; done' &
"$0" -c 'trap "" HUP TERM; echo $$ > foreground.pid; printf READY; while :; do read value; done'
"#,
    ))
    .expect("spawn job-control PTY");
    wait_until(|| screen(&session).contains("READY"));
    let pid = |name: &str| -> i32 {
        let path = directory.path().join(format!("{name}.pid"));
        let mut value = None;
        wait_until(|| {
            value = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| text.trim().parse().ok());
            value.is_some()
        });
        value.expect("complete numeric pid")
    };
    let shell = pid("shell");
    let stopped = pid("stopped");
    let background = pid("background");
    let foreground = pid("foreground");
    wait_until(|| process_state(stopped) == Some('T'));
    for job in [stopped, background, foreground] {
        assert_eq!(
            nix::unistd::getsid(Some(Pid::from_raw(job)))
                .expect("job session")
                .as_raw(),
            shell
        );
        assert_ne!(
            nix::unistd::getpgid(Some(Pid::from_raw(job)))
                .expect("job group")
                .as_raw(),
            shell,
            "fixture must exercise job-control groups separate from the shell"
        );
    }
    session.terminate();
    let done = session.updates();
    wait_until(|| *done.borrow());
    assert_eq!(
        waitpid(Pid::from_raw(shell), Some(WaitPidFlag::WNOHANG)),
        Err(Errno::ECHILD),
        "cleanup completion must follow direct-child reap"
    );
    for job in [stopped, background, foreground] {
        wait_until(|| matches!(process_state(job), None | Some('Z')));
    }
}

#[cfg(target_os = "linux")]
fn process_state(pid: i32) -> Option<char> {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => Some(
            stat.rsplit_once(") ")
                .expect("proc stat fields")
                .1
                .chars()
                .next()
                .expect("process state"),
        ),
        // A job that exits between the /proc entry lookup and the read reports
        // ESRCH; it is gone, which callers already accept as an outcome.
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || error.raw_os_error() == Some(nix::libc::ESRCH) =>
        {
            None
        }
        Err(error) => panic!("cannot inspect test process: {error}"),
    }
}

#[test]
fn osc52_requests_are_disabled_while_regular_queries_still_work() {
    let directory = tempfile::tempdir().expect("OSC52 directory");
    let session = Session::spawn(options(directory.path().to_owned(),
        r"stty -echo -icanon min 1 time 0; printf '\033]52;c;c2VjcmV0\033\\\033]52;c;?\033\\\033[6n'; dd bs=1 count=6 of=reply 2>/dev/null"))
        .expect("spawn OSC52 PTY");
    assert!(matches!(
        wait_for_exit(&session),
        Status::Exited { code: 0, .. }
    ));
    assert_eq!(
        std::fs::read(directory.path().join("reply")).expect("query bytes"),
        b"\x1b[1;1R",
        "disabled clipboard query must not inject a reply before DSR"
    );
}

#[test]
fn query_response_flood_is_bounded_and_completes_cleanup() {
    let directory = tempfile::tempdir().expect("query flood directory");
    let session = Session::spawn(options(directory.path().to_owned(),
        r"stty -echo -icanon min 1 time 0; while :; do printf '\033[6n\033[6n\033[6n\033[6n\033[6n\033[6n\033[6n\033[6n'; done"))
        .expect("spawn query flood PTY");
    assert!(
        matches!(wait_for_exit(&session), Status::Failed(error) if error.contains("response queue")),
        "a child that never consumes its replies must fail rather than grow an unbounded queue"
    );
    assert!(
        super::lock(&session.0.shared.state.control).response_bytes <= super::MAX_RESPONSE_BYTES
    );
    let done = session.updates();
    wait_until(|| *done.borrow());
}

#[cfg(target_os = "linux")]
#[test]
fn direct_exit_wakes_cleanup_while_a_background_job_holds_the_slave_open() {
    let directory = tempfile::tempdir().expect("child exit directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        "trap '' HUP; sleep 60 & echo $! > job.pid; printf FINAL; exit 9",
    ))
    .expect("spawn inherited-slave PTY");
    assert_eq!(
        wait_for_exit(&session),
        Status::Exited {
            code: 9,
            signal: None
        }
    );
    assert!(screen(&session).contains("FINAL"));
    let job = std::fs::read_to_string(directory.path().join("job.pid"))
        .expect("background pid")
        .trim()
        .parse()
        .expect("numeric pid");
    let done = session.updates();
    wait_until(|| *done.borrow());
    wait_until(|| matches!(process_state(job), None | Some('Z')));
}

#[test]
fn synchronized_output_deadline_flushes_without_further_pty_activity() {
    let directory = tempfile::tempdir().expect("synchronized output directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        r"stty -echo; printf '\033[?2026hDEADLINE'; read value",
    ))
    .expect("spawn synchronized output PTY");
    // The child neither ends sync mode nor writes more bytes. Only the parser's
    // deadline can make its buffered text visible before we close the session.
    wait_until(|| screen(&session).contains("DEADLINE"));
    assert!(session.is_running());
    session.terminate();
    let done = session.updates();
    wait_until(|| *done.borrow());
}

#[test]
fn queued_responses_precede_queued_input_on_the_real_pty() {
    let directory = tempfile::tempdir().expect("priority directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        "stty -echo -icanon min 1 time 0; printf READY; dd bs=1 count=10 of=wire 2>/dev/null",
    ))
    .expect("spawn priority PTY");
    wait_until(|| screen(&session).contains("READY"));
    {
        // Admit both queues atomically so thread scheduling cannot choose the
        // input before the reply exists. Assert the wire, not queue internals.
        let mut control = super::lock(&session.0.shared.state.control);
        control.input.push_back(b"INPUT".to_vec());
        control.input_bytes += 5;
        control.responses.push_back(b"REPLY".to_vec());
        control.response_bytes += 5;
    }
    session.0.shared.state.wake();
    assert!(matches!(
        wait_for_exit(&session),
        Status::Exited { code: 0, .. }
    ));
    assert_eq!(
        std::fs::read(directory.path().join("wire")).expect("PTY wire"),
        b"REPLYINPUT"
    );
}

#[test]
fn input_backpressure_is_atomic() {
    let directory = tempfile::tempdir().expect("test directory");
    let session = Session::spawn(options(
        directory.path().to_owned(),
        "stty -echo; printf READY; sleep 60",
    ))
    .expect("spawn PTY");
    wait_until(|| screen(&session).contains("READY"));
    assert!(matches!(
        session.write(&vec![b'x'; 4 * 1024 * 1024 + 1]),
        Err(Error::InputQueueFull)
    ));
    assert_eq!(super::lock(&session.0.shared.state.control).input_bytes, 0);
    session.terminate();
    session.terminate();
    assert!(matches!(wait_for_exit(&session), Status::Exited { .. }));
}
