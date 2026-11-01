use super::*;

#[tokio::test]
async fn completed_copies_never_replace_existing_entries() {
    let root = tempfile::tempdir().expect("fixture");
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    std::fs::create_dir(&source).expect("source");
    std::fs::create_dir(&destination).expect("destination");
    std::fs::write(source.join("报告.txt"), b"new data").expect("source file");
    std::fs::write(destination.join("报告.txt"), b"keep original").expect("occupied target");
    let (updates, _) = watch::channel(Progress::default());
    let report = copy(
        vec![Resource::Local(source.join("报告.txt"))],
        Resource::Local(destination.clone()),
        CancellationToken::new(),
        updates,
    )
    .await;
    assert!(report.completed.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert_eq!(
        std::fs::read(destination.join("报告.txt")).expect("original"),
        b"keep original"
    );
    assert_eq!(std::fs::read_dir(destination).expect("listing").count(), 1);
}

#[tokio::test]
async fn cancellation_and_unsupported_links_leave_no_published_root() {
    let root = tempfile::tempdir().expect("fixture");
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    std::fs::create_dir(&source).expect("source");
    std::fs::create_dir(&destination).expect("destination");
    std::fs::write(source.join("data"), b"payload").expect("data");
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (updates, _) = watch::channel(Progress::default());
    let report = copy(
        vec![Resource::Local(source.clone())],
        Resource::Local(destination.clone()),
        cancel,
        updates,
    )
    .await;
    assert!(report.cancelled);
    assert!(!destination.join("source").exists());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source.join("data"), source.join("link")).expect("link");
        let (updates, _) = watch::channel(Progress::default());
        let report = copy(
            vec![Resource::Local(source)],
            Resource::Local(destination.clone()),
            CancellationToken::new(),
            updates,
        )
        .await;
        assert_eq!(report.failures.len(), 1);
        assert_eq!(
            std::fs::read_dir(destination)
                .expect("clean staging")
                .count(),
            0
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn deletion_does_not_follow_directory_links() {
    let root = tempfile::tempdir().expect("fixture");
    let kept = root.path().join("kept");
    std::fs::create_dir(&kept).expect("kept");
    std::fs::write(kept.join("data"), b"keep").expect("file");
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&kept, &link).expect("link");
    let (updates, _) = watch::channel(Progress::default());
    let report = delete(
        vec![Resource::Local(link.clone())],
        CancellationToken::new(),
        updates,
    )
    .await;
    assert!(report.failures.is_empty());
    assert_eq!(report.completed.len(), 1);
    assert!(kept.join("data").exists());
    assert!(!link.exists());
}
