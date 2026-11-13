use super::*;
use crate::entities::file::x::location::{Layout, Sort};
use crate::entities::location::Location as Address;

#[tokio::test]
async fn persist_ordered_abstract_locations() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("state.sqlite");
    let local = Address::from(Path::new("/existing-pin"));
    let tags = Address::from_uri("tags:/工作/设计").expect("parse tag address");
    let remote =
        Address::from_uri("sftp://user@example.com/home/user").expect("parse remote address");
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);

    let store = Store::open_at(&scope, &path)
        .await
        .expect("open places store");
    store
        .save(vec![
            Change::Pin {
                location: local.clone(),
                present: true,
            },
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
        .expect("save initial pins");
    assert_eq!(
        store.read().await.expect("read initial pins").pinned,
        vec![local.clone(), tags.clone(), Address::Recent, remote.clone()]
    );

    // A stale window only reorders its own pins; a newer remote pin survives.
    let expected = vec![Address::Recent, tags.clone(), local.clone(), remote.clone()];
    store
        .save(vec![Change::PinOrder(expected[..3].to_vec())])
        .await
        .expect("save reordered pins");
    drop(store);

    let store = Store::open_at(&scope, &path)
        .await
        .expect("reopen places store");
    assert_eq!(
        store.read().await.expect("read reordered pins").pinned,
        expected
    );
    store
        .save(vec![Change::Pin {
            location: tags,
            present: false,
        }])
        .await
        .expect("remove tag pin");
    assert_eq!(
        store.read().await.expect("read pins after removal").pinned,
        vec![Address::Recent, local, remote]
    );
}

#[tokio::test]
async fn read_legacy_pascal_case_view_settings() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("state.sqlite");
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);

    let mut store = Store::open_at(&scope, &path)
        .await
        .expect("open places store");
    toasty::sql::query(
        r#"UPDATE view_settings SET columns = '["Name","Modified","DetailedType"]', sort = '{"key":"Name","direction":"Ascending"}' WHERE key_kind = 'defaults'"#,
    )
    .exec(&mut store.db)
    .await
    .expect("write legacy view_settings JSON");
    drop(store);

    let store = Store::open_at(&scope, &path)
        .await
        .expect("reopen places store with legacy view_settings");
    let places = store
        .read()
        .await
        .expect("read places snapshot with legacy view_settings");
    let expected_columns: Layout =
        serde_json::from_str(r#"["name","modified","detailed-type"]"#).expect("canonical layout");
    assert_eq!(places.defaults.columns, Some(expected_columns));
    assert_eq!(places.defaults.sort, Some(Sort::default()));
}
