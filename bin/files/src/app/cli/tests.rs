use super::*;

#[test]
fn directory_picker_accepts_multiple_but_rejects_file_only_options() {
    let arguments =
        Arguments::try_parse_from(["gds-files", "--pick-directory", "--multiple", "/tmp"])
            .expect("parse directory picker");
    let options = arguments.picker().expect("picker options");
    assert!(options.directory);
    assert!(options.multiple);
    assert_eq!(arguments.directory, Some(PathBuf::from("/tmp")));
    for arguments in [
        vec!["gds-files", "--pick-file", "--pick-directory"],
        vec!["gds-files", "--multiple"],
        vec!["gds-files", "--pick-directory", "--filter", "*.txt"],
        vec!["gds-files", "--pick-directory", "--read-only"],
    ] {
        assert!(
            Arguments::try_parse_from(&arguments).is_err(),
            "{arguments:?}"
        );
    }
    let file = Arguments::try_parse_from(["gds-files", "--pick-file"])
        .expect("parse file picker")
        .picker()
        .expect("picker options");
    assert!(!file.directory);
    assert!(!file.multiple);
}

#[test]
fn logs_are_opt_in_and_compose_with_picker_filters() {
    assert!(
        !Arguments::try_parse_from(["gds-files"])
            .expect("default launch")
            .enable_logs
    );
    let args = Arguments::try_parse_from([
        "gds-files",
        "--enable-logs",
        "--pick-file",
        "--multiple",
        "--read-only",
        "--filter=Text:*.txt,*.md",
        "--filter",
        "Rust:*.rs",
        "/tmp",
    ])
    .expect("picker with logs and filters");
    assert!(args.enable_logs);
    let picker = args.picker().expect("picker mode");
    assert!(picker.multiple && picker.read_only);
    assert_eq!(picker.filters.len(), 2);
    assert!(picker.matches(0, std::path::Path::new("notes.md")));
    assert!(!picker.matches(1, std::path::Path::new("notes.md")));
    for arguments in [
        vec!["gds-files", "--filter", "*.txt"],
        vec!["gds-files", "--pick-file", "--filter"],
        vec!["gds-files", "--pick-file", "--filter", "["],
        vec!["gds-files", "/first", "/second"],
        vec!["gds-files", "--unknown"],
    ] {
        assert!(
            Arguments::try_parse_from(&arguments).is_err(),
            "{arguments:?}"
        );
    }
    let literal = Arguments::try_parse_from(["gds-files", "--", "--enable-logs"])
        .expect("directory after the separator");
    assert!(!literal.enable_logs);
    assert_eq!(literal.directory, Some(PathBuf::from("--enable-logs")));
}

#[cfg(unix)]
#[test]
fn non_utf8_directory_names_survive_clap_parsing() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let directory = OsString::from_vec(b"-raw-\xff".to_vec());
    let args = Arguments::try_parse_from([
        OsString::from("gds-files"),
        OsString::from("--enable-logs"),
        OsString::from("--"),
        directory.clone(),
    ])
    .expect("raw directory argument");
    assert!(args.enable_logs);
    assert_eq!(args.directory, Some(PathBuf::from(directory)));
}
