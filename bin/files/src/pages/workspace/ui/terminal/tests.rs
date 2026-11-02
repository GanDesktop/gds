use super::launch_cwd;
use std::path::Path;

#[test]
fn terminal_launch_uses_real_available_directory_or_home() {
    let home = tempfile::tempdir().expect("home fixture");
    let local = tempfile::tempdir().expect("local fixture");
    for path in [
        "",
        ".",
        "smb://host/share",
        "trash://",
        "recent://",
        "~/project",
    ] {
        assert_eq!(
            launch_cwd(Some(Path::new(path)), true, home.path()),
            home.path()
        );
    }
    assert_eq!(launch_cwd(None, true, home.path()), home.path());
    assert_eq!(
        launch_cwd(Some(local.path()), false, home.path()),
        home.path()
    );
    assert_eq!(
        launch_cwd(Some(local.path()), true, home.path()),
        local.path()
    );
    let missing = local.path().join("missing");
    assert_eq!(launch_cwd(Some(&missing), true, home.path()), home.path());
    let file = local.path().join("file");
    std::fs::write(&file, "fixture").expect("write file fixture");
    assert_eq!(launch_cwd(Some(&file), true, home.path()), home.path());
}

#[cfg(feature = "gui-tests")]
mod motion;

#[cfg(all(feature = "gui-tests", unix))]
mod drop;
