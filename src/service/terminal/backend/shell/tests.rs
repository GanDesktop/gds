use std::{io::Read, path::PathBuf};

use super::{Command, Error, SpawnOptions, default_shell, login_argument, resolve_shell};

#[cfg(target_os = "linux")]
#[test]
fn renamed_cwd_remains_pinned_instead_of_falling_back_to_home() {
    let directory = tempfile::tempdir().expect("test directory");
    let original = directory.path().join("original");
    let moved = directory.path().join("moved");
    std::fs::create_dir(&original).expect("cwd fixture");
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("sh"))
        .chain([PathBuf::from("/bin/sh")])
        .find(|path| path.is_absolute() && path.is_file())
        .expect("POSIX sh");
    let command = Command::prepare(&SpawnOptions {
        cwd: original.clone(),
        shell: Some(shell),
        args: vec!["-c".into(), "pwd -P".into()],
        ..SpawnOptions::default()
    })
    .expect("pin cwd");
    std::fs::rename(&original, &moved).expect("rename between validation and spawn");
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize::default())
        .expect("PTY");
    let mut reader = pair.master.try_clone_reader().expect("PTY reader");
    let mut child = command
        .spawn(&pair.master.tty_name().expect("Unix slave path"))
        .expect("spawn into pinned directory");
    drop(pair.slave);
    let mut output = String::new();
    reader
        .read_to_string(&mut output)
        .expect("read final output");
    assert!(child.wait().expect("reap").success());
    assert_eq!(output.trim(), moved.to_str().expect("UTF-8 fixture"));
}

fn executable_fixture(directory: &std::path::Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join(name);
    // Selection only inspects the file; these fixtures are never executed.
    std::fs::write(&path, b"#!/fixture-not-executed\n").expect("shell fixture");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .expect("executable shell fixture");
    path
}

#[test]
fn default_shell_prefers_account_over_development_environment() {
    let directory = tempfile::tempdir().expect("test directory");
    let account = executable_fixture(directory.path(), "user-interactive-shell");
    let environment = executable_fixture(directory.path(), "development-build-shell");
    assert_eq!(
        default_shell(Some(&account), Some(&environment)).expect("account shell"),
        account,
    );

    let missing = directory.path().join("unavailable-account-shell");
    assert_eq!(
        default_shell(Some(&missing), Some(&environment)).expect("environment fallback"),
        environment,
    );
    assert_eq!(
        default_shell(None, Some(&environment)).expect("no account entry"),
        environment,
    );
    assert!(matches!(
        default_shell(None, Some(&missing)),
        Err(Error::ShellUnavailable),
    ));
}

#[test]
fn explicit_shell_is_respected_and_invalid_configuration_does_not_fall_back() {
    let directory = tempfile::tempdir().expect("test directory");
    let configured = executable_fixture(directory.path(), "explicit-shell");
    assert_eq!(
        resolve_shell(Some(&configured)).expect("explicit shell"),
        configured,
    );
    let missing = directory.path().join("missing-explicit-shell");
    assert!(matches!(
        resolve_shell(Some(&missing)),
        Err(Error::InvalidShell(path)) if path == missing,
    ));
}

#[test]
fn login_arguments_are_shell_specific_and_unknown_shells_are_rejected() {
    assert_eq!(
        login_argument(std::path::Path::new("/usr/bin/fish")).expect("fish"),
        "--login"
    );
    assert_eq!(
        login_argument(std::path::Path::new("/usr/bin/dash")).expect("dash"),
        "-l"
    );
    assert!(matches!(
        login_argument(std::path::Path::new("/custom/shell")),
        Err(Error::UnsupportedLogin(_))
    ));
}
