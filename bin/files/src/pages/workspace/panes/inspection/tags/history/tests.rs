use super::*;

#[test]
fn undo_tag_creation_restores_draft_and_redo_restores_chip() {
    let draft = Snapshot {
        tags: vec!["工作".into()],
        draft: "设计".into(),
        selection: 6..6,
        reversed: false,
    };
    let mut history = History::new(draft.clone());
    let added = Snapshot {
        tags: vec!["工作".into(), "设计".into()],
        ..Default::default()
    };
    history.record(added.clone(), false);
    assert_eq!(history.step(false), Some(&draft));
    assert_eq!(history.step(true), Some(&added));
    history.record(
        Snapshot {
            tags: vec!["工作".into()],
            ..Default::default()
        },
        false,
    );
    assert_eq!(history.step(false), Some(&added));
    history.record(
        Snapshot {
            tags: vec!["设计".into()],
            ..Default::default()
        },
        false,
    );
    assert_eq!(history.step(true), None);
}

#[test]
fn text_and_tag_changes_share_the_same_undo_order() {
    let mut history = History::with_tags(vec!["existing".into()]);
    record_draft(&mut history, "d");
    record_draft(&mut history, "dr");
    record_draft(&mut history, "draft");
    let typed = Snapshot {
        tags: vec!["existing".into()],
        draft: "draft".into(),
        selection: 5..5,
        reversed: false,
    };
    let mut removed = typed.clone();
    removed.tags.clear();
    history.record(removed, false);
    assert_eq!(history.step(false), Some(&typed));
    let initial = history.step(false).expect("undo back to initial tags");
    assert_eq!(initial.draft, "");
    assert_eq!(initial.tags, ["existing"]);
    assert_eq!(history.step(true), Some(&typed));
}

#[test]
fn break_typing_splits_undo_groups_and_ignores_unchanged_snapshots() {
    let mut history = History::default();
    assert_eq!(history.step(false), None);
    assert_eq!(history.step(true), None);

    record_draft(&mut history, "ab");
    history.break_typing();
    record_draft(&mut history, "abcd");

    assert_eq!(history.undo_depth(), 2);
    let after_undo = history.step(false).expect("undo second typing group");
    assert_eq!(after_undo.draft, "ab");

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
    assert_eq!(history.undo_depth(), 1);
    let after_redo = history
        .step(true)
        .expect("redo preserved after metadata-only edit");
    assert_eq!(after_redo.draft, "abcd");
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
