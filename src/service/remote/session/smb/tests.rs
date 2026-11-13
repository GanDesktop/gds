use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn stat_size_only_populates_regular_files() {
    assert_eq!(Client::stat_size(EntryKind::File, 4096), Some(4096));
    assert_eq!(Client::stat_size(EntryKind::File, 0), Some(0));
    assert_eq!(Client::stat_size(EntryKind::Directory, 4096), None);
    assert_eq!(Client::stat_size(EntryKind::SymbolicLink, 4096), None);
    assert_eq!(Client::stat_size(EntryKind::Other, 4096), None);
    assert_eq!(Client::kind(FileAttributes::new()), EntryKind::File);
    assert_eq!(
        Client::kind(FileAttributes::new().with_directory(true)),
        EntryKind::Directory
    );
}

#[test]
fn remove_args_enforce_entry_kind_and_preserve_reparse_flag() {
    let dir = Client::remove_args(EntryKind::Directory);
    assert!(dir.options.directory_file());
    assert!(!dir.options.non_directory_file());
    assert!(dir.options.open_reparse_point());

    for kind in [EntryKind::File, EntryKind::SymbolicLink, EntryKind::Other] {
        let non_dir = Client::remove_args(kind);
        assert!(!non_dir.options.directory_file());
        assert!(non_dir.options.non_directory_file());
        assert!(non_dir.options.open_reparse_point());
    }
}

#[tokio::test]
async fn with_share_retry_evicts_stale_share_and_reconnects_once() {
    let shares = Mutex::new(HashSet::from([String::from("docs")]));
    let connect_calls = AtomicUsize::new(0);
    let op_calls = AtomicUsize::new(0);

    let result = Client::with_share_retry(
        &shares,
        "docs",
        || async {
            connect_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        || async {
            if op_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(Error::protocol("SMB test", anyhow::anyhow!("tree dropped")))
            } else {
                Ok(42)
            }
        },
    )
    .await
    .expect("retry after reconnecting should succeed");

    assert_eq!(result, 42);
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(op_calls.load(Ordering::SeqCst), 2);
    assert!(shares.lock().await.contains("docs"));
}

#[tokio::test]
async fn with_share_retry_evicts_ipc_on_failed_reconnect_and_recovers_later() {
    let shares = Mutex::new(HashSet::from([String::from("IPC$")]));
    let connect_calls = AtomicUsize::new(0);
    let op_calls = AtomicUsize::new(0);

    let first = Client::with_share_retry(
        &shares,
        "IPC$",
        || async {
            connect_calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::protocol(
                "SMB share enumeration connection",
                anyhow::anyhow!("offline"),
            ))
        },
        || async {
            op_calls.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(Error::ShareList(anyhow::anyhow!("session closed")))
        },
    )
    .await;

    assert!(matches!(
        first,
        Err(Error::Protocol {
            operation: "SMB share enumeration connection",
            ..
        })
    ));
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(op_calls.load(Ordering::SeqCst), 1);
    assert!(!shares.lock().await.contains("IPC$"));

    let second = Client::with_share_retry(
        &shares,
        "IPC$",
        || async {
            connect_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        || async {
            op_calls.fetch_add(1, Ordering::SeqCst);
            Ok("ok")
        },
    )
    .await
    .expect("subsequent call reconnects cleanly");

    assert_eq!(second, "ok");
    assert_eq!(connect_calls.load(Ordering::SeqCst), 2);
    assert_eq!(op_calls.load(Ordering::SeqCst), 2);
    assert!(shares.lock().await.contains("IPC$"));
}

#[tokio::test]
async fn with_share_retry_skips_retry_for_fresh_connection_and_caps_at_one_retry() {
    let shares = Mutex::new(HashSet::new());
    let connect_calls = AtomicUsize::new(0);
    let op_calls = AtomicUsize::new(0);

    let fresh_error = Client::with_share_retry(
        &shares,
        "docs",
        || async {
            connect_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        || async {
            op_calls.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(Error::protocol(
                "SMB metadata open",
                anyhow::anyhow!("not found"),
            ))
        },
    )
    .await;
    assert!(fresh_error.is_err());
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(op_calls.load(Ordering::SeqCst), 1);
    assert!(shares.lock().await.contains("docs"));

    let cached_error = Client::with_share_retry(
        &shares,
        "docs",
        || async {
            connect_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        || async {
            op_calls.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(Error::protocol(
                "SMB metadata open",
                anyhow::anyhow!("not found"),
            ))
        },
    )
    .await;
    assert!(cached_error.is_err());
    assert_eq!(connect_calls.load(Ordering::SeqCst), 2);
    assert_eq!(op_calls.load(Ordering::SeqCst), 3);
}
