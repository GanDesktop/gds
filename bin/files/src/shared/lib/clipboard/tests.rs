use super::{
    Error, Identity, Mode, Snapshot,
    native::NativeClipboard,
    protocol::{CLIPBOARD_ID, GNOME_FILES, KDE_CUT, TEXT_PLAIN_UTF8, URI_LIST},
    state::Cache,
};
use std::{path::PathBuf, sync::Arc};
use zgui::platform::{ClipboardMime, ClipboardMimeOffer};

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

#[test]
fn shared_snapshot_removal_preserves_order_and_rejects_replaced_identity() {
    let paths: Vec<PathBuf> = (0..512)
        .map(|index| PathBuf::from(format!("/tmp/clip-{index}")))
        .collect();
    let removed = PathBuf::from("/tmp/clip-257");
    let expected = Arc::new(Snapshot::new(Mode::Copy, paths.clone()).expect("copy snapshot"));
    let current = expected.as_ref().clone();

    let remaining = expected
        .excluding(&current, std::slice::from_ref(&removed))
        .expect("matching snapshot removes target entry");
    assert_eq!(remaining.len(), paths.len() - 1);
    assert!(!remaining.contains(&removed));
    assert_eq!(remaining[256], PathBuf::from("/tmp/clip-256"));
    assert_eq!(remaining[257], PathBuf::from("/tmp/clip-258"));
    assert!(
        expected
            .excluding(&current, &[PathBuf::from("/tmp/not-in-clipboard")])
            .is_none(),
        "absent paths must not republish an unchanged offer"
    );

    let replaced = Snapshot::new(Mode::Copy, paths.clone()).expect("newer copy with same paths");
    assert!(
        expected
            .excluding(&replaced, std::slice::from_ref(&removed))
            .is_none(),
        "a newer offer with the same paths has a distinct identity and must not be overwritten"
    );

    let foreign = Snapshot {
        mode: Mode::Copy,
        paths,
        identity: Identity(None),
    };
    assert!(
        expected
            .excluding(&foreign, std::slice::from_ref(&removed))
            .is_none(),
        "a foreign clipboard offer must not be overwritten"
    );
}

#[test]
fn multi_mime_offers_preserve_intent_identity_and_ignore_plain_uri_text() {
    let snapshot =
        Snapshot::new(Mode::Cut, vec![PathBuf::from("/tmp/a b#%.txt")]).expect("selection");
    let (written, entries) = NativeClipboard::encode_snapshot(&snapshot).expect("encoded offer");
    assert_eq!(written, snapshot);
    for expected_mime in [
        URI_LIST,
        GNOME_FILES,
        KDE_CUT,
        CLIPBOARD_ID,
        TEXT_PLAIN_UTF8,
    ] {
        assert!(
            entries.iter().any(|entry| entry.mime_type == expected_mime),
            "missing expected MIME {expected_mime}"
        );
    }
    let plain = entries
        .iter()
        .find(|entry| entry.mime_type == TEXT_PLAIN_UTF8)
        .expect("plain text fallback entry");
    assert_eq!(plain.bytes, b"/tmp/a b#%.txt");

    let mut offer = ClipboardMimeOffer {
        revision: 7,
        entries,
    };
    assert_eq!(
        NativeClipboard::decode_snapshot(&offer).expect("owned cut"),
        Some(snapshot.clone())
    );
    offer
        .entries
        .retain(|entry| entry.mime_type != CLIPBOARD_ID);
    let foreign = NativeClipboard::decode_snapshot(&offer)
        .expect("GNOME cut")
        .expect("files");
    assert_eq!(foreign.mode, Mode::Cut);
    assert_eq!(foreign.paths, snapshot.paths);
    assert!(!foreign.can_cancel());

    offer.entries.retain(|entry| entry.mime_type != GNOME_FILES);
    assert_eq!(
        NativeClipboard::decode_snapshot(&offer).expect("KDE cut"),
        Some(foreign)
    );

    offer.entries = vec![ClipboardMime::new(TEXT_PLAIN_UTF8, b"file:///tmp/a")];
    assert!(
        NativeClipboard::decode_snapshot(&offer)
            .expect("ordinary text")
            .is_none()
    );
}

#[test]
fn remote_file_offers_preserve_cut_intent_without_becoming_local_paths() {
    let offer = ClipboardMimeOffer {
        revision: 4,
        entries: vec![ClipboardMime::new(
            GNOME_FILES,
            b"cut\nsftp://example.com/home/user/a%20b",
        )],
    };
    let (mode, uris) = NativeClipboard::decode_uris(&offer)
        .expect("URI format")
        .expect("offer");
    assert_eq!(mode, Mode::Cut);
    assert_eq!(uris, ["sftp://example.com/home/user/a%20b"]);
    assert!(matches!(
        NativeClipboard::decode_snapshot(&offer),
        Err(Error::InvalidUri(_))
    ));
}

#[test]
fn local_unfocused_fallback_tracks_baseline_revision_until_platform_changes() {
    let path = PathBuf::from("/tmp/headless-cut");
    let snapshot = Snapshot::new(Mode::Cut, vec![path.clone()]).expect("cut snapshot");
    let mut cache = Cache::default();
    cache.record_local_write(&snapshot, Some(3));

    assert!(cache.is_local_fallback());
    assert!(cache.fallback_matches_platform(Some(3)));
    assert!(!cache.fallback_matches_platform(Some(4)));
    assert!(!cache.fallback_matches_platform(None));
    assert!(cache.is_cut(&path));
    assert!(cache.can_paste());

    let gen1 = cache.begin_refresh().expect("refresh starts");
    assert!(cache.reconcile(gen1, Ok(None), false));
    assert!(!cache.is_local_fallback());
    assert!(!cache.can_paste());
}
