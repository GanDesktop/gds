use super::*;

#[cfg(unix)]
#[test]
fn quoted_cd_preserves_spaces_quotes_unicode_and_shell_metacharacters() {
    let parent = tempfile::tempdir().expect("test directory");
    let directory = parent
        .path()
        .join("空 格 ' \" ; $(touch PWNED) `touch PWNED2` $HOME \\");
    std::fs::create_dir(&directory).expect("literal directory");
    let mut command = command(&directory).expect("quoted command");
    assert_eq!(command.pop(), Some(b'\r'));
    command.extend_from_slice(b" && pwd -P");
    let result = std::process::Command::new("sh")
        .args(["-c", std::str::from_utf8(&command).expect("UTF-8 command")])
        .current_dir(parent.path())
        .output()
        .expect("execute quoted command");
    assert!(result.status.success(), "{:?}", result.stderr);
    let expected = format!(
        "{}\n",
        directory
            .canonicalize()
            .expect("canonical directory")
            .display()
    );
    assert_eq!(result.stdout, expected.as_bytes());
    assert!(!parent.path().join("PWNED").exists());
    assert!(!parent.path().join("PWNED2").exists());
}

#[test]
fn refuses_missing_relative_and_terminal_control_paths() {
    let parent = tempfile::tempdir().expect("test directory");
    for path in [
        Path::new("."),
        Path::new(""),
        &parent.path().join("missing"),
    ] {
        assert!(command(path).is_err());
    }
    let control = parent.path().join("line\nbreak");
    std::fs::create_dir(&control).expect("control-character directory");
    assert!(
        command(&control).is_err(),
        "newline must not reach terminal input"
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let non_utf8 = parent.path().join(std::ffi::OsString::from_vec(vec![0xff]));
        std::fs::create_dir(&non_utf8).expect("non-UTF-8 directory");
        assert!(
            command(&non_utf8).is_err(),
            "do not use lossy path conversion"
        );
    }
}
