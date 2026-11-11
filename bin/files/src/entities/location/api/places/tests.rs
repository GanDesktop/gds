use super::*;
use crate::entities::location::Location as Address;

#[tokio::test]
async fn migrate_directory_pins_and_persist_ordered_abstract_locations() {
    const PREVIOUS: MigrationSet = MigrationSet::new(&[
        MigrationFile::new(1, "001-places.sql", include_str!("assets/001-places.sql")),
        MigrationFile::new(
            2,
            "002-location-views.sql",
            include_str!("assets/002-location-views.sql"),
        ),
        MigrationFile::new(
            3,
            "003-remote-locations.sql",
            include_str!("assets/003-remote-locations.sql"),
        ),
        MigrationFile::new(
            4,
            "004-unified-places.sql",
            include_str!("assets/004-unified-places.sql"),
        ),
    ]);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.sqlite");
    let local = Address::from(Path::new("/existing-pin"));
    let mut old = Options::new(PREVIOUS)
        .models(models!(Location, SavedLocation, ViewSettings))
        .open(
            &gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ),
            &path,
        )
        .await
        .unwrap();
    Location::upsert_by_kind_and_path(
        record::Kind::Pin,
        serde_json::to_vec(Path::new("/existing-pin").as_os_str()).unwrap(),
    )
    .position(3)
    .exec(&mut old)
    .await
    .unwrap();
    drop(old);
    let store = Store::open_at(
        &gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false),
        &path,
    )
    .await
    .unwrap();
    assert_eq!(store.read().await.unwrap().pinned, vec![local.clone()]);
    let tags = Address::from_uri("tags:/工作/设计").unwrap();
    let remote = Address::from_uri("sftp://user@example.com/home/user").unwrap();
    store
        .save(vec![
            Change::Pin {
                location: tags.clone(),
                present: true,
            },
            Change::Pin {
                location: Address::Recent,
                present: true,
            },
            Change::Pin {
                location: remote.clone(),
                present: true,
            },
        ])
        .await
        .unwrap();
    // A stale window only reorders its own pins; a newer remote pin survives.
    let expected = vec![Address::Recent, tags.clone(), local.clone(), remote.clone()];
    store
        .save(vec![Change::PinOrder(expected[..3].to_vec())])
        .await
        .unwrap();
    drop(store);
    let store = Store::open_at(
        &gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false),
        &path,
    )
    .await
    .unwrap();
    assert_eq!(store.read().await.unwrap().pinned, expected);
    store
        .save(vec![Change::Pin {
            location: tags,
            present: false,
        }])
        .await
        .unwrap();
    assert_eq!(
        store.read().await.unwrap().pinned,
        vec![Address::Recent, local, remote]
    );
}
