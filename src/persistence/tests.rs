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

#[cfg(unix)]
#[test]
fn configuration_under_a_symlinked_ancestor_or_parent_directory_can_be_created_and_replaced() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create fixture");
    let persist_ancestor = root.path().join("persist/config");
    let persist_direct = root.path().join("persist/direct");
    fs::create_dir_all(&persist_ancestor).expect("create persist ancestor");
    fs::create_dir_all(&persist_direct).expect("create persist direct");

    let home = root.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let config_link = home.join(".config");
    let direct_link = home.join("direct");
    symlink(&persist_ancestor, &config_link).expect("symlink ancestor dir");
    symlink(&persist_direct, &direct_link).expect("symlink parent dir");

    let nested = File::new(config_link.join("gds/app/files/settings.toml"));
    write_after_inherited_holders(&nested, None, b"first")
        .expect("create config under symlinked ancestor");
    write_after_inherited_holders(&nested, Some(b"first"), b"second")
        .expect("replace config under symlinked ancestor");
    assert_eq!(
        nested.read().expect("read nested").as_deref(),
        Some(b"second".as_slice())
    );
    assert_eq!(
        fs::read(persist_ancestor.join("gds/app/files/settings.toml"))
            .expect("read backing target"),
        b"second"
    );
    assert!(
        fs::symlink_metadata(&config_link)
            .expect("inspect ancestor link")
            .file_type()
            .is_symlink()
    );

    let direct = File::new(direct_link.join("settings.toml"));
    write_after_inherited_holders(&direct, None, b"one")
        .expect("create config under symlinked parent");
    write_after_inherited_holders(&direct, Some(b"one"), b"two")
        .expect("replace config under symlinked parent");
    assert_eq!(
        fs::read(persist_direct.join("settings.toml")).expect("read backing direct target"),
        b"two"
    );
}

#[cfg(unix)]
#[test]
fn leaf_symlinks_are_rejected_without_clobbering_or_creating_lock_files() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create fixture");
    let managed = root.path().join("managed.toml");
    fs::write(&managed, b"managed").expect("write managed target");

    let valid_path = root.path().join("settings.toml");
    symlink(&managed, &valid_path).expect("create valid leaf symlink");
    let valid_file = File::new(valid_path.clone());
    let valid_lock = root.path().join("settings.toml.lock");

    let replace_err = valid_file
        .write(Some(b"managed"), b"overwrite")
        .expect_err("valid leaf symlink must be rejected on replace");
    assert!(matches!(
        replace_err,
        Error::Symlink(ref err_path) if err_path.as_path() == valid_file.path()
    ));
    let create_err = valid_file
        .write(None, b"overwrite")
        .expect_err("valid leaf symlink must be rejected on create");
    assert!(matches!(
        create_err,
        Error::Symlink(ref err_path) if err_path.as_path() == valid_file.path()
    ));
    assert!(
        fs::symlink_metadata(&valid_path)
            .expect("valid symlink remains")
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&managed).expect("read managed"), b"managed");
    assert!(!valid_lock.exists());

    let missing_target = root.path().join("missing.toml");
    let dangling_path = root.path().join("dangling.toml");
    symlink(&missing_target, &dangling_path).expect("create dangling leaf symlink");
    let dangling_file = File::new(dangling_path.clone());
    let dangling_lock = root.path().join("dangling.toml.lock");

    let dangling_err = dangling_file
        .write(None, b"value")
        .expect_err("dangling leaf symlink must be rejected");
    assert!(matches!(
        dangling_err,
        Error::Symlink(ref err_path) if err_path.as_path() == dangling_file.path()
    ));
    assert!(
        fs::symlink_metadata(&dangling_path)
            .expect("dangling symlink remains")
            .file_type()
            .is_symlink()
    );
    assert!(!missing_target.exists());
    assert!(!dangling_lock.exists());
}

#[cfg(unix)]
#[test]
fn symlinked_lock_files_are_rejected() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create fixture");

    let lock_target = root.path().join("target.txt");
    fs::write(&lock_target, b"keep").expect("write lock target");
    let config_path = root.path().join("settings.toml");
    let lock_path = root.path().join("settings.toml.lock");
    symlink(&lock_target, &lock_path).expect("symlink lock file");

    let file = File::new(config_path.clone());
    let valid_err = file
        .write(None, b"value")
        .expect_err("valid .lock symlink must be rejected");
    assert!(matches!(
        valid_err,
        Error::Symlink(ref err_path) if err_path.as_path() == lock_path.as_path()
    ));
    assert_eq!(fs::read(&lock_target).expect("target untouched"), b"keep");
    assert!(!config_path.exists());

    let missing_target = root.path().join("missing-lock-target");
    let dangling_config = root.path().join("dangling.toml");
    let dangling_lock = root.path().join("dangling.toml.lock");
    symlink(&missing_target, &dangling_lock).expect("create dangling lock symlink");

    let dangling_err = File::new(dangling_config.clone())
        .write(None, b"value")
        .expect_err("dangling .lock symlink must be rejected");
    assert!(matches!(
        dangling_err,
        Error::Symlink(ref err_path) if err_path.as_path() == dangling_lock.as_path()
    ));
    assert!(!missing_target.exists());
    assert!(!dangling_config.exists());
}

#[cfg(unix)]
#[test]
fn dangling_ancestor_symlink_is_rejected_before_creating_directories() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create fixture");
    let home = root.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let missing_persist = root.path().join("missing-persist");
    let config_link = home.join(".config");
    symlink(&missing_persist, &config_link).expect("create dangling ancestor symlink");

    let file = File::new(config_link.join("gds/app/files/settings.toml"));
    let err = file
        .write(None, b"value")
        .expect_err("dangling ancestor symlink must be rejected");
    assert!(matches!(
        err,
        Error::Symlink(ref err_path) if err_path.as_path() == config_link.as_path()
    ));
    assert!(
        fs::symlink_metadata(&config_link)
            .expect("dangling ancestor remains")
            .file_type()
            .is_symlink()
    );
    assert!(!missing_persist.exists());
}

#[cfg(unix)]
#[test]
fn readonly_target_and_symlinked_readonly_directory_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = tempfile::tempdir().expect("create fixture");

    let readonly_file_path = root.path().join("readonly.toml");
    fs::write(&readonly_file_path, b"original").expect("seed readonly file");
    fs::set_permissions(&readonly_file_path, fs::Permissions::from_mode(0o444))
        .expect("mark target file readonly");
    let readonly_file = File::new(readonly_file_path.clone());
    let file_err = readonly_file
        .write(Some(b"original"), b"updated")
        .expect_err("readonly target file must be rejected");
    fs::set_permissions(&readonly_file_path, fs::Permissions::from_mode(0o644))
        .expect("restore target file permissions");
    assert!(matches!(
        file_err,
        Error::ReadOnly(ref err_path) if err_path.as_path() == readonly_file_path.as_path()
    ));
    assert_eq!(
        fs::read(&readonly_file_path).expect("read original"),
        b"original"
    );
    assert!(!root.path().join("readonly.toml.lock").exists());

    let store_ro = root.path().join("nix-store-config");
    fs::create_dir_all(&store_ro).expect("create readonly store dir");
    fs::set_permissions(&store_ro, fs::Permissions::from_mode(0o555))
        .expect("mark store dir readonly");
    let home = root.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let config_link = home.join(".config");
    symlink(&store_ro, &config_link).expect("symlink readonly store dir");

    let nested_err = File::new(config_link.join("gds/settings.toml"))
        .write(None, b"value")
        .expect_err("symlinked readonly ancestor must be rejected");
    let direct_err = File::new(config_link.join("settings.toml"))
        .write(None, b"value")
        .expect_err("symlinked readonly parent must be rejected");
    fs::set_permissions(&store_ro, fs::Permissions::from_mode(0o755))
        .expect("restore store dir permissions for cleanup");

    assert!(matches!(
        nested_err,
        Error::ReadOnly(ref err_path) if err_path.as_path() == config_link.as_path()
    ));
    assert!(matches!(
        direct_err,
        Error::ReadOnly(ref err_path) if err_path.as_path() == config_link.as_path()
    ));
    assert!(!store_ro.join("settings.toml.lock").exists());
    assert!(!store_ro.join("gds").exists());
}
