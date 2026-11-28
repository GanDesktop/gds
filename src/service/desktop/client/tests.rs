use super::*;

#[cfg(unix)]
#[test]
fn media_with_executable_permissions_opens_as_a_document() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("视频 with spaces.mp4");
    fs::write(&path, b"\0\0\0\x18ftypisom\0\0\0\0isommp42").expect("MP4 header");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable bits");
    assert_eq!(
        Desktop::document_path(&path, OpenMode::DefaultApplication).expect("open media"),
        fs::canonicalize(&path).expect("resolved path")
    );

    let link = directory.path().join("video-link");
    symlink(&path, &link).expect("media link");
    assert!(Desktop::document_path(&link, OpenMode::DefaultApplication).is_ok());

    // A media extension does not make an executable script a video.
    fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("script disguised as video");
    assert!(Desktop::document_path(&link, OpenMode::DefaultApplication).is_err());
    assert!(Desktop::document_path(&link, OpenMode::ChooseApplication).is_ok());
}

#[test]
fn launchers_require_a_chooser_and_non_files_are_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let launcher = directory.path().join("example.desktop");
    fs::write(&launcher, "[Desktop Entry]\nType=Application\nExec=false\n")
        .expect("desktop launcher");
    assert!(Desktop::document_path(&launcher, OpenMode::DefaultApplication).is_err());
    assert!(Desktop::document_path(&launcher, OpenMode::ChooseApplication).is_ok());
    assert!(Desktop::document_path(directory.path(), OpenMode::ChooseApplication).is_err());
    assert!(
        Desktop::document_path(
            &directory.path().join("missing"),
            OpenMode::DefaultApplication
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn executable_media_opens_one_rewound_descriptor_without_reopening_path() {
    use std::{
        io::{Read, Seek},
        os::unix::fs::PermissionsExt,
    };

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("clip.mp4");
    let mut payload = b"\0\0\0\x18ftypisom\0\0\0\0isommp42".to_vec();
    payload.resize(16 * 1024, b'x');
    fs::write(&path, &payload).expect("write MP4 payload");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable bits");

    let opened = Desktop::open_document(&path, OpenMode::DefaultApplication).expect("open media");
    assert_eq!(
        opened.path,
        fs::canonicalize(&path).expect("canonical path")
    );
    let mut file = opened.file;
    assert_eq!(
        file.stream_position().expect("descriptor offset"),
        0,
        "header sniffing must rewind the descriptor back to offset 0"
    );

    fs::remove_file(&path).expect("unlink path while open descriptor stays readable");
    let mut read_back = Vec::new();
    file.read_to_end(&mut read_back).expect("read full payload");
    assert_eq!(read_back, payload);
}

#[tokio::test]
async fn open_file_rejects_non_files_and_launchers_off_async_worker() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let launcher = directory.path().join("example.desktop");
    fs::write(&launcher, "[Desktop Entry]\nType=Application\nExec=false\n")
        .expect("desktop launcher");
    let desktop = Desktop::default();

    assert!(matches!(
        desktop
            .open_file(&launcher, OpenMode::DefaultApplication)
            .await,
        Err(Error::RequiresApplication(_))
    ));
    assert!(matches!(
        desktop
            .open_file(directory.path(), OpenMode::ChooseApplication)
            .await,
        Err(Error::NotFile(_))
    ));
    assert!(matches!(
        desktop
            .open_file(
                &directory.path().join("missing"),
                OpenMode::DefaultApplication
            )
            .await,
        Err(Error::Io { .. })
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let script = directory.path().join("run.sh");
        fs::write(&script, b"#!/bin/sh\nexit 0\n").expect("script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("executable bits");
        assert!(matches!(
            desktop
                .open_file(&script, OpenMode::DefaultApplication)
                .await,
            Err(Error::RequiresApplication(_))
        ));
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[test]
fn opened_document_does_not_retain_nonblock_flag() {
    use rustix::fs::{OFlags, fcntl_getfl};

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("note.txt");
    fs::write(&path, b"hello\n").expect("write document");

    for mode in [OpenMode::DefaultApplication, OpenMode::ChooseApplication] {
        let opened = Desktop::open_document(&path, mode).expect("open document");
        let flags = fcntl_getfl(&opened.file).expect("read descriptor status flags");
        assert!(
            !flags.contains(OFlags::NONBLOCK),
            "opened document descriptor must not retain O_NONBLOCK"
        );
        assert!(opened.writable, "writable document should open read-write");
    }

    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("make read-only");
    let readonly =
        Desktop::open_document(&path, OpenMode::DefaultApplication).expect("open read-only doc");
    assert!(
        !readonly.writable,
        "read-only document must not advertise writable portal access"
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("restore writable");
}

#[cfg(unix)]
#[test]
fn non_executable_documents_with_execute_bits_open_while_elf_and_scripts_are_gated() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory");
    let documents = [
        ("report.pdf", b"%PDF-1.7\n1 0 obj\n".as_slice()),
        ("notes.txt", b"hello from a FAT32 drive\n"),
        ("readme.md", b"# Readme\n"),
        (
            "diagram.svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
        ),
        ("sheet.docx", b"PK\x03\x04dummy-docx"),
        ("index.html", b"<!doctype html><p>hi</p>"),
    ];
    for (name, bytes) in documents {
        let path = directory.path().join(name);
        fs::write(&path, bytes).expect("write document");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod +x");
        assert!(
            Desktop::document_path(&path, OpenMode::DefaultApplication).is_ok(),
            "expected {name} with 0755 to open directly"
        );
    }

    let executables = [
        (
            "binary",
            b"\x7fELF\x02\x01\x01\x00\0\0\0\0\0\0\0\0".as_slice(),
        ),
        ("disguised.pdf", b"\x7fELF\x02\x01\x01\x00\0\0\0\0\0\0\0\0"),
        ("run.sh", b"#!/bin/sh\nexit 0\n"),
        ("no-shebang.sh", b"echo hello\n"),
        ("disguised.txt", b"#!/usr/bin/env sh\nexit 0\n"),
    ];
    for (name, bytes) in executables {
        let path = directory.path().join(name);
        fs::write(&path, bytes).expect("write executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod +x");
        assert!(
            matches!(
                Desktop::document_path(&path, OpenMode::DefaultApplication),
                Err(Error::RequiresApplication(_))
            ),
            "expected {name} with 0755 to require chooser"
        );
        assert!(
            Desktop::document_path(&path, OpenMode::ChooseApplication).is_ok(),
            "expected {name} to remain openable via explicit chooser"
        );
    }
}

#[test]
fn open_terminal_missing_launcher_returns_terminal_launch_error() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let desktop = Desktop::default();
    let missing = directory.path().join("missing-terminal-launcher");
    let expected_dir = fs::canonicalize(directory.path()).expect("canonical directory");

    let err = desktop
        .open_terminal_with_override(directory.path(), Some(missing.clone().into_os_string()))
        .expect_err("missing launcher must fail");
    assert!(matches!(
        err,
        Error::TerminalLaunch {
            program,
            directory: err_dir,
            source,
        } if program == missing.as_os_str()
            && err_dir == expected_dir
            && source.kind() == std::io::ErrorKind::NotFound
    ));

    assert!(matches!(
        desktop.open_terminal_with_override(directory.path(), Some(OsString::new())),
        Err(Error::EmptyTerminalCommand)
    ));

    let file_path = directory.path().join("not-a-dir");
    fs::write(&file_path, b"hi").expect("write file");
    assert!(matches!(
        desktop.open_terminal_with_override(&file_path, Some(OsString::from("sh"))),
        Err(Error::NotDirectory(_))
    ));
}

#[cfg(unix)]
#[test]
fn open_terminal_fast_failing_script_returns_terminal_exit_with_stderr() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory");
    let script = directory.path().join("fail.sh");
    fs::write(
        &script,
        b"#!/bin/sh\necho \"boom on stderr\" >&2\necho \"note on stdout\"\nexit 7\n",
    )
    .expect("write failing script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod script");

    let desktop = Desktop::default();
    let err = desktop
        .open_terminal_with_override(directory.path(), Some(script.into_os_string()))
        .expect_err("failing launcher should report TerminalExit");
    assert!(matches!(
        err,
        Error::TerminalExit {
            status,
            stderr,
            stdout,
            ..
        } if status.code() == Some(7)
            && stderr == "boom on stderr"
            && stdout == "note on stdout"
    ));
}

#[cfg(unix)]
#[test]
fn open_terminal_long_running_script_returns_promptly_without_hanging() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory");
    let script = directory.path().join("terminal.sh");
    fs::write(&script, b"#!/bin/sh\necho $$ > pid\nexec sleep 30\n")
        .expect("write long-running script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod script");

    let desktop = Desktop::default();
    let started = Instant::now();
    let result =
        desktop.open_terminal_with_override(directory.path(), Some(script.into_os_string()));
    let elapsed = started.elapsed();

    if let Ok(pid) = fs::read_to_string(directory.path().join("pid")) {
        let pid = pid.trim();
        if !pid.is_empty() {
            let _ = Command::new("kill").args(["-9", pid]).status();
        }
    }

    result.expect("long-running terminal should detach cleanly");
    assert!(
        elapsed < Duration::from_secs(2),
        "open_terminal hung for {elapsed:?}"
    );
}
