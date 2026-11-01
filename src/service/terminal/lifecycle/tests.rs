use super::{Status, Terminal};
use crate::service::terminal::SpawnOptions;
use std::task::{Context, Poll, Waker};

#[test]
fn shutdown_waits_for_inflight_startup_and_is_idempotent() {
    let (terminal, launch) = Terminal::prepare(SpawnOptions::default());
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
    let (terminal, launch) = Terminal::prepare(SpawnOptions::default());
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
    let directory = tempfile::tempdir().expect("service working directory");
    let (terminal, launch) = Terminal::prepare(options(directory.path().to_owned()));
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
fn startup_waiters_do_not_keep_the_process_owner_alive() {
    let directory = tempfile::tempdir().expect("service working directory");
    let (terminal, launch) = Terminal::prepare(options(directory.path().to_owned()));
    let ready = terminal.ready();
    drop(terminal);
    launch.run();
    assert!(futures_lite::future::block_on(ready).is_none());
}
