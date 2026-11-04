use super::*;
use std::os::unix::fs::symlink;

#[test]
fn saving_directory_access_preserves_special_bits_and_child_permissions() {
    let root = tempfile::tempdir().expect("temporary directory");
    let directory = root.path().join("shared");
    fs::create_dir(&directory).expect("create directory");
    let child = directory.join("child");
    fs::write(&child, "unchanged contents").expect("create child");
    fs::set_permissions(&child, fs::Permissions::from_mode(0o640)).expect("child permissions");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o1700))
        .expect("sticky directory permissions");

    let draft = read(&directory).expect("read permissions");
    // Opening the editor does not change the on-disk mode.
    assert_eq!(draft & 0o7777, 0o1700);
    save(&directory, 0o750).expect("save permissions");
    assert_eq!(read(&directory).expect("saved mode") & 0o7777, 0o1750);
    assert_eq!(read(&child).expect("child mode") & ACCESS_BITS, 0o640);
    assert_eq!(
        fs::read_to_string(&child).expect("child contents"),
        "unchanged contents"
    );
}

#[test]
fn saving_symlink_does_not_change_target_permissions() {
    let root = tempfile::tempdir().expect("temporary directory");
    let target = root.path().join("target");
    fs::write(&target, "content").expect("create target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).expect("target permissions");
    let link = root.path().join("link");
    symlink(&target, &link).expect("create symbolic link");

    assert!(matches!(save(&link, 0o777), Err(Error::Symlink)));
    assert_eq!(read(&target).expect("target mode") & ACCESS_BITS, 0o600);
    assert!(save(&root.path().join("missing"), 0o644).is_err());
}

#[test]
fn recursive_permissions_visit_children_before_removing_directory_access() {
    let root = tempfile::tempdir().expect("temporary directory");
    let directory = root.path().join("tree");
    let nested = directory.join("nested");
    fs::create_dir_all(&nested).expect("nested directories");
    let plain = nested.join("plain");
    let executable = nested.join("executable");
    fs::write(&plain, "plain").expect("plain file");
    fs::write(&executable, "executable").expect("executable file");
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).expect("plain permissions");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o751))
        .expect("executable permissions");
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o1700)).expect("sticky directory");
    let outside = root.path().join("outside");
    fs::write(&outside, "outside").expect("outside file");
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o644)).expect("outside permissions");
    symlink(&outside, nested.join("link")).expect("outside link");
    symlink(&directory, nested.join("loop")).expect("cycle link");

    let report = apply(&directory, 0o600, true);
    // Restore traversal so assertions and temporary-directory cleanup can reach children.
    let saved = read(&directory).expect("directory mode") & ACCESS_BITS;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .expect("restore directory access");
    let nested_saved = read(&nested).expect("nested mode") & 0o7777;
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o1700))
        .expect("restore nested access");

    assert_eq!(report.failures.len(), 0);
    assert_eq!(report.succeeded, 4);
    assert_eq!(saved, 0o600);
    assert_eq!(nested_saved, 0o1600);
    assert_eq!(read(&plain).expect("plain mode") & ACCESS_BITS, 0o600);
    assert_eq!(
        read(&executable).expect("executable mode") & ACCESS_BITS,
        0o711
    );
    assert_eq!(read(&outside).expect("outside mode") & ACCESS_BITS, 0o644);
}

#[test]
fn non_recursive_apply_leaves_children_unchanged_and_reports_missing_paths() {
    let root = tempfile::tempdir().expect("temporary directory");
    let child = root.path().join("child");
    fs::write(&child, "content").expect("child");
    fs::set_permissions(&child, fs::Permissions::from_mode(0o644)).expect("child permissions");

    let report = apply(root.path(), 0o700, false);
    assert_eq!(report.succeeded, 1);
    assert!(report.failures.is_empty());
    assert_eq!(read(&child).expect("child mode") & ACCESS_BITS, 0o644);

    let missing = root.path().join("missing");
    let report = apply(&missing, 0o700, true);
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].path, missing);
}

#[test]
fn recursive_apply_rejects_root_symlink_without_visiting_target() {
    let root = tempfile::tempdir().expect("temporary directory");
    let target = root.path().join("target");
    fs::create_dir(&target).expect("target directory");
    let child = target.join("child");
    fs::write(&child, "content").expect("child file");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).expect("target permissions");
    fs::set_permissions(&child, fs::Permissions::from_mode(0o644)).expect("child permissions");
    let link = root.path().join("link");
    symlink(&target, &link).expect("root symbolic link");

    let report = apply(&link, 0o700, true);
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failures.len(), 1);
    assert!(matches!(report.failures[0].error, Error::Symlink));
    assert_eq!(read(&target).expect("target mode") & ACCESS_BITS, 0o755);
    assert_eq!(read(&child).expect("child mode") & ACCESS_BITS, 0o644);
}
