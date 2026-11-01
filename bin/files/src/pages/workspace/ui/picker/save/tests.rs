use super::SaveTargets;
use std::{ffi::OsString, fs};

#[test]
fn batch_save_keeps_order_and_reserves_unique_names_without_touching_files() {
    use std::os::unix::ffi::OsStringExt as _;
    let root = tempfile::tempdir().expect("save folder");
    fs::write(root.path().join("report.txt"), b"keep").expect("existing file");
    let raw = OsString::from_vec(b"a\xff.png".to_vec());
    let names = [
        OsString::from("report.txt"),
        OsString::from("report.txt"),
        raw.clone(),
    ];
    let targets = SaveTargets::unique(root.path(), &names).expect("save targets");
    assert_eq!(
        targets,
        [
            root.path().join("report (1).txt"),
            root.path().join("report (2).txt"),
            root.path().join(raw)
        ]
    );
    assert_eq!(
        fs::read(root.path().join("report.txt")).expect("existing file"),
        b"keep"
    );
    assert!(targets.iter().all(|path| !path.exists()));
}
