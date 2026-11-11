use super::*;

#[test]
fn tag_history_preserves_uri_and_rejects_stale_results() {
    let tag = Location::from_uri("tags:/工作/设计").expect("parse tag location");
    let mut navigation = Navigation::<()>::new(Location::Recent, None);
    let old = navigation.prepare_open(tag.clone(), None, ReadOptions::default());
    let current = navigation.prepare_open(tag.clone(), None, ReadOptions::default());
    assert_eq!(
        navigation
            .finish_load(&old, Ok(None))
            .expect("finish stale load"),
        LoadOutcome::Stale
    );
    assert_eq!(
        navigation
            .finish_load(&current, Ok(None))
            .expect("finish current load"),
        LoadOutcome::Applied
    );
    assert_eq!(navigation.location(), &tag);
    assert!(navigation.path().is_none());
    assert!(navigation.directory().is_none());
    let back = navigation
        .prepare_history(0, None, ReadOptions::default())
        .expect("prepare back navigation");
    navigation
        .finish_load(&back, Ok(None))
        .expect("finish back navigation");
    assert_eq!(navigation.location(), &Location::Recent);
    let forward = navigation
        .prepare_history(1, None, ReadOptions::default())
        .expect("prepare forward navigation");
    navigation
        .finish_load(&forward, Ok(None))
        .expect("finish forward navigation");
    assert_eq!(navigation.location(), &tag);
}
