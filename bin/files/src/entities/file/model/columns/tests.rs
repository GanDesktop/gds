use super::{Column, Layout};

fn visible(layout: &Layout) -> Vec<Column> {
    layout.columns().to_vec()
}

#[test]
fn default_layout_keeps_the_existing_visible_columns() {
    assert_eq!(
        visible(&Layout::default()),
        vec![Column::Name, Column::Modified, Column::Type, Column::Size,]
    );
}

#[test]
fn normalization_keeps_name_visible_and_removes_duplicates() {
    let layout = Layout::new([Column::Size, Column::Name, Column::Size, Column::Owner]);
    assert_eq!(
        visible(&layout),
        vec![Column::Size, Column::Name, Column::Owner]
    );

    let layout = Layout::new([Column::Size, Column::Owner]);
    assert_eq!(
        visible(&layout),
        vec![Column::Name, Column::Size, Column::Owner]
    );

    let layout = Layout::new(std::iter::empty());
    assert_eq!(visible(&layout), vec![Column::Name]);
}

#[test]
fn showing_hiding_and_moving_keep_the_visibility_invariant() {
    let mut layout = Layout::default();
    assert!(!layout.hide(Column::Name));
    assert!(layout.hide(Column::Type));
    assert!(!layout.hide(Column::Type));
    assert!(layout.show(Column::Owner));
    assert!(!layout.show(Column::Owner));
    assert_eq!(
        visible(&layout),
        vec![Column::Name, Column::Modified, Column::Size, Column::Owner,]
    );

    assert!(layout.move_to(Column::Owner, 1));
    assert_eq!(
        visible(&layout),
        vec![Column::Name, Column::Owner, Column::Modified, Column::Size,]
    );
    assert!(!layout.move_to(Column::Owner, 1));
    assert!(layout.move_to(Column::Name, usize::MAX));
    assert_eq!(
        visible(&layout),
        vec![Column::Owner, Column::Modified, Column::Size, Column::Name,]
    );
    assert!(!layout.move_to(Column::Type, 0));

    assert!(layout.set_visible(Column::Type, true));
    assert!(layout.set_visible(Column::Type, false));
    assert!(!layout.set_visible(Column::Name, false));
    assert!(layout.contains(Column::Name));
}

#[test]
fn serialization_uses_stable_ids_and_tolerates_unknown_entries() {
    let layout = Layout::new([Column::Name, Column::DetailedType, Column::Accessed]);
    let encoded = serde_json::to_string(&layout).expect("serialize layout");
    assert_eq!(encoded, r#"["name","detailed-type","accessed"]"#);
    assert_eq!(
        serde_json::from_str::<Layout>(&encoded).expect("deserialize layout"),
        layout
    );

    let recovered = serde_json::from_str::<Layout>(r#"["size","unknown","size","name"]"#)
        .expect("ignore unknown and duplicate columns");
    assert_eq!(
        recovered.columns().to_vec(),
        vec![Column::Size, Column::Name]
    );

    let ids: Vec<_> = Column::ALL.iter().map(|column| column.id()).collect();
    assert_eq!(
        ids,
        [
            "name",
            "type",
            "size",
            "owner",
            "group",
            "permissions",
            "modified",
            "accessed",
            "detailed-type",
        ]
    );
}
