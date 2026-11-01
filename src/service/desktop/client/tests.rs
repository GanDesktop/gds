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
