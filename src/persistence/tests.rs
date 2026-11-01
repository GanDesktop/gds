use super::*;

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
    let file = File::new(writable.join(".config/gds/files/settings.toml"));

    let created = file.write(None, b"first");
    let replaced = file.write(Some(b"first"), b"second");
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
