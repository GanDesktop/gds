use super::*;
use std::fs;

#[test]
fn remote_authentication_resolution_uses_the_existing_history_and_rejects_late_results() {
    let mut nav = Nav::new(Location::Network, None);
    let address = gds::service::remote::Address::parse("sftp://host/team").expect("address");
    let request = nav.prepare_open(
        Location::Remote(address.clone()),
        None,
        ReadOptions::default(),
    );
    let authenticated = address.with_username("alice").expect("username");
    let request = request.resolve_remote(authenticated.clone());
    assert_eq!(
        nav.finish_load(&request, Ok(None)).expect("commit"),
        LoadOutcome::Applied
    );
    assert_eq!(nav.location(), &Location::Remote(authenticated.clone()));
    assert!(nav.path().is_none());
    let late = nav.prepare_refresh(None, ReadOptions::default());
    let back = nav
        .prepare_back(None, ReadOptions::default())
        .expect("back");
    assert_eq!(
        nav.finish_load(&back, Ok(None)).expect("network"),
        LoadOutcome::Applied
    );
    assert_eq!(
        nav.finish_load(&late, Ok(None)).expect("late response"),
        LoadOutcome::Stale
    );
    assert_eq!(nav.location(), &Location::Network);
    let forward = nav
        .prepare_forward(None, ReadOptions::default())
        .expect("forward");
    assert_eq!(
        nav.finish_load(&forward, Ok(None)).expect("remote"),
        LoadOutcome::Applied
    );
    assert_eq!(nav.location(), &Location::Remote(authenticated));
}

#[derive(Default, Debug, PartialEq)]
struct View {
    selected: Vec<PathBuf>,
    scroll: f32,
}
type Nav = Navigation<View>;

fn apply(nav: &mut Nav, request: LoadRequest) {
    assert_eq!(
        nav.finish_load(&request, request.load())
            .expect("route load"),
        LoadOutcome::Applied
    );
}

fn open(nav: &mut Nav, location: Location, path: Option<PathBuf>) {
    let request = nav.prepare_open(location, path, ReadOptions::default());
    apply(nav, request);
}

#[test]
fn files_and_virtual_locations_share_one_history_and_restore_view_state() {
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().to_owned();
    let mut nav = Nav::new(Location::from(&path), Some(path.clone()));
    let first = nav.prepare_refresh(Some(path.clone()), ReadOptions::default());
    apply(&mut nav, first);
    nav.view_mut().selected = vec![path.join("selected")];
    nav.view_mut().scroll = -120.;
    open(&mut nav, Location::Recent, None);
    nav.view_mut().scroll = -20.;
    open(&mut nav, Location::Trash, None);
    let back = nav
        .prepare_back(None, ReadOptions::default())
        .expect("recent");
    apply(&mut nav, back);
    assert_eq!(nav.location(), &Location::Recent);
    assert_eq!(nav.view().scroll, -20.);
    let back = nav
        .prepare_back(Some(path.clone()), ReadOptions::default())
        .expect("directory");
    apply(&mut nav, back);
    assert_eq!(nav.path(), Some(path.as_path()));
    assert_eq!(nav.view().selected, [path.join("selected")]);
    assert_eq!(nav.view().scroll, -120.);
    let forward = nav
        .prepare_forward(None, ReadOptions::default())
        .expect("recent");
    apply(&mut nav, forward);
    assert_eq!(nav.location(), &Location::Recent);
    open(&mut nav, Location::Favorites, None);
    assert!(!nav.can_go_forward());
}

#[test]
fn jumping_to_a_history_entry_restores_its_view_and_preserves_the_forward_branch() {
    let mut nav = Nav::new(Location::Recent, None);
    nav.view_mut().scroll = -120.;
    open(&mut nav, Location::Trash, None);
    nav.view_mut().scroll = -20.;
    open(&mut nav, Location::Network, None);
    open(&mut nav, Location::Volumes, None);

    let request = nav
        .prepare_history(0, None, ReadOptions::default())
        .expect("jump across multiple visits");
    apply(&mut nav, request);
    assert_eq!(nav.history_index(), 0);
    assert_eq!(nav.location(), &Location::Recent);
    assert_eq!(nav.view().scroll, -120.);
    assert!(nav.can_go_forward());
    assert_eq!(nav.history().len(), 4);

    let request = nav
        .prepare_history(1, None, ReadOptions::default())
        .expect("jump forward");
    apply(&mut nav, request);
    assert_eq!(nav.location(), &Location::Trash);
    assert_eq!(nav.view().scroll, -20.);
    open(&mut nav, Location::Favorites, None);
    assert_eq!(nav.history().len(), 3);
    assert_eq!(nav.history()[2].location, Location::Favorites);
    assert!(!nav.can_go_forward());
    assert!(
        nav.prepare_history(2, None, ReadOptions::default())
            .is_none()
    );
    assert!(
        nav.prepare_history(3, None, ReadOptions::default())
            .is_none()
    );
}

#[test]
fn failed_and_superseded_loads_cannot_replace_the_current_route() {
    let root = tempfile::tempdir().expect("fixture");
    let mut nav = Nav::new(Location::Trash, None);
    let older = nav.prepare_open(
        Location::from(root.path()),
        Some(root.path().to_owned()),
        ReadOptions::default(),
    );
    let latest = nav.prepare_open(Location::Favorites, None, ReadOptions::default());
    assert_eq!(
        nav.finish_load(&older, older.load()).expect("stale"),
        LoadOutcome::Stale
    );
    apply(&mut nav, latest);
    let missing = root.path().join("missing");
    let failed = nav.prepare_open(
        Location::from(&missing),
        Some(missing),
        ReadOptions::default(),
    );
    assert!(nav.finish_load(&failed, failed.load()).is_err());
    assert_eq!(nav.location(), &Location::Favorites);
    assert_eq!(nav.back_location(), Some(&Location::Trash));
}

#[test]
fn volume_identity_survives_unmount_and_resolves_to_a_new_mount_without_duplicate_history() {
    let first = tempfile::tempdir().expect("mount one");
    let next = tempfile::tempdir().expect("mount two");
    let volume = Location::Volume {
        identity: super::super::address::VolumeIdentity::Stable("disk".into()),
        relative: "folder".into(),
    };
    for root in [first.path(), next.path()] {
        fs::create_dir(root.join("folder")).expect("subdirectory");
    }
    let mut nav = Nav::new(Location::Volumes, None);
    open(&mut nav, volume.clone(), Some(first.path().join("folder")));
    let refresh = nav.prepare_refresh(None, ReadOptions::default());
    apply(&mut nav, refresh);
    assert_eq!(nav.location(), &volume);
    assert!(nav.path().is_none());
    let refresh = nav.prepare_refresh(Some(next.path().join("folder")), ReadOptions::default());
    apply(&mut nav, refresh);
    assert_eq!(nav.back_location(), Some(&Location::Volumes));
    assert_eq!(nav.location(), &volume);
    assert_eq!(nav.path(), Some(next.path().join("folder").as_path()));
}

#[test]
fn typed_paths_preserve_literal_names_and_symlink_parent_semantics() {
    let base = Path::new("/tmp/base");
    assert_eq!(
        resolve_input("link/../target", base, None).expect("relative"),
        base.join("link/../target")
    );
    assert_eq!(
        resolve_input(" folder with spaces ", base, None).expect("spaces"),
        base.join(" folder with spaces ")
    );
    assert_eq!(
        resolve_input("~/Downloads", base, Some(Path::new("/home/user"))).expect("home"),
        Path::new("/home/user/Downloads")
    );
    assert!(resolve_input("smb://server/share", base, None).is_err());
}
