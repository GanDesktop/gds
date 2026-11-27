use std::{cell::Cell, rc::Rc};

use tokio_util::sync::CancellationToken;

use super::{BackgroundTask, BackgroundTasks, FILES_TASK_KEY, TRASH_TASK_KEY, remote_task_key};

#[test]
fn concurrent_tasks_preserve_insertion_order_and_cancel_independently() {
    let mut tasks = BackgroundTasks::default();
    let files_token = CancellationToken::new();
    let trash_token = CancellationToken::new();
    let remote_token = CancellationToken::new();
    let remote_key = remote_task_key(7);
    let callback_hits = Rc::new(Cell::new(0usize));
    let hits_ref = Rc::clone(&callback_hits);

    assert!(tasks.begin(FILES_TASK_KEY, "Copying 4 items", files_token.clone()));
    assert!(!tasks.begin(FILES_TASK_KEY, "Duplicate copy", CancellationToken::new()));
    assert!(tasks.begin(TRASH_TASK_KEY, "Emptying Trash", trash_token.clone()));
    tasks.upsert(
        BackgroundTask::new(remote_key.clone(), "sftp://host/srv", remote_token.clone())
            .with_detail("archive.tar.zst")
            .with_progress(0.4)
            .with_on_cancel(move || hits_ref.set(hits_ref.get() + 1)),
    );

    assert_eq!(tasks.len(), 3);
    assert_eq!(tasks.items()[0].key, FILES_TASK_KEY);
    assert_eq!(tasks.items()[1].key, TRASH_TASK_KEY);
    assert_eq!(tasks.items()[2].key, remote_key);
    assert_eq!(
        tasks.items()[2].display_label(),
        "sftp://host/srv — archive.tar.zst"
    );
    assert_eq!(tasks.items()[2].progress_percent(), Some(40));

    assert!(tasks.update_progress(
        TRASH_TASK_KEY,
        Some("Purging Trash".to_owned()),
        Some("notes.txt".to_owned()),
        Some(0.75),
    ));
    assert_eq!(
        tasks.items()[1].display_label(),
        "Purging Trash — notes.txt"
    );
    assert_eq!(tasks.items()[1].progress_percent(), Some(75));

    assert!(tasks.cancel(&remote_key));
    assert!(tasks.cancel(&remote_key));
    assert_eq!(callback_hits.get(), 1);
    assert!(remote_token.is_cancelled());
    assert!(!files_token.is_cancelled());
    assert!(!trash_token.is_cancelled());

    let finished = tasks.finish(TRASH_TASK_KEY).expect("trash task existed");
    assert_eq!(finished.key, TRASH_TASK_KEY);
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks.items()[0].key, FILES_TASK_KEY);
    assert_eq!(tasks.items()[1].key, remote_key);

    tasks.sync(remote_key.clone(), None);
    assert_eq!(tasks.len(), 1);
    tasks.cancel_all();
    assert!(files_token.is_cancelled());
}
