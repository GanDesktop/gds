use super::*;
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
