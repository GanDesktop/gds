use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use gds::service::remote::{Address, Entry as RemoteEntry, EntryKind as RemoteKind};

use super::{
    apply_reorder_tab, apply_toggle_pin, filter_remote_entries, location_supports_grid,
    pinned_tab_locations, sort_remote_entries, sortable_columns_for, sync_pinned_navigation,
    toggle_tab_or_selected_favorite,
};
use crate::entities::{
    file::{
        columns::Column,
        sorting::{Direction, Key, Sort},
    },
    location::{Location, places::Places, scheme::TagAddress},
};
use crate::features::file_operations::history::History;
use crate::pages::workspace::model::{Contents, Operation, Row, State, Tab};

fn remote_item(name: &str, kind: RemoteKind, size: Option<u64>, secs: u64) -> RemoteEntry {
    let base = Address::parse("sftp://user@example.com/root").expect("valid remote test address");
    let address = base.child(name).expect("valid remote child");
    RemoteEntry {
        address,
        name: name.to_owned(),
        kind,
        size,
        modified: Some(UNIX_EPOCH + Duration::from_secs(secs)),
    }
}

fn make_state(tabs: Vec<Tab>, active: usize, places: Places) -> State {
    State {
        next_tab: (tabs.len() as u64) + 1,
        tabs,
        active,
        home: PathBuf::from("/home/tester"),
        places,
        history: History::default(),
        picker: None,
        operation: Operation::Idle,
        saving_places: false,
        favorites_reload: None,
        pending_device_removals: Vec::new(),
    }
}

#[test]
fn remote_sort_keeps_directories_first_and_ties_stable() {
    let mut entries = vec![
        remote_item("zeta.txt", RemoteKind::File, Some(5), 10),
        remote_item("docs", RemoteKind::Directory, None, 5),
        remote_item("alpha.txt", RemoteKind::File, Some(5), 20),
    ];
    sort_remote_entries(
        &mut entries,
        Sort {
            key: Key::Size,
            direction: Direction::Ascending,
        },
    );
    assert_eq!(entries[0].name, "docs");
    assert_eq!(entries[1].name, "alpha.txt");
    assert_eq!(entries[2].name, "zeta.txt");
}

#[test]
fn remote_sort_descending_keeps_directories_first_and_reverses_files() {
    let mut entries = vec![
        remote_item("small.txt", RemoteKind::File, Some(5), 10),
        remote_item("dir-b", RemoteKind::Directory, None, 5),
        remote_item("large.txt", RemoteKind::File, Some(50), 20),
        remote_item("dir-a", RemoteKind::Directory, None, 15),
    ];
    sort_remote_entries(
        &mut entries,
        Sort {
            key: Key::Size,
            direction: Direction::Descending,
        },
    );
    assert_eq!(entries[0].name, "dir-b");
    assert_eq!(entries[1].name, "dir-a");
    assert_eq!(entries[2].name, "large.txt");
    assert_eq!(entries[3].name, "small.txt");
}

#[test]
fn remote_sort_by_type_orders_directories_files_by_category_then_links() {
    let mut entries = vec![
        remote_item("shortcut", RemoteKind::SymbolicLink, None, 10),
        remote_item("photo.png", RemoteKind::File, Some(100), 10),
        remote_item("readme.txt", RemoteKind::File, Some(20), 10),
        remote_item("assets", RemoteKind::Directory, None, 10),
    ];
    sort_remote_entries(
        &mut entries,
        Sort {
            key: Key::Type,
            direction: Direction::Ascending,
        },
    );
    assert_eq!(entries[0].name, "assets");
    assert_eq!(entries[1].name, "readme.txt");
    assert_eq!(entries[2].name, "photo.png");
    assert_eq!(entries[3].name, "shortcut");
}

#[test]
fn filter_remote_entries_respects_hidden_toggle_and_substring_query() {
    let entries = vec![
        remote_item(".env", RemoteKind::File, Some(12), 1),
        remote_item("Report.PDF", RemoteKind::File, Some(40), 2),
        remote_item("notes.txt", RemoteKind::File, Some(10), 3),
    ];
    let visible_only = filter_remote_entries(&entries, "", false);
    assert_eq!(visible_only.len(), 2);
    assert_eq!(visible_only[0].name, "Report.PDF");

    let with_hidden = filter_remote_entries(&entries, "env", true);
    assert_eq!(with_hidden.len(), 1);
    assert_eq!(with_hidden[0].name, ".env");

    let case_insensitive = filter_remote_entries(&entries, "report", false);
    assert_eq!(case_insensitive.len(), 1);
    assert_eq!(case_insensitive[0].name, "Report.PDF");
}

#[test]
fn toggle_pin_repartitions_tabs_and_preserves_active_tab_identity() {
    let places = Places::default();
    let loc_a = Location::from(PathBuf::from("/tmp/a").as_path());
    let loc_b = Location::from(PathBuf::from("/tmp/b").as_path());
    let loc_c = Location::from(PathBuf::from("/tmp/c").as_path());
    let mut tabs = vec![
        Tab::new(1, loc_a.clone(), &places, true),
        Tab::new(2, loc_b, &places, false),
        Tab::new(3, loc_c.clone(), &places, false),
    ];
    let mut active = 1_usize;

    let pinned_outcome = apply_toggle_pin(&mut tabs, &mut active, 2);
    assert_eq!(pinned_outcome, Some((loc_c.clone(), true)));
    assert_eq!(pinned_tab_locations(&tabs), vec![loc_a.clone(), loc_c]);
    assert_eq!(tabs[active].id, 2);

    let unpinned_outcome = apply_toggle_pin(&mut tabs, &mut active, 0);
    assert_eq!(unpinned_outcome, Some((loc_a, false)));
    assert_eq!(tabs[0].id, 3);
    assert_eq!(tabs[1].id, 1);
    assert_eq!(tabs[active].id, 2);
}

#[test]
fn reorder_tab_clamps_within_pinned_and_unpinned_partitions() {
    let places = Places::default();
    let loc_a = Location::from(PathBuf::from("/tmp/a").as_path());
    let loc_b = Location::from(PathBuf::from("/tmp/b").as_path());
    let loc_c = Location::from(PathBuf::from("/tmp/c").as_path());
    let loc_d = Location::from(PathBuf::from("/tmp/d").as_path());
    let mut tabs = vec![
        Tab::new(1, loc_a, &places, true),
        Tab::new(2, loc_b, &places, true),
        Tab::new(3, loc_c, &places, false),
        Tab::new(4, loc_d, &places, false),
    ];
    let mut active = 0_usize;

    let moved_pinned = apply_reorder_tab(&mut tabs, &mut active, 0, 3);
    assert_eq!(moved_pinned, Some(true));
    assert_eq!(tabs[0].id, 2);
    assert_eq!(tabs[1].id, 1);
    assert_eq!(active, 1);

    let moved_unpinned = apply_reorder_tab(&mut tabs, &mut active, 3, 0);
    assert_eq!(moved_unpinned, Some(false));
    assert_eq!(tabs[2].id, 4);
    assert_eq!(tabs[3].id, 3);
}

#[test]
fn sync_pinned_navigation_updates_places_pin_set_and_order() {
    let loc_old = Location::from(PathBuf::from("/tmp/old").as_path());
    let loc_keep = Location::from(PathBuf::from("/tmp/keep").as_path());
    let loc_new = Location::from(PathBuf::from("/tmp/new").as_path());
    let mut places = Places::default();
    places.set_pinned(&loc_old, true);
    places.set_pinned(&loc_keep, true);
    places.reorder_pinned(vec![loc_old.clone(), loc_keep.clone()]);

    let tabs = vec![
        Tab::new(1, loc_new.clone(), &places, true),
        Tab::new(2, loc_keep.clone(), &places, true),
    ];
    let mut state = make_state(tabs, 0, places);

    let changed = sync_pinned_navigation(&mut state, &loc_old, &loc_new);
    assert!(changed);
    assert_eq!(state.places.pinned, vec![loc_new, loc_keep]);
}

#[test]
fn favorite_on_favorites_page_toggles_selected_row_and_ignores_empty_selection() {
    let target = Location::from(PathBuf::from("/tmp/bookmarked").as_path());
    let mut places = Places::default();
    places.set_favorite(target.clone(), true);

    let mut fav_tab = Tab::new(1, Location::Favorites, &places, false);
    let row = Row::Place(target.clone());
    let key = row.key();
    fav_tab.rows = vec![row];

    let mut state = make_state(vec![fav_tab], 0, places);

    // Pressing Ctrl+D with no selected row on Favorites must not bookmark location://favorites.
    let (mutated_empty, cleared_empty) = toggle_tab_or_selected_favorite(&mut state);
    assert!(!mutated_empty);
    assert!(!cleared_empty);
    assert!(!state.places.is_favorite(&Location::Favorites));
    assert!(state.places.is_favorite(&target));

    // Selecting the bookmarked row and toggling removes the target and clears selection.
    state
        .tab_mut()
        .navigation
        .view_mut()
        .selection
        .replace([key]);
    let (mutated_sel, cleared_sel) = toggle_tab_or_selected_favorite(&mut state);
    assert!(mutated_sel);
    assert!(cleared_sel);
    assert!(!state.places.is_favorite(&target));
    assert!(state.tab().navigation.view().selection.is_empty());
}

#[test]
fn trash_and_places_columns_are_unsortable_while_directories_expose_all_columns() {
    let places = Places::default();
    let mut trash_tab = Tab::new(1, Location::Trash, &places, false);
    trash_tab.contents = Contents::Trash(Vec::new());
    let trash_state = make_state(vec![trash_tab], 0, places);
    assert!(sortable_columns_for(&trash_state).is_empty());

    let recent_places = Places::default();
    let mut recent_tab = Tab::new(1, Location::Recent, &recent_places, false);
    recent_tab.contents = Contents::Recent(Vec::new());
    let recent_state = make_state(vec![recent_tab], 0, recent_places);
    assert!(sortable_columns_for(&recent_state).is_empty());

    let fav_places = Places::default();
    let mut fav_tab = Tab::new(1, Location::Favorites, &fav_places, false);
    fav_tab.contents = Contents::Places(Vec::new());
    let fav_state = make_state(vec![fav_tab], 0, fav_places);
    assert!(sortable_columns_for(&fav_state).is_empty());

    let dir_places = Places::default();
    let dir_tab = Tab::new(
        1,
        Location::from(PathBuf::from("/tmp").as_path()),
        &dir_places,
        false,
    );
    let dir_state = make_state(vec![dir_tab], 0, dir_places);
    assert_eq!(sortable_columns_for(&dir_state), Column::ALL.to_vec());
}

#[test]
fn grid_mode_is_supported_on_places_tags_and_remote_but_disabled_on_trash_volumes_network() {
    let remote_addr = Address::parse("sftp://user@example.com/srv").expect("valid remote address");
    let tag_addr = TagAddress::new("work").expect("valid tag address");
    assert!(location_supports_grid(&Location::Recent));
    assert!(location_supports_grid(&Location::Favorites));
    assert!(location_supports_grid(&Location::Tags(tag_addr)));
    assert!(location_supports_grid(&Location::Remote(remote_addr)));
    assert!(!location_supports_grid(&Location::Trash));
    assert!(!location_supports_grid(&Location::Volumes));
    assert!(!location_supports_grid(&Location::Network));
}
