use super::*;

#[test]
fn mime_offers_preserve_intent_identity_and_ignore_plain_uri_text() {
    let snapshot =
        Snapshot::new(Mode::Cut, vec![PathBuf::from("/tmp/a b#%.txt")]).expect("selection");
    let entries = Clipboard::entries(
        Formats::encode(&snapshot).expect("formats"),
        &snapshot.identity,
    );
    let mut offer = ClipboardMimeOffer { id: 7, entries };
    assert_eq!(
        Clipboard::decode(&offer).expect("owned cut"),
        Some(snapshot.clone())
    );
    offer
        .entries
        .retain(|entry| entry.mime_type != CLIPBOARD_ID);
    let foreign = Clipboard::decode(&offer)
        .expect("GNOME cut")
        .expect("files");
    assert_eq!(foreign.mode, Mode::Cut);
    assert_eq!(foreign.paths, snapshot.paths);
    assert!(!foreign.can_cancel());
    offer.entries.retain(|entry| entry.mime_type != GNOME_FILES);
    assert_eq!(Clipboard::decode(&offer).expect("KDE cut"), Some(foreign));
    offer.entries = vec![ClipboardMime {
        mime_type: "text/plain".into(),
        bytes: b"file:///tmp/a".to_vec(),
    }];
    assert!(Clipboard::decode(&offer).expect("ordinary text").is_none());
}

#[test]
fn remote_file_offers_preserve_cut_intent_without_becoming_local_paths() {
    let offer = ClipboardMimeOffer {
        id: 4,
        entries: vec![ClipboardMime {
            mime_type: GNOME_FILES.into(),
            bytes: b"cut\nsftp://example.com/home/user/a%20b".to_vec(),
        }],
    };
    let (mode, uris) = Clipboard::decode_uris(&offer)
        .expect("URI format")
        .expect("offer");
    assert_eq!(mode, Mode::Cut);
    assert_eq!(uris, ["sftp://example.com/home/user/a%20b"]);
    assert!(matches!(
        Clipboard::decode(&offer),
        Err(Error::InvalidUri(_))
    ));
}
