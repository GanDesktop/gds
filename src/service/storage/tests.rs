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
    let mut db = options(INITIAL).open(&path).await.expect("initialize");
    create!(Record {
        id: 1,
        value: "keep"
    })
    .exec(&mut db)
    .await
    .expect("saved record");
    let mut upgraded = options(UPGRADED).open(&path).await.expect("upgrade");
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
        .open(&path)
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
    let mut db = options(INITIAL).open(&path).await.expect("initial schema");
    const BROKEN: MigrationSet = MigrationSet::new(&[
        INITIAL_MIGRATION,
        MigrationFile::new(
            2,
            "002-broken.sql",
            "INSERT INTO records VALUES (1, 'rolled back');\n-- #[toasty::breakpoint]\nCREATE TABLE extra(value);\n-- #[toasty::breakpoint]\nINSERT INTO missing VALUES (1);",
        ),
    ]);
    assert!(options(BROKEN).open(&path).await.is_err());
    assert_eq!(Record::all().count().exec(&mut db).await.expect("rows"), 0);
    assert!(Extra::all().exec(&mut db).await.is_err());
    let db = options(UPGRADED)
        .open(&path)
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
            let mut db = options(INITIAL).open(path).await.expect("concurrent open");
            for value in 0..10 {
                let mut tx = db
                    .transaction_builder()
                    .mode(TransactionMode::Immediate)
                    .begin()
                    .await
                    .expect("writer");
                create!(Record {
                    id: id * 10_i64 + value,
                    value: value.to_string()
                })
                .exec(&mut tx)
                .await
                .expect("insert");
                tx.commit().await.expect("commit");
            }
        });
    }
    while let Some(worker) = workers.join_next().await {
        worker.expect("writer");
    }
    let mut db = options(INITIAL).open(&path).await.expect("read");
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
    assert!(options(BROKEN).open(&path).await.is_err());
    let mut db = options(INITIAL)
        .open(&path)
        .await
        .expect("retry initialization");
    create!(Record {
        id: 1,
        value: "saved"
    })
    .exec(&mut db)
    .await
    .expect("usable database");
    let mut reopened = options(INITIAL).open(&path).await.expect("reopen");
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
