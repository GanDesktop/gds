use super::*;

#[test]
fn undo_tag_creation_restores_draft_and_redo_restores_chip() {
    let mut history = History::default();
    let draft = Snapshot {
        tags: vec!["工作".into()],
        draft: "设计".into(),
        selection: 6..6,
        reversed: false,
    };
    history.current = draft.clone();
    let added = Snapshot {
        tags: vec!["工作".into(), "设计".into()],
        ..Default::default()
    };
    history.record(added.clone(), false);
    assert!(history.step(false));
    assert_eq!(history.current, draft);
    assert!(history.step(true));
    assert_eq!(history.current, added);
    history.record(
        Snapshot {
            tags: vec!["工作".into()],
            ..Default::default()
        },
        false,
    );
    assert!(history.step(false));
    assert_eq!(history.current, added);
    history.record(
        Snapshot {
            tags: vec!["设计".into()],
            ..Default::default()
        },
        false,
    );
    assert!(!history.step(true));
}

#[test]
fn text_and_tag_changes_share_the_same_undo_order() {
    let mut history = History::default();
    history.current.tags.push("existing".into());
    record_draft(&mut history, "d");
    record_draft(&mut history, "dr");
    record_draft(&mut history, "draft");
    let typed = history.current.clone();
    let mut removed = typed.clone();
    removed.tags.clear();
    history.record(removed, false);
    assert!(history.step(false));
    assert_eq!(history.current, typed);
    assert!(history.step(false));
    assert_eq!(history.current.draft, "");
    assert_eq!(history.current.tags, ["existing"]);
    assert!(history.step(true));
    assert_eq!(history.current, typed);
}

#[test]
fn break_typing_splits_undo_groups_and_ignores_unchanged_snapshots() {
    let mut history = History::default();
    assert!(!history.step(false));
    assert!(!history.step(true));

    record_draft(&mut history, "ab");
    history.break_typing();
    record_draft(&mut history, "abcd");

    assert_eq!(history.undo.len(), 2);
    assert!(history.step(false));
    assert_eq!(history.current.draft, "ab");

    // Same tags & draft only updates selection/metadata without pushing undo or clearing redo.
    history.record(
        Snapshot {
            tags: vec!["existing".into()],
            draft: "ab".into(),
            selection: 0..2,
            reversed: true,
        },
        false,
    );
    assert_eq!(history.undo.len(), 1);
    assert!(history.step(true));
    assert_eq!(history.current.draft, "abcd");
}

fn record_draft(history: &mut History, draft: &str) {
    history.record(
        Snapshot {
            tags: vec!["existing".into()],
            draft: draft.into(),
            selection: draft.len()..draft.len(),
            reversed: false,
        },
        true,
    );
}

#[test]
fn parse_draft_tags_splits_comma_delimited_drafts_and_deduplicates() {
    assert_eq!(
        super::super::parse_draft_tags("   ").expect("whitespace draft"),
        Vec::<String>::new()
    );
    assert_eq!(
        super::super::parse_draft_tags("work, urgent , work, , client ")
            .expect("comma-delimited tags"),
        vec!["work".to_owned(), "urgent".to_owned(), "client".to_owned()]
    );
    assert!(super::super::parse_draft_tags("bad\ntag").is_err());
    assert!(super::super::parse_draft_tags("bad\0tag").is_err());
}
