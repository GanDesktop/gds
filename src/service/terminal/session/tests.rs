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
        "trap '' HUP; sleep 60 & echo $! > job.pid; printf READY; wait",
    ))
    .expect("spawn PTY");
    wait_until(|| screen(&session).contains("READY"));
    let pid = session.process_id().expect("Unix child PID");
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
        waitpid(Pid::from_raw(pid as i32), Some(WaitPidFlag::WNOHANG)),
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
        "trap '' HUP; printf READY; while :; do read value; done",
    ))
    .expect("spawn PTY");
    wait_until(|| screen(&session).contains("READY"));
    let pid = session.process_id().expect("Unix PID");
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
    let pid = session.process_id();
    session
        .write(b"stty -echo; PS1=''; printf 'READY_%s\\n' switch\r")
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
        .write(b"printf '\\033]2;%s\\007' \"$PWD\"\r")
        .expect("inspect cwd through process title");
    wait_until(|| session.title().as_deref() == target.to_str());
    assert_eq!(session.title().as_deref(), target.to_str());
    assert_eq!(
        session.process_id(),
        pid,
        "cd must preserve the original instance"
    );

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
