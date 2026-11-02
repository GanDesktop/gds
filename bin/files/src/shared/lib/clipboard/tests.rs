use std::path::PathBuf;

use gpui_kit::{ClipboardEntry, ClipboardItem, ExternalPaths};

use super::{
    Error, Identity, Mode, Snapshot, copyable_uris, fallback, protocol::Formats, state::Cache,
};

#[test]
fn file_formats_round_trip_intent_and_original_names() {
    let paths = vec![
        PathBuf::from("/tmp/a # percent% and\nnewline"),
        PathBuf::from("/tmp/中文.txt"),
    ];
    let snapshot = Snapshot::new(Mode::Cut, paths.clone()).expect("file selection");
    let formats = Formats::encode(&snapshot).expect("encode file formats");
    let gnome = Formats::decode_gnome(&formats.gnome).expect("GNOME file list");
    assert_eq!(gnome.mode, Mode::Cut);
    assert_eq!(gnome.paths, paths);
    assert_eq!(
        Formats::kde_mode(&formats.kde_cut).expect("KDE cut marker"),
        Mode::Cut
    );
    let copied = Formats::decode_uris(&formats.uri_list, Mode::Copy).expect("URI list");
    assert_eq!(copied.mode, Mode::Copy);
    assert_eq!(copied.paths, paths);
    assert!(!copied.identity.is_owned());
}

#[test]
fn published_formats_match_nautilus_and_kde_wire_data() {
    // Nautilus splits on LF and rejects empty URI entries, including a trailing one.
    // https://github.com/GNOME/nautilus/blob/main/src/nautilus-clipboard.c
    let paths = vec![PathBuf::from("/tmp/a b.txt"), PathBuf::from("/tmp/%#.png")];
    for (mode, gnome, kde) in [
        (
            Mode::Copy,
            "copy\nfile:///tmp/a%20b.txt\nfile:///tmp/%25%23.png",
            b"0",
        ),
        (
            Mode::Cut,
            "cut\nfile:///tmp/a%20b.txt\nfile:///tmp/%25%23.png",
            b"1",
        ),
    ] {
        let snapshot = Snapshot::new(mode, paths.clone()).expect("file selection");
        let formats = Formats::encode(&snapshot).expect("desktop file formats");
        assert_eq!(formats.gnome, gnome.as_bytes());
        assert_eq!(
            formats.uri_list,
            b"file:///tmp/a%20b.txt\r\nfile:///tmp/%25%23.png\r\n"
        );
        assert_eq!(formats.kde_cut, kde);
        let external = Formats::decode_gnome(gnome.as_bytes()).expect("external Nautilus payload");
        assert_eq!(external.mode, mode);
        assert_eq!(external.paths, paths);
        assert!(
            !external.identity.is_owned(),
            "standard formats need no private identity"
        );
    }
}

#[test]
fn unrelated_text_remote_uris_and_invalid_intent_do_not_become_files() {
    let plain = ClipboardItem::new_string("file:///tmp/a\n".into());
    assert_eq!(
        fallback::Clipboard::decode(Some(&plain)).expect("ordinary text is not a file offer"),
        None
    );
    for uri in [
        "https://example.org/a",
        "file://server/tmp/a",
        "file:relative",
        "file:///tmp/a?query",
        "file:///tmp/a#fragment",
        "file:///tmp/a%00b",
        "/tmp/a",
    ] {
        assert!(
            Formats::decode_uris(uri.as_bytes(), Mode::Copy).is_err(),
            "{uri}"
        );
    }
    assert!(Formats::decode_gnome(b"move\nfile:///tmp/a\n").is_err());
    assert!(Formats::kde_mode(b"maybe").is_err());
    assert_eq!(
        Formats::kde_mode(b"0\r\n").expect("copy marker"),
        Mode::Copy
    );
    let local = Formats::decode_uris(b"# comment\r\nfile://localhost/tmp/a\r\n", Mode::Copy)
        .expect("local host");
    assert_eq!(local.paths, [PathBuf::from("/tmp/a")]);

    let snapshot = Snapshot::new(Mode::Copy, vec![PathBuf::from("/tmp/a")]).expect("selection");
    let mut item = fallback::Clipboard::encode(&snapshot).expect("same-process file item");
    for entry in &mut item.entries {
        if let ClipboardEntry::ExternalPaths(paths) = entry {
            *paths = ExternalPaths([PathBuf::from("/tmp/replaced")].into_iter().collect());
        }
    }
    assert!(
        fallback::Clipboard::decode(Some(&item)).is_err(),
        "typed paths must agree with the advertised URI list"
    );
}

#[test]
fn completed_cut_entries_only_change_the_unchanged_owned_snapshot() {
    let a = PathBuf::from("/tmp/a");
    let b = PathBuf::from("/tmp/b");
    let snapshot =
        Snapshot::new(Mode::Cut, vec![a.clone(), b.clone(), a.clone()]).expect("selection");
    assert_eq!(snapshot.paths, [a.clone(), b.clone()]);
    assert_eq!(
        snapshot.remaining_after(&snapshot, &[a.clone()]),
        Some(vec![b.clone()])
    );
    assert_eq!(
        snapshot.remaining_after(&snapshot, &[a.clone(), b.clone()]),
        Some(Vec::new())
    );
    assert_eq!(
        snapshot.remaining_after(&snapshot, &[PathBuf::from("/tmp/other")]),
        None
    );
    let new_copy =
        Snapshot::new(Mode::Cut, snapshot.paths.clone()).expect("same paths copied again");
    assert_ne!(snapshot.identity, new_copy.identity);
    assert_eq!(snapshot.remaining_after(&new_copy, &[a.clone()]), None);
    let foreign = Snapshot {
        mode: Mode::Cut,
        paths: snapshot.paths.clone(),
        identity: Identity(None),
    };
    assert_eq!(foreign.remaining_after(&foreign, &[a]), None);
    let copy = Snapshot::new(Mode::Copy, vec![b.clone()]).expect("copy");
    assert_eq!(copy.remaining_after(&copy, &[b]), None);
}

#[cfg(unix)]
#[test]
fn unix_non_utf8_paths_survive_uri_and_gpui_metadata_round_trips() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let path = PathBuf::from(OsString::from_vec(b"/tmp/raw-\xff%#\n".to_vec()));
    let snapshot = Snapshot::new(Mode::Copy, vec![path.clone()]).expect("raw path selection");
    let formats = Formats::encode(&snapshot).expect("encode raw paths");
    assert_eq!(
        Formats::decode_uris(&formats.uri_list, Mode::Copy)
            .expect("decode raw URI")
            .paths,
        [path]
    );
    let item = fallback::Clipboard::encode(&snapshot)
        .expect("GPUI item without touching system clipboard");
    assert_eq!(
        fallback::Clipboard::decode(Some(&item)).expect("GPUI own metadata"),
        Some(snapshot)
    );
    let invalid = PathBuf::from(OsString::from_vec(b"/tmp/nul\0".to_vec()));
    assert!(matches!(
        Snapshot::new(Mode::Copy, vec![invalid]),
        Err(Error::InvalidPath(_))
    ));
}

#[cfg(unix)]
#[test]
fn uri_parent_resolution_preserves_filesystem_meaning_without_following_the_leaf() {
    use std::{fs, os::unix::fs::symlink};
    let temp = tempfile::tempdir().expect("create fixture");
    let root = temp.path();
    fs::create_dir_all(root.join("actual/nested")).expect("create target tree");
    symlink(root.join("actual/nested"), root.join("alias")).expect("parent alias");
    symlink(root.join("missing"), root.join("actual/leaf")).expect("dangling leaf");
    let original = root.join("alias/../leaf");
    let snapshot =
        Snapshot::new(Mode::Cut, vec![original.clone()]).expect("selection through parent alias");
    let formats = Formats::encode(&snapshot).expect("URI resolves only the parent");
    assert_eq!(
        Formats::decode_uris(&formats.uri_list, Mode::Cut)
            .expect("decode URI")
            .paths,
        [root.join("actual/leaf")]
    );
    assert_eq!(
        fallback::Clipboard::decode(Some(
            &fallback::Clipboard::encode(&snapshot).expect("GPUI file item")
        ))
        .expect("typed GPUI paths")
        .expect("same-process metadata")
        .paths,
        [original],
    );
    assert!(
        fs::symlink_metadata(root.join("actual/leaf"))
            .expect("leaf remains")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn cut_cache_tracks_partial_pastes_and_newer_clipboard_writes() {
    let a = PathBuf::from("/tmp/cut-a");
    let b = PathBuf::from("/tmp/cut-b");
    let cut = Snapshot::new(Mode::Cut, vec![a.clone(), b.clone()]).expect("cut snapshot");
    let mut cache = Cache::default();
    assert!(!cache.has_cut() && !cache.is_cut(&a));

    cache.record_write(&cut);
    assert!(cache.is_cut(&a) && cache.is_cut(&b));

    let copy = Snapshot::new(Mode::Copy, vec![a.clone()]).expect("copy snapshot");
    cache.record_write(&copy);
    assert!(!cache.has_cut(), "an ordinary copy replaces the cut intent");
    assert_eq!(cache.current().as_deref(), Some(&copy));

    cache.record_write(&cut);
    cache.note_pasted(&cut, &[a.clone()]);
    assert!(!cache.is_cut(&a));
    assert_eq!(
        cache.current().expect("clipboard remainder").paths,
        [b.clone()]
    );
    assert!(
        cache.is_cut(&b),
        "an entry the paste did not move stays marked"
    );

    let newer = Snapshot::new(Mode::Cut, vec![a.clone()]).expect("newer cut");
    cache.note_pasted(&newer, &[a.clone()]);
    assert!(
        cache.is_cut(&b),
        "a paste of an older snapshot must not clear a newer intent"
    );

    cache.note_pasted(&cut, &[b.clone()]);
    assert!(!cache.has_cut(), "moving every source clears the intent");
}

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
fn cut_validation_adopts_republished_paths_and_clears_replaced_content() {
    let a = PathBuf::from("/tmp/reconcile-a");
    let b = PathBuf::from("/tmp/reconcile-b");
    let cut = Snapshot::new(Mode::Cut, vec![a.clone(), b.clone()]).expect("cut snapshot");
    let mut cache = Cache::default();
    cache.record_write(&cut);
    // A read started before the paste is still in flight when the kept paths
    // change; the changed intent must be able to validate on its own.
    let stale_generation = cache.begin_refresh().expect("validation read");
    cache.note_pasted(&cut, &[a.clone()]);
    let generation = cache.begin_refresh().expect("validation read");

    // A partial cut paste republishes the remaining paths with a fresh identity.
    let republished = Snapshot::new(Mode::Cut, vec![b.clone()]).expect("republished cut");
    assert!(!cache.reconcile(stale_generation, Ok(Some(cut.clone())), false));
    assert!(
        cache.begin_refresh().is_none(),
        "the stale completion must not release the newer read"
    );
    cache.reconcile(generation, Ok(Some(republished)), false);
    assert!(cache.is_cut(&b) && !cache.is_cut(&a));

    let generation = cache.begin_refresh().expect("validation read");
    assert!(cache.reconcile(generation, Ok(None), false));
    assert!(!cache.has_cut(), "ordinary text ends the cut intent");

    cache.record_write(&cut);
    let generation = cache.begin_refresh().expect("validation read");
    assert!(!cache.reconcile(generation, Err(Error::Changed), false));
    assert!(
        cache.has_cut(),
        "an unreadable clipboard is not evidence of replacement"
    );
}

#[test]
fn copy_contents_refresh_after_external_replacement_and_text_clears_the_summary() {
    let copy = Snapshot::new(Mode::Copy, vec![PathBuf::from("/tmp/copied")]).expect("copy");
    let mut cache = Cache::default();
    assert!(!cache.can_paste());
    let generation = cache
        .begin_refresh()
        .expect("initial read without a cached cut");
    assert!(cache.reconcile(generation, Ok(Some(copy.clone())), false));
    assert_eq!(cache.current().as_deref(), Some(&copy));
    assert!(cache.can_paste());
    assert!(!cache.has_cut());

    let cut = Snapshot::new(Mode::Cut, vec![PathBuf::from("/tmp/new-cut")]).expect("cut");
    let generation = cache.begin_refresh().expect("refresh copied contents");
    assert!(cache.reconcile(generation, Ok(Some(cut.clone())), false));
    assert_eq!(cache.current().as_deref(), Some(&cut));
    assert!(cache.is_cut(&cut.paths[0]));

    let generation = cache.begin_refresh().expect("text replacement");
    assert!(cache.reconcile(generation, Ok(None), false));
    assert!(cache.current().is_none());
    assert!(!cache.has_cut());
    assert!(!cache.can_paste());

    let stale = cache.begin_refresh().expect("read before remote copy");
    cache.record_uri_copy(true);
    assert!(cache.can_paste());
    assert!(cache.current().is_none());
    assert!(!cache.reconcile(stale, Ok(None), false));
    assert!(
        cache.can_paste(),
        "an older read cannot disable a new remote copy"
    );
    let generation = cache.begin_refresh().expect("text replaces remote copy");
    assert!(cache.reconcile(generation, Ok(None), false));
    assert!(!cache.can_paste());
}

#[test]
fn cancelling_cut_invalidates_old_reads_and_clears_both_summary_and_markers() {
    let cut = Snapshot::new(Mode::Cut, vec![PathBuf::from("/tmp/cancel")]).expect("cut");
    let mut cache = Cache::default();
    cache.record_write(&cut);
    let generation = cache.begin_refresh().expect("read before Escape");
    let expected = cache.begin_cancel().expect("cancel owned cut");
    assert!(cache.begin_cancel().is_none());
    assert!(cache.begin_refresh().is_none());
    assert!(!cache.reconcile(generation, Ok(Some(cut.clone())), false));
    cache.finish_cancel(&expected, true);
    assert!(cache.current().is_none());
    assert!(!cache.is_cut(&cut.paths[0]));
    assert!(!cache.reconcile(generation, Ok(Some(cut)), false));
}

#[test]
fn failed_cancellation_can_retry_and_a_later_copy_survives_older_cleanup() {
    let cut = Snapshot::new(Mode::Cut, vec![PathBuf::from("/tmp/kept")]).expect("cut");
    let mut cache = Cache::default();
    cache.record_write(&cut);
    let expected = cache.begin_cancel().expect("first Escape");
    cache.finish_cancel(&expected, false);
    assert_eq!(cache.current().as_deref(), Some(&cut));
    let expected = cache.begin_cancel().expect("retry after clipboard error");

    let copy = Snapshot::new(Mode::Copy, cut.paths.clone()).expect("later copy of same paths");
    cache.record_write(&copy);
    cache.finish_cancel(&expected, true);
    cache.note_pasted(&cut, &cut.paths);
    assert_eq!(cache.current().as_deref(), Some(&copy));
    assert!(
        cache.begin_cancel().is_none(),
        "Escape keeps copies available"
    );
}

#[test]
fn foreign_cut_contents_can_be_shown_without_claiming_cancellation_ownership() {
    let foreign = Snapshot {
        mode: Mode::Cut,
        paths: vec![PathBuf::from("/tmp/foreign")],
        identity: Identity(None),
    };
    let mut cache = Cache::default();
    let generation = cache.begin_refresh().expect("external file offer");
    cache.reconcile(generation, Ok(Some(foreign.clone())), false);
    assert_eq!(cache.current().as_deref(), Some(&foreign));
    assert!(cache.begin_cancel().is_none());
}

#[test]
fn remote_uri_offers_preserve_local_cut_intent_and_do_not_promote_ordinary_text() {
    let text = ClipboardItem::new_string("sftp://host/team/file.txt".into());
    assert!(
        fallback::Clipboard::decode_uris(Some(&text))
            .expect("plain text")
            .is_none()
    );
    let remote = ClipboardItem::new_string_with_metadata(
        "sftp://alice@host/team/a%20b.txt".into(),
        super::REMOTE_URI_METADATA.into(),
    );
    let (mode, uris) = fallback::Clipboard::decode_uris(Some(&remote))
        .expect("remote offer")
        .expect("files");
    assert_eq!(mode, Mode::Copy);
    assert_eq!(uris, ["sftp://alice@host/team/a%20b.txt"]);
    assert!(copyable_uris(&uris));
    assert!(!copyable_uris(&["https://example.com/file.txt".into()]));
    let cut =
        Snapshot::new(Mode::Cut, vec![PathBuf::from("/tmp/中文 空格.txt")]).expect("selection");
    let encoded = fallback::Clipboard::encode(&cut).expect("private local cut");
    let (mode, uris) = fallback::Clipboard::decode_uris(Some(&encoded))
        .expect("decode")
        .expect("files");
    assert_eq!(mode, Mode::Cut);
    assert_eq!(
        url::Url::parse(&uris[0])
            .expect("URI")
            .to_file_path()
            .expect("path"),
        cut.paths[0]
    );
    let secret = ClipboardItem::new_string_with_metadata(
        "sftp://alice:secret@host/file".into(),
        super::REMOTE_URI_METADATA.into(),
    );
    assert!(fallback::Clipboard::decode_uris(Some(&secret)).is_err());
}
