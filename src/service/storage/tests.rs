use super::*;
use toasty::{Model, create, models};

#[derive(Debug, Model)]
#[table = "records"]
struct Record {
    #[key]
    id: i64,
    value: String,
}

#[derive(Debug, Model)]
#[table = "extra"]
struct Extra {
    #[key]
    value: i64,
}

const INITIAL_MIGRATION: MigrationFile = MigrationFile::new(
    1,
    "001-records.sql",
    "CREATE TABLE records (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
);
const INITIAL: MigrationSet = MigrationSet::new(&[INITIAL_MIGRATION]);
const UPGRADED: MigrationSet = MigrationSet::new(&[
    INITIAL_MIGRATION,
    MigrationFile::new(
        2,
        "002-pinned.sql",
        "ALTER TABLE records ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;",
    ),
]);

fn options(migrations: MigrationSet) -> Options {
    Options::new(migrations).models(models!(Record, Extra))
}

#[tokio::test]
async fn native_migrations_upgrade_once_and_preserve_records() {
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("app.sqlite");
    let mut db = options(INITIAL)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("initialize");
    create!(Record {
        id: 1,
        value: "keep"
    })
    .exec(&mut db)
    .await
    .expect("saved record");
    let mut upgraded = options(UPGRADED)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("upgrade");
    let row = Record::filter_by_id(1)
        .one()
        .exec(&mut upgraded)
        .await
        .expect("row");
    assert_eq!(row.value, "keep");
    assert_eq!(
        UPGRADED
            .apply(&upgraded)
            .await
            .expect("native tracking")
            .skipped(),
        2
    );
    let mut reopened = options(UPGRADED)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("skip completed migrations");
    assert_eq!(
        Record::all()
            .count()
            .exec(&mut reopened)
            .await
            .expect("saved rows"),
        1
    );
}

#[tokio::test]
async fn failed_migration_rolls_back_rows_schema_and_tracking() {
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("app.sqlite");
    let mut db = options(INITIAL)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("initial schema");
    const BROKEN: MigrationSet = MigrationSet::new(&[
        INITIAL_MIGRATION,
        MigrationFile::new(
            2,
            "002-broken.sql",
            "INSERT INTO records VALUES (1, 'rolled back');\n-- #[toasty::breakpoint]\nCREATE TABLE extra(value);\n-- #[toasty::breakpoint]\nINSERT INTO missing VALUES (1);",
        ),
    ]);
    assert!(
        options(BROKEN)
            .open(
                &gds::service::task_manager::Scope::with_handle(
                    tokio::runtime::Handle::current(),
                    false
                ),
                &path
            )
            .await
            .is_err()
    );
    assert_eq!(Record::all().count().exec(&mut db).await.expect("rows"), 0);
    assert!(Extra::all().exec(&mut db).await.is_err());
    let db = options(UPGRADED)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("failed migration was not recorded");
    assert_eq!(
        UPGRADED
            .apply(&db)
            .await
            .expect("native tracking")
            .skipped(),
        2
    );
}

/// A worker that has finished opening the database can start writing while
/// another worker is still applying migrations inside `Options::open`. SQLite
/// answers that lock conflict with an immediate `SQLITE_BUSY` instead of
/// invoking the busy handler (waiting could deadlock against the migrating
/// connection), so `PRAGMA busy_timeout` cannot cover it. Retry the whole
/// transaction for a bounded period, as an application client has to; the
/// final row count still proves that no update was lost.
const BUSY_RETRY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const BUSY_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

fn is_database_locked(error: &toasty::Error) -> bool {
    let message = error.to_string();
    message.contains("database is locked") || message.contains("database table is locked")
}

async fn write_record(db: &mut Database, id: i64, value: String) {
    let deadline = std::time::Instant::now() + BUSY_RETRY_TIMEOUT;
    loop {
        let attempt = async {
            let mut tx = db
                .transaction_builder()
                .mode(TransactionMode::Immediate)
                .begin()
                .await?;
            if let Err(error) = create!(Record {
                id,
                value: value.clone()
            })
            .exec(&mut tx)
            .await
            {
                tx.rollback().await?;
                return Err(error);
            }
            tx.commit().await
        }
        .await;
        match attempt {
            Ok(()) => return,
            Err(error) if is_database_locked(&error) && std::time::Instant::now() < deadline => {
                tokio::time::sleep(BUSY_RETRY_INTERVAL).await;
            }
            Err(error) => panic!("writer: {error}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_open_and_writers_serialize_without_lost_updates() {
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("app.sqlite");
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(4));
    let mut workers = tokio::task::JoinSet::new();
    for id in 0..4 {
        let path = path.clone();
        let barrier = barrier.clone();
        workers.spawn(async move {
            barrier.wait().await;
            let mut db = options(INITIAL)
                .open(
                    &gds::service::task_manager::Scope::with_handle(
                        tokio::runtime::Handle::current(),
                        false,
                    ),
                    path,
                )
                .await
                .expect("concurrent open");
            for value in 0..10 {
                write_record(&mut db, id * 10_i64 + value, value.to_string()).await;
            }
        });
    }
    while let Some(worker) = workers.join_next().await {
        worker.expect("writer");
    }
    let mut db = options(INITIAL)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("read");
    assert_eq!(
        Record::all().count().exec(&mut db).await.expect("count"),
        40
    );
}

#[tokio::test]
async fn failed_first_migration_can_be_retried_without_resetting_the_database() {
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("app.sqlite");
    const BROKEN: MigrationSet = MigrationSet::new(&[MigrationFile::new(
        1,
        "001-records.sql",
        "CREATE TABLE records (id INTEGER PRIMARY KEY, value TEXT NOT NULL);\n-- #[toasty::breakpoint]\nINSERT INTO missing VALUES (1);",
    )]);
    assert!(
        options(BROKEN)
            .open(
                &gds::service::task_manager::Scope::with_handle(
                    tokio::runtime::Handle::current(),
                    false
                ),
                &path
            )
            .await
            .is_err()
    );
    let mut db = options(INITIAL)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("retry initialization");
    create!(Record {
        id: 1,
        value: "saved"
    })
    .exec(&mut db)
    .await
    .expect("usable database");
    let mut reopened = options(INITIAL)
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .expect("reopen");
    assert_eq!(
        Record::all()
            .count()
            .exec(&mut reopened)
            .await
            .expect("saved row"),
        1
    );
    assert_eq!(
        INITIAL
            .apply(&reopened)
            .await
            .expect("recorded once")
            .skipped(),
        1
    );
}

#[cfg(unix)]
fn test_scope() -> gds::service::task_manager::Scope {
    gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false)
}

#[cfg(unix)]
#[tokio::test]
async fn opening_and_migrating_under_a_symlinked_ancestor_directory_succeeds() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = tempfile::tempdir().expect("fixture");
    let persist = root.path().join("persist");
    std::fs::create_dir_all(&persist).expect("create backing dir");
    let state_link = root.path().join("state");
    symlink(&persist, &state_link).expect("symlink ancestor dir");

    let nested_db = state_link.join("gds/app/state.sqlite");
    let mut initial = options(INITIAL);
    initial.private_directory = true;
    let mut db = initial
        .open(&test_scope(), &nested_db)
        .await
        .expect("open nested db through symlinked ancestor");
    create!(Record {
        id: 1,
        value: "persisted"
    })
    .exec(&mut db)
    .await
    .expect("insert record");
    drop(db);

    let mut upgraded_opts = options(UPGRADED);
    upgraded_opts.private_directory = true;
    let mut upgraded = upgraded_opts
        .open(&test_scope(), &nested_db)
        .await
        .expect("migrate nested db through symlinked ancestor");
    let row = Record::filter_by_id(1)
        .one()
        .exec(&mut upgraded)
        .await
        .expect("read persisted record");
    assert_eq!(row.value, "persisted");

    let parent_meta =
        std::fs::symlink_metadata(nested_db.parent().expect("parent dir")).expect("inspect parent");
    assert!(!parent_meta.file_type().is_symlink());
    assert!(parent_meta.is_dir());
    assert_eq!(parent_meta.permissions().mode() & 0o077, 0);
    assert!(persist.join("gds/app/state.sqlite").exists());

    let direct_db = state_link.join("direct/state.sqlite");
    let _direct = options(INITIAL)
        .open(&test_scope(), &direct_db)
        .await
        .expect("open db one level under symlinked ancestor");
    assert!(persist.join("direct/state.sqlite").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_dangling_and_non_directory_ancestor_symlinks() {
    use std::io::ErrorKind;
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("fixture");
    let scope = test_scope();

    let missing = root.path().join("missing");
    let dangling = root.path().join("dangling");
    symlink(&missing, &dangling).expect("create dangling ancestor symlink");
    let dangling_err = match options(INITIAL)
        .open(&scope, dangling.join("app/state.sqlite"))
        .await
    {
        Ok(_) => panic!("dangling ancestor symlink must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        dangling_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
    ));
    assert!(!missing.exists());

    let regular = root.path().join("regular");
    std::fs::write(&regular, b"not-a-dir").expect("create regular file");
    let file_link = root.path().join("file-link");
    symlink(&regular, &file_link).expect("symlink to regular file");

    for candidate in [
        file_link.join("app/state.sqlite"),
        file_link.join("sub/app/state.sqlite"),
    ] {
        let err = match options(INITIAL).open(&scope, candidate).await {
            Ok(_) => panic!("non-directory ancestor symlink must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            err,
            Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
        ));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinked_leaf_directory_database_sidecars_and_lock() {
    use std::io::ErrorKind;
    use std::os::fd::AsFd;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;

    let root = tempfile::tempdir().expect("fixture");
    let scope = test_scope();

    let real_dir = root.path().join("real_dir");
    std::fs::create_dir_all(&real_dir).expect("create private leaf dir");
    std::fs::set_permissions(&real_dir, std::fs::Permissions::from_mode(0o700))
        .expect("chmod private leaf dir");

    let linked_dir = root.path().join("linked_dir");
    symlink(&real_dir, &linked_dir).expect("symlink leaf dir");
    let dir_err = match options(INITIAL)
        .open(&scope, linked_dir.join("state.sqlite"))
        .await
    {
        Ok(_) => panic!("symlinked leaf parent directory must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        dir_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
    ));
    assert!(!real_dir.join("state.sqlite").exists());

    let target_db = root.path().join("target.sqlite");
    std::fs::write(&target_db, b"keep").expect("write target db");
    std::fs::set_permissions(&target_db, std::fs::Permissions::from_mode(0o600))
        .expect("chmod target db");
    let linked_db = real_dir.join("leaf.sqlite");
    symlink(&target_db, &linked_db).expect("symlink db file");
    let db_err = match options(INITIAL).open(&scope, &linked_db).await {
        Ok(_) => panic!("symlinked db file must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        db_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
    ));
    assert_eq!(std::fs::read(&target_db).expect("read target db"), b"keep");

    let missing_db = root.path().join("missing.sqlite");
    let dangling_db = real_dir.join("dangling.sqlite");
    symlink(&missing_db, &dangling_db).expect("create dangling db symlink");
    let dangling_db_err = match options(INITIAL).open(&scope, &dangling_db).await {
        Ok(_) => panic!("dangling db symlink must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        dangling_db_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
    ));
    assert!(!missing_db.exists());

    for (idx, suffix) in ["-journal", "-wal", "-shm"].iter().enumerate() {
        let db_path = real_dir.join(format!("sidecar_{idx}.sqlite"));
        let mut sidecar_name = db_path.as_os_str().to_owned();
        sidecar_name.push(suffix);
        let sidecar_path = PathBuf::from(sidecar_name);
        let sidecar_target = root.path().join(format!("sidecar_target_{idx}"));
        std::fs::write(&sidecar_target, b"keep").expect("write sidecar target");
        std::fs::set_permissions(&sidecar_target, std::fs::Permissions::from_mode(0o600))
            .expect("chmod sidecar target");
        symlink(&sidecar_target, &sidecar_path).expect("symlink sidecar");

        let sidecar_err = match options(INITIAL).open(&scope, &db_path).await {
            Ok(_) => panic!("symlinked {suffix} sidecar must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            sidecar_err,
            Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
        ));
        assert!(!db_path.exists());
    }

    let lock_target = root.path().join("lock_target");
    std::fs::write(&lock_target, b"keep").expect("write lock target");
    std::fs::set_permissions(&lock_target, std::fs::Permissions::from_mode(0o600))
        .expect("chmod lock target");
    let locked_db = real_dir.join("locked.sqlite");
    symlink(&lock_target, real_dir.join("locked.sqlite.lock")).expect("symlink valid lock");
    let valid_lock_err = match options(INITIAL).open(&scope, &locked_db).await {
        Ok(_) => panic!("symlinked lock file must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        valid_lock_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
    ));
    assert_eq!(
        std::fs::read(&lock_target).expect("lock target untouched"),
        b"keep"
    );

    let missing_lock = root.path().join("missing_lock");
    let dangling_locked_db = real_dir.join("dangling_locked.sqlite");
    symlink(&missing_lock, real_dir.join("dangling_locked.sqlite.lock"))
        .expect("symlink dangling lock");
    let dangling_lock_err = match options(INITIAL).open(&scope, &dangling_locked_db).await {
        Ok(_) => panic!("dangling lock symlink must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        dangling_lock_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::PermissionDenied
    ));
    assert!(!missing_lock.exists());

    let lock_file = paths::lock(&scope, &real_dir.join("cloexec.sqlite"))
        .await
        .expect("acquire lock file");
    let flags = nix::fcntl::fcntl(lock_file.as_fd(), nix::fcntl::FcntlArg::F_GETFD)
        .expect("read lock fd flags");
    assert!(nix::fcntl::FdFlag::from_bits_truncate(flags).contains(nix::fcntl::FdFlag::FD_CLOEXEC));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_and_open_cancel_promptly_when_scope_is_cancelled() {
    use std::io::ErrorKind;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().expect("fixture");
    let db_path = root.path().join("locked.sqlite");
    let holder_scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    paths::prepare(&db_path, false)
        .await
        .expect("prepare database path");
    let held_lock = paths::lock(&holder_scope, &db_path)
        .await
        .expect("acquire holder lock");

    let lock_scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let wait_scope = lock_scope.clone();
    let lock_path = db_path.clone();
    let lock_task = tokio::spawn(async move { paths::lock(&wait_scope, &lock_path).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let started = Instant::now();
    lock_scope.shutdown().await;
    let lock_err = match lock_task.await.expect("join lock task") {
        Ok(_) => panic!("cancelled paths::lock must fail"),
        Err(error) => error,
    };
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(matches!(
        lock_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::Interrupted
    ));

    let open_scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let open_wait_scope = open_scope.clone();
    let open_path = db_path.clone();
    let open_task =
        tokio::spawn(async move { options(INITIAL).open(&open_wait_scope, &open_path).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let started = Instant::now();
    open_scope.shutdown().await;
    let open_err = match open_task.await.expect("join open task") {
        Ok(_) => panic!("cancelled Options::open must fail"),
        Err(error) => error,
    };
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(matches!(
        open_err,
        Error::Io(ref io_err) if io_err.kind() == ErrorKind::Interrupted
    ));

    let lock_file_path = {
        let mut name = db_path.as_os_str().to_owned();
        name.push(".lock");
        std::path::PathBuf::from(name)
    };
    let contender = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_file_path)
        .expect("open lock contender");
    let timeout_err = paths::acquire_lock(contender, Duration::from_millis(40), || false)
        .expect_err("held lock should time out");
    assert_eq!(timeout_err.kind(), ErrorKind::TimedOut);

    drop(held_lock);
    let _db = options(INITIAL)
        .open(&holder_scope, &db_path)
        .await
        .expect("open succeeds once lock is released");
}
