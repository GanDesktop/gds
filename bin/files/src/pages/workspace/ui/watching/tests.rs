use super::{DEBOUNCE, subscribe};

#[test]
fn observes_external_create_write_rename_and_remove() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root = tempfile::tempdir().unwrap();
        let (sender, mut events) = tokio::sync::mpsc::channel(1);
        let _watcher = subscribe(root.path(), sender).unwrap();
        let path = root.path().join("external");
        let renamed = root.path().join("renamed");
        for operation in 0..4 {
            while events.try_recv().is_ok() {}
            match operation {
                0 => std::fs::write(&path, "created").unwrap(),
                1 => std::fs::write(&path, "modified").unwrap(),
                2 => std::fs::rename(&path, &renamed).unwrap(),
                _ => std::fs::remove_file(&renamed).unwrap(),
            }
            tokio::time::timeout(std::time::Duration::from_secs(3), events.recv())
                .await
                .expect("external change must wake the browser")
                .expect("watcher remains alive");
            tokio::time::sleep(DEBOUNCE).await;
        }
    });
}
