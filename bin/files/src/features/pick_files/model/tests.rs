use super::*;

#[test]
#[cfg(unix)]
fn directory_results_preserve_single_protocol_and_multiple_raw_paths() {
    use std::os::unix::ffi::OsStringExt as _;
    let paths = [
        PathBuf::from("/tmp/one folder"),
        PathBuf::from(OsString::from_vec(b"/tmp/\xff".to_vec())),
    ];
    let single: serde_json::Value = serde_json::from_slice(
        &selection_line(&paths[..1], false, 0).expect("serialize single directory"),
    )
    .expect("parse single directory");
    assert_eq!(single["path"], "/tmp/one folder");
    assert!(single.get("selections").is_none());
    let multiple: serde_json::Value =
        serde_json::from_slice(&selection_line(&paths, false, 0).expect("serialize directories"))
            .expect("parse directories");
    assert_eq!(multiple["selections"][0], single);
    assert!(multiple["selections"][1]["path"].is_null());
    assert_eq!(
        multiple["selections"][1]["path_os"]["Unix"],
        serde_json::json!(b"/tmp/\xff".to_vec())
    );
}

#[test]
fn portal_filters_treat_braces_literally_and_use_detected_mime_subclasses() {
    let filter = Filter::from_portal(&chooser::Filter {
        label: "Names".into(),
        patterns: vec![Pattern::Glob("a{b,c}:*.txt".into())],
    })
    .expect("literal glob");
    assert!(filter.is_match(Path::new("a{b,c}:file.txt")));
    assert!(!filter.is_match(Path::new("ab:file.txt")));
    let mime = Filter::from_portal(&chooser::Filter {
        label: "Images".into(),
        patterns: vec![Pattern::Mime("image/*".into())],
    })
    .expect("MIME filter");
    assert!(mime.is_match_type(Path::new("no extension"), Some("image/png")));
    assert!(!mime.is_match_type(Path::new("not an image.png"), Some("text/plain")));
    assert!(!Options::valid_name(std::ffi::OsStr::new("../outside")));
    assert!(!Options::valid_name(std::ffi::OsStr::new("folder/")));
}
