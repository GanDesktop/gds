use super::*;
use std::os::unix::ffi::OsStrExt as _;

fn value(value: impl Into<Value<'static>>) -> OwnedValue {
    value.into().try_to_owned().expect("test variant")
}

#[test]
fn filters_choices_and_native_paths_cross_process_boundary_losslessly() {
    let raw = b"/tmp/a\xff\0".to_vec();
    let options = Options::from([
        (
            "filters".into(),
            value(vec![(
                "MIME: types".to_owned(),
                vec![(1u32, "image/*".to_owned()), (0, "a,b:{c}".to_owned())],
            )]),
        ),
        (
            "choices".into(),
            value(vec![(
                "id".to_owned(),
                "Choice".to_owned(),
                Vec::<(String, String)>::new(),
                "true".to_owned(),
            )]),
        ),
        ("current_folder".into(), value(raw)),
    ]);
    let picker = Picker::from_invocation("OpenFile", "test.app", "x11:abcd", "Pick", &options)
        .expect("native request");
    let encoded = serde_json::to_vec(&picker.request).expect("request JSON");
    let decoded: Request = serde_json::from_slice(&encoded).expect("request");
    assert!(decoded.modal, "modal defaults to true");
    assert_eq!(
        decoded.current_folder.expect("folder").as_bytes(),
        b"/tmp/a\xff"
    );
    assert_eq!(
        decoded.filters[0].patterns[0],
        Pattern::Mime("image/*".into())
    );
    assert_eq!(
        decoded.filters[0].patterns[1],
        Pattern::Glob("a,b:{c}".into())
    );
    assert_eq!(decoded.choices[0].selected, "true");
}

#[test]
fn open_and_save_results_validate_paths_counts_and_filter_identity() {
    let root = tempfile::tempdir().expect("fixture");
    let path = root
        .path()
        .join(OsString::from_vec(b"a #\xff.txt".to_vec()));
    std::fs::write(&path, []).expect("selected file");
    let current: Filter = ("Text".into(), vec![(0, "*.txt".into())]);
    let options = Options::from([("current_filter".into(), value(current.clone()))]);
    let picker = Picker::from_invocation("OpenFile", "", "", "", &options).expect("picker");
    let mut payload = chooser::Result {
        paths: vec![path.into_os_string()],
        read_only: true,
        filter_index: 0,
        choices: Vec::new(),
    };
    let result = picker
        .results(&serde_json::to_vec(&payload).expect("result"))
        .expect("valid result");
    let uris = Vec::<String>::try_from(result["uris"].try_clone().expect("uris")).expect("uris");
    assert!(uris[0].ends_with("/a%20%23%FF.txt"));
    assert_eq!(
        Filter::try_from(result["current_filter"].try_clone().expect("filter")).expect("filter"),
        current
    );
    payload.paths = vec![root.path().join("new file").into_os_string()];
    assert!(matches!(
        picker.results(&serde_json::to_vec(&payload).expect("result")),
        Err(Error::InvalidPath(_))
    ));
    let save = Picker::from_invocation("SaveFile", "", "", "Save", &options).expect("save picker");
    assert!(
        save.results(&serde_json::to_vec(&payload).expect("result"))
            .is_ok()
    );
    payload.paths.clear();
    assert!(matches!(
        save.results(&serde_json::to_vec(&payload).expect("result")),
        Err(Error::SelectionCount)
    ));
}
