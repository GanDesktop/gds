use super::*;
#[test]
fn kde_format_has_no_csv_escaping() {
    assert_eq!(
        decode("工作,, Project/design,\"a,b\"".as_bytes()).unwrap(),
        ["工作", " Project/design", "\"a", "b\""]
    );
    assert!(validate("a,b").is_err());
}
#[test]
#[cfg(target_os = "linux")]
fn associations_and_copy_preserve_other_tags() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, "a").unwrap();
    std::fs::write(&b, "b").unwrap();
    change(&a, "工作/设计", true).unwrap();
    change(&a, "keep", true).unwrap();
    change(&a, "工作/设计", false).unwrap();
    copy(&a, &b).unwrap();
    assert_eq!(read(&b).unwrap(), ["keep"]);
    write(&a, &[]).unwrap();
    assert!(read_raw(&a).unwrap().is_none());
}
#[test]
#[cfg(target_os = "linux")]
fn editor_delta_retains_unrelated_external_changes() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let original = vec!["old".into(), "keep".into()];
    write(file.path(), &original).unwrap();
    change(file.path(), "external", true).unwrap();
    apply_edit(file.path(), &original, &["keep".into(), "new".into()]).unwrap();
    assert_eq!(read(file.path()).unwrap(), ["keep", "external", "new"]);
}
