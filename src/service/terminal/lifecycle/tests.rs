use super::{Status, Terminal};
use crate::service::terminal::SpawnOptions;
use std::task::{Context, Poll, Waker};

#[test]
fn shutdown_waits_for_inflight_startup_and_is_idempotent() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test executor");
    let tasks = crate::service::task_manager::Scope::with_runtime(&runtime, false);
    let (terminal, launch) = Terminal::prepare(tasks.clone(), SpawnOptions::default());
    assert_eq!(terminal.status(), Status::Starting);
    let mut first = terminal.shutdown();
    let mut second = terminal.shutdown();
    assert_eq!(terminal.status(), Status::Stopping);
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(first.as_mut().poll(&mut cx), Poll::Pending);
    assert_eq!(second.as_mut().poll(&mut cx), Poll::Pending);
    drop(launch);
    assert_eq!(first.as_mut().poll(&mut cx), Poll::Ready(()));
    assert_eq!(second.as_mut().poll(&mut cx), Poll::Ready(()));
    assert_eq!(terminal.status(), Status::Closed);
    let mut late = terminal.shutdown();
    assert_eq!(late.as_mut().poll(&mut cx), Poll::Ready(()));
}

#[test]
fn cancelled_startup_finishes_without_a_window_or_process() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test executor");
    let tasks = crate::service::task_manager::Scope::with_runtime(&runtime, false);
    let (terminal, launch) = Terminal::prepare(tasks.clone(), SpawnOptions::default());
    let ready = terminal.ready();
    let shutdown = terminal.shutdown();
    launch.run();
    assert!(futures_lite::future::block_on(ready).is_none());
    futures_lite::future::block_on(shutdown);
    assert_eq!(terminal.status(), Status::Closed);
}

#[cfg(unix)]
fn options(cwd: std::path::PathBuf) -> SpawnOptions {
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("sh"))
        .chain(["/bin/sh".into()])
        .find(|path| path.is_absolute() && path.is_file())
        .expect("a POSIX shell for the service test");
    SpawnOptions {
        cwd,
        shell: Some(shell),
        args: vec!["-c".into(), "read value; exit 7".into()],
        ..Default::default()
    }
}

#[cfg(unix)]
#[test]
fn service_runs_input_and_reports_exit_without_a_window() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test executor");
    let tasks = crate::service::task_manager::Scope::with_runtime(&runtime, false);
    let directory = tempfile::tempdir().expect("service working directory");
    let (terminal, launch) = Terminal::prepare(tasks.clone(), options(directory.path().to_owned()));
    launch.run();
    let session = futures_lite::future::block_on(terminal.ready()).expect("ready session");
    assert_eq!(terminal.cwd(), directory.path());
    session.write(b"finish\r").expect("terminal input");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while terminal.status().is_running() {
        assert!(
            std::time::Instant::now() < deadline,
            "the process should exit"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        terminal.status(),
        Status::Exited {
            code: 7,
            signal: None
        }
    );
    futures_lite::future::block_on(terminal.shutdown());
    assert_eq!(terminal.status(), Status::Closed);
}

#[cfg(unix)]
#[test]
fn theme_queries_follow_updates_before_launch_and_without_a_view() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test executor");
    let tasks = crate::service::task_manager::Scope::with_runtime(&runtime, false);
    use alacritty_terminal::{term::color::COUNT, vte::ansi::Rgb};
    let directory = tempfile::tempdir().expect("query working directory");
    let mut options = options(directory.path().to_owned());
    options.palette = Some([Rgb { r: 1, g: 2, b: 3 }; COUNT]);
    options.args[1] = r"stty -echo -icanon min 1 time 0
printf '\033]10;?\033\\'
dd bs=1 count=25 of=start.reply 2>/dev/null
: > start.done
read go
printf '\033]10;?\033\\\033]11;?\033\\\033]4;1;?\033\\'
dd bs=1 count=76 of=live.reply 2>/dev/null
: > live.done
"
    .into();
    let (terminal, launch) = Terminal::prepare(tasks.clone(), options);
    // The launch already exists, but the native process has not started yet.
    terminal.set_palette(
        [Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        }; COUNT],
    );
    launch.run();
    let session = futures_lite::future::block_on(terminal.ready()).expect("ready session");
    wait_for_file(&directory.path().join("start.done"));
    assert_eq!(
        std::fs::read(directory.path().join("start.reply")).expect("startup reply"),
        b"\x1b]10;rgb:1212/3434/5656\x1b\\"
    );
    // No GPUI view or prepaint exists, even though this session is already running.
    terminal.set_palette(
        [Rgb {
            r: 0xab,
            g: 0xcd,
            b: 0xef,
        }; COUNT],
    );
    session.write(b"go\n").expect("release query barrier");
    wait_for_file(&directory.path().join("live.done"));
    assert_eq!(
        std::fs::read(directory.path().join("live.reply")).expect("live replies"),
        b"\x1b]10;rgb:abab/cdcd/efef\x1b\\\x1b]11;rgb:abab/cdcd/efef\x1b\\\x1b]4;1;rgb:abab/cdcd/efef\x1b\\"
    );
    futures_lite::future::block_on(terminal.shutdown());
}

#[cfg(unix)]
fn wait_for_file(path: &std::path::Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "shell query timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn startup_waiters_do_not_keep_the_process_owner_alive() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test executor");
    let tasks = crate::service::task_manager::Scope::with_runtime(&runtime, false);
    let directory = tempfile::tempdir().expect("service working directory");
    let (terminal, launch) = Terminal::prepare(tasks.clone(), options(directory.path().to_owned()));
    let ready = terminal.ready();
    drop(terminal);
    launch.run();
    assert!(futures_lite::future::block_on(ready).is_none());
}

#[cfg(unix)]
#[test]
fn application_shutdown_reaps_terminal_with_the_service_still_retained() {
    let supervisor = crate::service::task_manager::Supervisor::new(false).expect("executor");
    let tasks = supervisor.scope();
    let directory = tempfile::tempdir().expect("terminal directory");
    let (terminal, launch) = Terminal::prepare(tasks.clone(), options(directory.path().to_owned()));
    tasks.spawn_blocking(move || launch.run());
    let session = supervisor
        .block_on(terminal.ready())
        .unwrap()
        .expect("session");
    futures_lite::future::block_on(tasks.shutdown());
    futures_lite::future::block_on(terminal.shutdown());
    assert_eq!(terminal.status(), Status::Closed);
    assert!(!session.is_running());
}
