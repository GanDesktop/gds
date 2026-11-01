use super::{Batch, Error, Mode, Prepared, Request};
use crate::features::file_operations::names::{self, NamedTarget};
use rustix::{
    fs::{CWD, Mode as UnixMode},
    io::Errno,
};
use std::{
    ffi::OsString,
    fs,
    os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
};

fn batch(paths: Vec<PathBuf>, destination: &Path, mode: Mode) -> Batch {
    Batch::new(Request {
        paths,
        destination: destination.to_owned(),
        mode,
    })
    .expect("valid fixture request")
}

fn named_batch(
    paths: Vec<PathBuf>,
    destination: &Path,
    mode: Mode,
    names: Vec<NamedTarget>,
) -> Result<Batch, Error> {
    Batch::named(
        Request {
            paths,
            destination: destination.to_owned(),
            mode,
        },
        names,
    )
}

#[test]
fn cross_device_cleanup_preserves_concurrent_additions_and_edits() {
    for mutation in ["new", "edit", "replace"] {
        let root = tempfile::tempdir().expect("create cleanup fixture");
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).expect("create source");
        fs::write(source.join("copied"), b"original").expect("write source");
        let item = super::Item {
            source: source.clone(),
            destination: destination.clone(),
            destination_committed: false,
            result: Ok(()),
        };
        let prepared = Prepared::new(&item).expect("prepare cross-device copy");
        let copied = super::Node {
            source: source.clone(),
            destination: destination.clone(),
        }
        .copy_root()
        .expect("copy before deterministic cleanup barrier");
        let changed = source.join(if mutation == "new" { "new" } else { "copied" });
        if mutation == "replace" {
            fs::rename(&changed, root.path().join("old-inode")).expect("retain original inode");
        }
        fs::write(&changed, b"concurrent data").expect("simulate concurrent source writer");
        let result = prepared.remove_source(&copied);
        assert!(
            result.is_err(),
            "changed source must report incomplete move"
        );
        assert_eq!(
            fs::read(&changed).expect("concurrent data must survive cleanup"),
            b"concurrent data"
        );
        assert_eq!(
            fs::read(destination.join("copied")).expect("committed copy"),
            b"original"
        );
        assert!(!destination.join("new").exists());
    }
}

#[test]
fn cross_device_directory_move_removes_only_the_copied_tree() {
    let root = tempfile::tempdir().expect("create directory move fixture");
    let source = root.path().join("tree");
    let destination = root.path().join("destination");
    fs::create_dir_all(source.join("child")).expect("create source tree");
    fs::create_dir(&destination).expect("create destination");
    fs::write(source.join("child/file"), b"payload").expect("write payload");
    symlink("missing", source.join("child/link")).expect("create dangling link");
    let report = batch(vec![source.clone()], &destination, Mode::Move)
        .execute_with_rename(|_, _| Err(Errno::XDEV));
    assert!(report.items[0].result.is_ok(), "{report:?}");
    assert!(report.items[0].destination_committed);
    assert!(!source.exists());
    assert_eq!(
        fs::read(destination.join("tree/child/file")).expect("read moved payload"),
        b"payload"
    );
    assert_eq!(
        fs::read_link(destination.join("tree/child/link")).expect("read moved link"),
        Path::new("missing")
    );
}

#[test]
fn copy_preserves_raw_names_links_and_readonly_directory_contents() {
    let root = tempfile::tempdir().expect("create transfer fixture");
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    let tree = source.join("tree");
    let readonly = tree.join("readonly");
    fs::create_dir_all(&readonly).expect("create source tree");
    fs::create_dir(&destination).expect("create destination");
    fs::write(readonly.join("data"), b"complete payload")
        .expect("write readonly directory payload");
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o500))
        .expect("set readonly source directory");
    let raw_name = OsString::from_vec(b"raw-\xff.bin".to_vec());
    fs::write(tree.join(&raw_name), b"raw filename").expect("write non-UTF8 filename");
    let keep = root.path().join("keep");
    fs::write(&keep, b"outside target").expect("write link target");
    symlink(&keep, tree.join("internal-link")).expect("create internal link");
    symlink("missing-target", tree.join("dangling")).expect("create internal dangling link");
    let leaf_link = source.join("leaf-link");
    symlink(&tree, &leaf_link).expect("create link leaf");

    let report = batch(
        vec![tree.clone(), leaf_link.clone()],
        &destination,
        Mode::Copy,
    )
    .execute();
    assert!(
        report.items.iter().all(|item| item.result.is_ok()),
        "{report:?}"
    );
    assert_eq!(report.mode, Mode::Copy);
    assert_eq!(report.committed_destinations().len(), 2);
    assert_eq!(
        fs::read(destination.join("tree/readonly/data")).expect("read copied payload"),
        b"complete payload"
    );
    assert_eq!(
        fs::metadata(destination.join("tree/readonly"))
            .expect("read copied permissions")
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
    assert_eq!(
        fs::read(destination.join("tree").join(&raw_name)).expect("read raw copied filename"),
        b"raw filename"
    );
    assert_eq!(
        fs::read_link(destination.join("tree/internal-link")).expect("read copied link"),
        keep
    );
    assert_eq!(
        fs::read_link(destination.join("tree/dangling")).expect("read copied dangling link"),
        Path::new("missing-target")
    );
    assert!(
        fs::symlink_metadata(destination.join("leaf-link"))
            .expect("inspect copied link leaf")
            .file_type()
            .is_symlink()
    );
    assert!(tree.exists() && fs::symlink_metadata(leaf_link).is_ok());
    assert_eq!(
        fs::read(&keep).expect("read untouched target"),
        b"outside target"
    );
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o700))
        .expect("restore fixture cleanup access");
    fs::set_permissions(
        destination.join("tree/readonly"),
        fs::Permissions::from_mode(0o700),
    )
    .expect("restore copied fixture cleanup access");
}

#[test]
fn conflicts_and_resolved_descendants_never_replace_or_move_sources() {
    let root = tempfile::tempdir().expect("create conflict fixture");
    let source_parent = root.path().join("source");
    let source = source_parent.join("tree");
    let destination = root.path().join("destination");
    fs::create_dir_all(source.join("child")).expect("create source directory");
    fs::create_dir_all(destination.join("tree")).expect("create conflicting target directory");
    fs::write(source.join("original"), b"source").expect("write source payload");
    fs::write(destination.join("tree/sentinel"), b"target").expect("write target sentinel");
    let parent_alias = root.path().join("source-alias");
    let child_alias = root.path().join("child-alias");
    symlink(&source_parent, &parent_alias).expect("create source parent alias");
    symlink(source.join("child"), &child_alias).expect("create destination alias into source");
    for mode in [Mode::Copy, Mode::Move] {
        let conflict = batch(vec![source.clone()], &destination, mode).execute();
        assert!(matches!(
            conflict.items[0].result,
            Err(Error::Conflict { .. })
        ));
        assert!(!conflict.items[0].destination_committed);
        let recursive = batch(vec![parent_alias.join("tree")], &child_alias, mode).execute();
        assert!(matches!(
            recursive.items[0].result,
            Err(Error::IntoSelf { .. })
        ));
        let same = batch(vec![source.clone()], &source, mode).execute();
        assert!(matches!(same.items[0].result, Err(Error::IntoSelf { .. })));
    }
    assert_eq!(
        fs::read(source.join("original")).expect("read unchanged source"),
        b"source"
    );
    assert_eq!(
        fs::read(destination.join("tree/sentinel")).expect("read unchanged target"),
        b"target"
    );
    assert!(!destination.join("tree/original").exists());
    assert_eq!(
        fs::read_dir(source.join("child"))
            .expect("inspect unchanged child")
            .count(),
        0
    );

    let left = root.path().join("left");
    let right = root.path().join("right");
    fs::write(&left, b"left").expect("write rename source");
    fs::write(&right, b"right").expect("write rename target");
    assert_eq!(
        Prepared::rename_no_replace(&left, &right),
        Err(Errno::EXIST)
    );
    assert_eq!(
        fs::read(left).expect("read source after refused rename"),
        b"left"
    );
    assert_eq!(
        fs::read(right).expect("read target after refused rename"),
        b"right"
    );
}

#[test]
fn same_filesystem_move_uses_rename_and_preserves_link_identity() {
    let root = tempfile::tempdir().expect("create move fixture");
    let destination = root.path().join("destination");
    fs::create_dir(&destination).expect("create destination");
    let name = OsString::from_vec(b"move-\xff.bin".to_vec());
    let source = root.path().join(&name);
    fs::write(&source, b"move payload").expect("write move source");
    let inode = fs::metadata(&source).expect("inspect source inode").ino();
    let link = root.path().join("link");
    symlink("missing", &link).expect("create dangling move source");
    let report = batch(
        vec![source.clone(), link.clone(), source.clone()],
        &destination,
        Mode::Move,
    )
    .execute();
    assert_eq!(report.items.len(), 2);
    assert!(
        report.items.iter().all(|item| item.result.is_ok()),
        "{report:?}"
    );
    assert_eq!(
        report.succeeded_sources(),
        vec![source.clone(), link.clone()]
    );
    assert!(!source.exists() && fs::symlink_metadata(&link).is_err());
    assert_eq!(
        fs::metadata(destination.join(&name))
            .expect("inspect moved inode")
            .ino(),
        inode
    );
    assert_eq!(
        fs::read(destination.join(name)).expect("read moved contents"),
        b"move payload"
    );
    assert_eq!(
        fs::read_link(destination.join("link")).expect("read moved link"),
        Path::new("missing")
    );
}

#[test]
fn cross_device_fallback_commits_before_removing_sources_and_retains_failed_cut() {
    let root = tempfile::tempdir().expect("create fallback fixture");
    let destination = root.path().join("destination");
    fs::create_dir(&destination).expect("create destination");
    let success = root.path().join("success");
    let after_failure = root.path().join("after-failure");
    fs::write(&success, b"first payload").expect("write first source");
    fs::write(&after_failure, b"last payload").expect("write last source");
    let failure = root.path().join("unsupported-tree");
    fs::create_dir(&failure).expect("create source containing special file");
    fs::write(failure.join("keep"), b"must remain").expect("write failed tree payload");
    rustix::fs::mkfifoat(CWD, &failure.join("pipe"), UnixMode::RUSR | UnixMode::WUSR)
        .expect("create nonblocking test FIFO fixture");
    let report = batch(
        vec![success.clone(), failure.clone(), after_failure.clone()],
        &destination,
        Mode::Move,
    )
    .execute_with_rename(|_, _| Err(Errno::XDEV));
    assert!(
        report.items[0].result.is_ok() && report.items[2].result.is_ok(),
        "{report:?}"
    );
    assert!(matches!(
        report.items[1].result,
        Err(Error::UnsupportedEntry { .. })
    ));
    assert!(!report.items[1].destination_committed);
    assert!(!success.exists() && !after_failure.exists());
    assert_eq!(
        fs::read(destination.join("success")).expect("read committed first file"),
        b"first payload"
    );
    assert_eq!(
        fs::read(destination.join("after-failure")).expect("read committed last file"),
        b"last payload"
    );
    assert_eq!(
        fs::read(failure.join("keep")).expect("read retained cut source"),
        b"must remain"
    );
    assert!(fs::symlink_metadata(failure.join("pipe")).is_ok());
    assert!(!destination.join("unsupported-tree").exists());
    assert_eq!(
        fs::read_dir(&destination)
            .expect("inspect staging cleanup")
            .count(),
        2
    );
    assert_eq!(report.succeeded_sources(), vec![success, after_failure]);
}

#[test]
fn named_copy_and_move_never_replace_the_existing_target() {
    let root = tempfile::tempdir().expect("create named fixture");
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).expect("create source directory");
    fs::create_dir(&destination).expect("create destination");
    fs::write(source.join("report.pdf"), b"source payload").expect("write source");
    fs::write(destination.join("report.pdf"), b"existing payload").expect("write target");
    let source_path = source.join("report.pdf");

    let report = named_batch(
        vec![source_path.clone()],
        &destination,
        Mode::Copy,
        vec![NamedTarget::new(
            source_path.clone(),
            OsString::from("report (2).pdf"),
        )],
    )
    .expect("named copy")
    .execute();
    assert!(report.items[0].result.is_ok(), "{report:?}");
    assert_eq!(
        fs::read(destination.join("report.pdf")).expect("read untouched target"),
        b"existing payload"
    );
    assert_eq!(
        fs::read(destination.join("report (2).pdf")).expect("read renamed copy"),
        b"source payload"
    );
    assert_eq!(
        fs::read(&source_path).expect("read retained copy source"),
        b"source payload"
    );

    let raw_name = OsString::from_vec(b"renamed-\xff.bin".to_vec());
    let report = named_batch(
        vec![source_path.clone()],
        &destination,
        Mode::Move,
        vec![NamedTarget::new(source_path.clone(), raw_name.clone())],
    )
    .expect("named move")
    .execute();
    assert!(report.items[0].result.is_ok(), "{report:?}");
    assert_eq!(
        fs::read(destination.join(&raw_name)).expect("read renamed move"),
        b"source payload"
    );
    assert!(
        fs::symlink_metadata(&source_path).is_err(),
        "move removed source"
    );
    assert_eq!(
        fs::read(destination.join("report.pdf")).expect("read target after move"),
        b"existing payload"
    );
}

#[test]
fn invalid_and_duplicate_named_targets_are_rejected_before_execution() {
    let root = tempfile::tempdir().expect("create invalid fixture");
    let destination = root.path().join("destination");
    fs::create_dir(&destination).expect("create destination");
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::write(&first, b"first").expect("write first source");
    fs::write(&second, b"second").expect("write second source");

    let invalid = named_batch(
        vec![first.clone()],
        &destination,
        Mode::Copy,
        vec![NamedTarget::new(first.clone(), OsString::from(".."))],
    );
    assert!(matches!(invalid, Err(Error::InvalidName { .. })));

    let duplicate = named_batch(
        vec![first.clone(), second.clone()],
        &destination,
        Mode::Copy,
        vec![
            NamedTarget::new(first.clone(), OsString::from("same")),
            NamedTarget::new(second.clone(), OsString::from("same")),
        ],
    );
    assert!(matches!(duplicate, Err(Error::DuplicateName { .. })));

    assert!(first.exists() && second.exists());
    assert_eq!(
        fs::read_dir(&destination)
            .expect("inspect destination")
            .count(),
        0
    );
}

#[test]
fn a_taken_named_target_retains_the_source_and_its_occupied_target() {
    let root = tempfile::tempdir().expect("create taken fixture");
    let destination = root.path().join("destination");
    fs::create_dir(&destination).expect("create destination");
    let source = root.path().join("source.txt");
    fs::write(&source, b"source").expect("write source");
    fs::write(destination.join("chosen.txt"), b"existing").expect("write occupied target");

    for mode in [Mode::Copy, Mode::Move] {
        let report = named_batch(
            vec![source.clone()],
            &destination,
            mode,
            vec![NamedTarget::new(
                source.clone(),
                OsString::from("chosen.txt"),
            )],
        )
        .expect("named batch")
        .execute();
        assert!(matches!(
            report.items[0].result,
            Err(Error::Conflict { .. })
        ));
        assert!(!report.items[0].destination_committed);
        assert_eq!(fs::read(&source).expect("read retained source"), b"source");
        assert_eq!(
            fs::read(destination.join("chosen.txt")).expect("read untouched target"),
            b"existing"
        );
    }
}

#[test]
fn a_scanned_free_name_completes_the_move_without_replacing_the_original() {
    let root = tempfile::tempdir().expect("create suggestion fixture");
    let destination = root.path().join("destination");
    fs::create_dir(&destination).expect("create destination");
    let source = root.path().join("report.pdf");
    fs::write(&source, b"source").expect("write source");
    fs::write(destination.join("report.pdf"), b"existing").expect("write target");
    let suggestion = names::free_name(
        &destination,
        source.file_name().expect("source leaf"),
        &std::collections::HashSet::new(),
    )
    .expect("free suggestion");
    assert_eq!(suggestion, OsString::from("report (2).pdf"));

    let report = named_batch(
        vec![source.clone()],
        &destination,
        Mode::Move,
        vec![NamedTarget::new(source.clone(), suggestion.clone())],
    )
    .expect("named move")
    .execute();
    assert!(report.items[0].result.is_ok(), "{report:?}");
    assert_eq!(
        report.committed_destinations(),
        vec![destination.join(&suggestion)]
    );
    assert_eq!(
        fs::read(destination.join(&suggestion)).expect("read moved source"),
        b"source"
    );
    assert_eq!(
        fs::read(destination.join("report.pdf")).expect("read original target"),
        b"existing"
    );
    assert!(fs::symlink_metadata(&source).is_err());
}
