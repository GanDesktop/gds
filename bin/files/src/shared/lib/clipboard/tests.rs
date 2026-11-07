use super::{Mode, Snapshot, state::Cache};
use std::path::PathBuf;

#[test]
fn stale_cut_validation_cannot_override_a_newer_intent() {
    let a = PathBuf::from("/tmp/stale-a");
    let b = PathBuf::from("/tmp/stale-b");
    let cut = Snapshot::new(Mode::Cut, vec![a.clone()]).expect("cut snapshot");
    let mut cache = Cache::default();
    cache.record_write(&cut);
    let generation = cache.begin_refresh().expect("validation read");
    assert!(
        cache.begin_refresh().is_none(),
        "only one validation read is in flight"
    );
    let newer = Snapshot::new(Mode::Cut, vec![b.clone()]).expect("newer cut");
    cache.record_write(&newer);
    let validating_generation = cache
        .begin_refresh()
        .expect("the newer intent validates at once");

    assert!(!cache.reconcile(generation, Ok(Some(cut)), false));
    assert!(cache.is_cut(&b) && !cache.is_cut(&a));
    assert!(
        cache.begin_refresh().is_none(),
        "a stale completion must not release the newer read"
    );
    assert!(!cache.reconcile(validating_generation, Ok(Some(newer)), false));
    assert!(cache.is_cut(&b));
}
