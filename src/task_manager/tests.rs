use super::*;
use std::task::{Context, Poll, Waker};

mod inspection;

#[test]
fn root_waits_for_descendants_and_rejects_late_work() {
    let root = Scope::default();
    let child = root.child();
    let grandchild = child.child();
    let running = grandchild.register().unwrap();
    let mut shutdown = Box::pin(root.shutdown());
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(shutdown.as_mut().poll(&mut cx), Poll::Pending);
    assert!(grandchild.token().is_cancelled());
    assert!(grandchild.register().is_err());
    assert!(root.child().register().is_err());
    drop(running);
    assert_eq!(shutdown.as_mut().poll(&mut cx), Poll::Ready(()));
}

#[test]
fn dropping_owner_cancels_only_its_subtree() {
    let root = Scope::default();
    let first = root.child();
    let second = root.child();
    let descendant = first.child();
    drop(first);
    assert!(descendant.token().is_cancelled());
    assert!(!second.token().is_cancelled());
    assert!(second.register().is_ok());
}

#[tokio::test]
async fn cancellation_does_not_abort_cleanup_and_detached_tasks_are_waited() {
    let root = Scope::default();
    let owner = root.child();
    let cancel = owner.token();
    let (release, ready) = tokio::sync::oneshot::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let handle = owner
        .spawn(&tokio::runtime::Handle::current(), async move {
            cancel.cancelled().await;
            ready.await.unwrap();
            finished.send(()).unwrap();
        })
        .unwrap();
    drop(handle);
    root.cancel();
    let mut shutdown = Box::pin(root.shutdown());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), &mut shutdown)
            .await
            .is_err()
    );
    release.send(()).unwrap();
    shutdown.await;
    done.await.unwrap();
}

#[tokio::test]
async fn threads_and_blocking_jobs_share_the_root_barrier() {
    let root = Scope::default();
    let (send, receive) = std::sync::mpsc::channel();
    let thread = root
        .spawn_thread(std::thread::Builder::new(), move || {
            receive.recv().unwrap();
        })
        .unwrap();
    let (release, ready) = std::sync::mpsc::channel();
    let work = root
        .spawn_blocking(&tokio::runtime::Handle::current(), move || {
            ready.recv().unwrap()
        })
        .unwrap();
    root.cancel();
    send.send(()).unwrap();
    thread.join().unwrap();
    let mut shutdown = Box::pin(root.shutdown());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), &mut shutdown)
            .await
            .is_err()
    );
    release.send(()).unwrap();
    shutdown.await;
    work.await.unwrap();
}

#[cfg(unix)]
#[test]
fn cancelled_child_is_killed_and_reaped_before_completion() {
    let child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut guard = process::ChildGuard::new(child);
    assert_eq!(
        guard
            .wait(&cancel, std::time::Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::Interrupted
    );
    drop(guard);
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}

#[cfg(unix)]
#[tokio::test]
async fn async_process_captures_output_and_reaps_cancelled_child() {
    let mut command = tokio::process::Command::new("sh");
    command.args(["-c", "printf out; printf err >&2"]);
    let output = process::output(
        &mut command,
        &CancellationToken::new(),
        std::time::Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"err");

    let directory = tempfile::tempdir().unwrap();
    let root = Scope::default();
    let cancel = root.token();
    let mut command = tokio::process::Command::new("sh");
    command
        .current_dir(directory.path())
        .args(["-c", "echo $$ > pid; exec sleep 30"]);
    let task = root
        .spawn(&tokio::runtime::Handle::current(), async move {
            process::output(&mut command, &cancel, std::time::Duration::from_secs(5)).await
        })
        .unwrap();
    let pid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(directory.path().join("pid"))
                && let Ok(pid) = text.trim().parse::<u32>()
            {
                break pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    root.shutdown().await;
    assert_eq!(
        task.await.unwrap().unwrap_err().kind(),
        std::io::ErrorKind::Interrupted
    );
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}
