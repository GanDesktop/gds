use std::{ffi::OsString, fs, path::PathBuf};

use gds::service::desktop::protocol::chooser::{Choice, Mode, Request};

use super::{
    ActivatedPath, Filter, Options, SaveDecision, Session, inspect_activated_path,
    prepare_save_targets, targets,
};

#[test]
fn portal_result_assembles_paths_filter_read_only_and_choices() {
    let request = Request {
        app_id: String::new(),
        parent_window: String::new(),
        title: String::from("Choose"),
        accept_label: None,
        modal: true,
        current_folder: None,
        current_file: None,
        filters: Vec::new(),
        current_filter: None,
        choices: vec![Choice {
            id: String::from("encoding"),
            label: String::from("Encoding"),
            options: vec![(String::from("utf8"), String::from("UTF-8"))],
            selected: String::from("utf8"),
        }],
        mode: Mode::Open {
            multiple: false,
            directory: false,
        },
    };
    let options = Options::from_request(request).expect("build portal options");
    let mut session = Session::new(options);
    session.read_only = true;

    let path = PathBuf::from("/tmp/example.txt");
    let result = session.portal_result(std::slice::from_ref(&path));
    assert_eq!(result.paths, vec![OsString::from("/tmp/example.txt")]);
    assert_eq!(result.filter_index, 0);
    assert!(result.read_only);
    assert_eq!(
        result.choices,
        vec![(String::from("encoding"), String::from("utf8"))]
    );
}

#[test]
fn selection_check_validates_files_and_filters_off_ui_thread() {
    let temp = tempfile::tempdir().expect("create temp directory");
    let rust_file = temp.path().join("main.rs");
    let text_file = temp.path().join("notes.txt");
    fs::write(&rust_file, b"fn main() {}").expect("write rust file");
    fs::write(&text_file, b"notes").expect("write text file");

    let filter = Filter::from_spec("Rust:*.rs").expect("parse filter");
    let options = Options::new(vec![filter], false);
    let session = Session::new(options);
    let check = session.selection_check(Vec::new());

    assert_eq!(check.validate(&[rust_file]), Ok(()));
    let mismatch = check
        .validate(&[text_file])
        .expect_err("reject non-matching file");
    assert_eq!(mismatch.message_id, "picker-filter-mismatch");
}

#[test]
fn prepare_save_targets_detects_occupied_and_allocates_unique_names() {
    let temp = tempfile::tempdir().expect("create temp directory");
    let existing = temp.path().join("report.txt");
    fs::write(&existing, b"existing").expect("write existing file");

    let single_save = Mode::Save {
        name: OsString::from("report.txt"),
    };
    let decision = prepare_save_targets(&single_save, vec![existing.clone()])
        .expect("prepare single save target");
    assert_eq!(decision, SaveDecision::ConfirmOverwrite(vec![existing]));

    let multi_save = Mode::SaveFiles {
        names: vec![OsString::from("report.txt"), OsString::from("summary.txt")],
    };
    let multi_decision =
        prepare_save_targets(&multi_save, vec![temp.path().to_path_buf()]).expect("prepare multi");
    assert_eq!(
        multi_decision,
        SaveDecision::Ready(vec![
            temp.path().join("report (1).txt"),
            temp.path().join("summary.txt"),
        ])
    );
}

#[test]
fn inspect_activated_path_distinguishes_files_and_directories() {
    let temp = tempfile::tempdir().expect("create temp directory");
    let file = temp.path().join("item.txt");
    fs::write(&file, b"data").expect("write file");

    assert_eq!(
        inspect_activated_path(temp.path()),
        Ok(ActivatedPath::Directory)
    );
    assert_eq!(inspect_activated_path(&file), Ok(ActivatedPath::File));
    assert!(targets::occupied(&file).expect("check occupied"));
}
