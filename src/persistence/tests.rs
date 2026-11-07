use super::*;
use std::time::{Duration, Instant};

/// A concurrently forked child shares our lock descriptor until it execs.
/// Tests wait that transient holder out instead of failing on the harness's own
/// subprocesses; a lock that is genuinely held still fails this budget.
const TEST_BUSY_RETRY: Duration = Duration::from_secs(10);

fn write_after_inherited_holders(
    file: &File,
    expected: Option<&[u8]>,
    bytes: &[u8],
) -> Result<(), Error> {
    let deadline = Instant::now() + TEST_BUSY_RETRY;
    loop {
        match file.write(expected, bytes) {
            Err(Error::Busy(_)) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

#[cfg(unix)]
#[test]
fn writable_configuration_below_a_readonly_ancestor_can_be_created_and_replaced() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("create fixture");
    let ancestor = root.path().join("readonly");
    let writable = ancestor.join("home");
    fs::create_dir_all(&writable).expect("create writable descendant");
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o555))
        .expect("make ancestor readonly");
    let file = File::new(writable.join(".config/gds/app/files/settings.toml"));

    let created = file.write(None, b"first");
    let replaced = write_after_inherited_holders(&file, Some(b"first"), b"second");
    let ancestor_mode = fs::metadata(&ancestor)
        .expect("inspect ancestor")
        .permissions()
        .mode();
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o755))
        .expect("restore fixture for cleanup");

    created.expect("create settings under writable descendant");
    replaced.expect("replace settings under writable descendant");
    assert_eq!(
        file.read().expect("reload settings").as_deref(),
        Some(b"second".as_slice())
    );
    assert_eq!(ancestor_mode & 0o777, 0o555);
}

/// `flock` follows the open file description, so a child that inherited the
/// lock descriptor keeps the lock alive until it execs. `O_CLOEXEC` closes the
/// descriptor on exec; the direct flag assertion pins that mechanism, and the
/// probe tolerates unrelated pre-exec children forked by parallel tests.
#[cfg(unix)]
#[test]
fn executed_children_do_not_inherit_the_configuration_lock() {
    use std::{
        io::{BufRead, BufReader},
        os::fd::AsFd,
        process::{Command, Stdio},
    };

    let root = tempfile::tempdir().expect("create fixture");
    let lock_path = root.path().join("settings.toml.lock");
    let lock = open_lock(&lock_path).expect("open lock");
    lock.try_lock().expect("hold lock");
    let flags = nix::fcntl::fcntl(lock.as_fd(), nix::fcntl::FcntlArg::F_GETFD)
        .expect("read lock descriptor flags");
    assert!(
        nix::fcntl::FdFlag::from_bits_truncate(flags).contains(nix::fcntl::FdFlag::FD_CLOEXEC),
        "the lock descriptor must not survive exec"
    );

    let mut child = Command::new("sh")
        .args(["-c", "echo ready; exec sleep 30"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn child process");
    let mut ready = String::new();
    BufReader::new(child.stdout.take().expect("child stdout"))
        .read_line(&mut ready)
        .expect("wait for the child to exec");
    assert_eq!(ready.trim(), "ready");
    drop(lock);

    let probe = open_lock(&lock_path).expect("reopen lock");
    let deadline = Instant::now() + TEST_BUSY_RETRY;
    loop {
        match probe.try_lock() {
            Ok(()) => break,
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(fs::TryLockError::WouldBlock) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("an executed child still holds the configuration lock");
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("cannot probe the configuration lock: {error}");
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The bounded wait must not hide a lock that is genuinely held elsewhere.
#[test]
fn a_genuinely_held_lock_is_reported_busy_after_the_retry_budget() {
    let root = tempfile::tempdir().expect("create fixture");
    let lock_path = root.path().join("settings.toml.lock");
    let file = File::new(root.path().join("settings.toml"));
    let lock = open_lock(&lock_path).expect("open lock");
    lock.try_lock().expect("hold lock");

    let started = Instant::now();
    let error = file
        .write(None, b"value")
        .expect_err("a held lock must stay busy");
    assert!(matches!(error, Error::Busy(_)));
    assert!(
        started.elapsed() >= super::LOCK_RETRY_BUDGET,
        "Busy must be reported only after the bounded retry"
    );
}
