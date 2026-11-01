use super::*;
use std::{fs, time::Duration};

async fn changed(receiver: &mut tokio::sync::mpsc::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .expect("file change was not observed")
        .expect("live file watcher");
}

async fn drain_until_quiet(receiver: &mut tokio::sync::mpsc::Receiver<()>) {
    while let Ok(event) = tokio::time::timeout(Duration::from_millis(150), receiver.recv()).await {
        event.expect("live file watcher");
    }
}

#[tokio::test]
async fn ignores_unrelated_files_in_watched_directory() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("palette.json");
    fs::write(&target, "initial").unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let mut sources = Sources::new(sender);
    sources.set_paths(vec![target.clone()]).unwrap();
    fs::write(&target, "observed").unwrap();
    changed(&mut receiver).await;
    drain_until_quiet(&mut receiver).await;
    fs::write(directory.path().join("unrelated.json"), "ignored").unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(150), receiver.recv())
            .await
            .is_err(),
        "unrelated event was forwarded"
    );
}

#[tokio::test]
async fn watches_parent_symlink_replacement_and_new_target_changes() {
    parent_symlink_replacement(true).await;
}

#[tokio::test]
async fn watches_missing_file_after_parent_symlink_replacement() {
    parent_symlink_replacement(false).await;
}

async fn parent_symlink_replacement(file_exists: bool) {
    let directory = tempfile::tempdir().expect("managed directory sources");
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    if file_exists {
        for target in [&first, &second] {
            fs::write(target.join("palette.json"), "initial").unwrap();
        }
    }
    let managed = directory.path().join("managed");
    std::os::unix::fs::symlink(&first, &managed).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let mut sources = Sources::new(sender);
    sources
        .set_paths(vec![managed.join("palette.json")])
        .unwrap();

    let replacement = directory.path().join("replacement");
    std::os::unix::fs::symlink(&second, &replacement).unwrap();
    fs::rename(&replacement, &managed).unwrap();
    changed(&mut receiver).await;
    sources
        .refresh()
        .expect("follow replacement parent symlink");
    drain_until_quiet(&mut receiver).await;
    fs::write(second.join("palette.json"), "updated").unwrap();
    changed(&mut receiver).await;
}

#[tokio::test]
async fn watches_missing_sources_and_atomic_replacements_through_managed_links() {
    let directory = tempfile::tempdir().expect("theme sources");
    let path = directory.path().join("created/palette.json");
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let mut sources = Sources::new(sender);
    sources
        .set_paths(vec![path.clone()])
        .expect("watch missing source");
    fs::create_dir(path.parent().expect("source parent")).expect("create source directory");
    changed(&mut receiver).await;
    sources.refresh().expect("follow created directory");
    fs::write(&path, "first").expect("create palette");
    changed(&mut receiver).await;

    let managed = directory.path().join("managed.json");
    std::os::unix::fs::symlink(&path, &managed).expect("managed palette");
    sources
        .set_paths(vec![managed])
        .expect("watch link and resolved source");
    while receiver.try_recv().is_ok() {}
    let replacement = path.with_extension("new");
    fs::write(&replacement, "second").expect("new palette");
    fs::rename(replacement, &path).expect("atomic palette replacement");
    changed(&mut receiver).await;
}
