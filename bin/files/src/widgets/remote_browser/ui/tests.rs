use super::{View, table::Files};
use crate::shared::lib::selection::Selection;
use gds::service::remote::{Address, Entry, EntryKind};
use std::sync::Arc;

fn sample_entry(path: &str, name: &str) -> Entry {
    Entry {
        address: Address::parse(path).expect("valid remote address"),
        name: name.to_owned(),
        kind: EntryKind::File,
        size: Some(64),
        modified: None,
    }
}

#[test]
fn format_size_uses_binary_units() {
    assert_eq!(Files::format_size(512), "512 B");
    assert_eq!(Files::format_size(2048), "2.0 KiB");
    assert_eq!(Files::format_size(3 * 1024 * 1024), "3.0 MiB");
    assert_eq!(Files::format_size(2 * 1024 * 1024 * 1024), "2.0 GiB");
}

#[test]
fn filter_entries_respects_show_hidden_and_case_insensitive_query() {
    let entries = vec![
        sample_entry("sftp://example.com/.secret", ".secret"),
        sample_entry("sftp://example.com/Readme.md", "Readme.md"),
        sample_entry("sftp://example.com/notes.txt", "notes.txt"),
        sample_entry("sftp://example.com/%C3%9Cber.txt", "Über.txt"),
    ];

    let visible = View::filter_entries(&entries, "   ", false);
    assert_eq!(visible.len(), 3);
    assert_eq!(visible[0].name, "Readme.md");
    assert_eq!(visible[1].name, "notes.txt");
    assert_eq!(visible[2].name, "Über.txt");

    let with_hidden = View::filter_entries(&entries, "", true);
    assert_eq!(with_hidden.len(), 4);

    let ascii_filtered = View::filter_entries(&entries, "READ", false);
    assert_eq!(ascii_filtered.len(), 1);
    assert_eq!(ascii_filtered[0].name, "Readme.md");

    let unicode_filtered = View::filter_entries(&entries, "über", false);
    assert_eq!(unicode_filtered.len(), 1);
    assert_eq!(unicode_filtered[0].name, "Über.txt");
}

#[test]
fn selection_modes_preserve_row_slice_and_filter_prunes_hidden_entries() {
    let entries = vec![
        sample_entry("sftp://example.com/.env", ".env"),
        sample_entry("sftp://example.com/alpha.txt", "alpha.txt"),
        sample_entry("sftp://example.com/beta.txt", "beta.txt"),
        sample_entry("sftp://example.com/gamma.log", "gamma.log"),
    ];
    let rows = View::filter_entries(&entries, "", true);
    let rows_snapshot = Arc::clone(&rows);
    let mut selected = Selection::new();
    let mut anchor = None;

    // Shift selection when anchor is initially None establishes anchor at clicked index.
    View::apply_selection(&rows, &mut selected, &mut anchor, 2, true, false);
    assert_eq!(anchor, Some(2));
    assert_eq!(selected.to_vec(), vec![rows[2].address.clone()]);

    // Single selection sets anchor and 1 item without touching the shared row slice.
    View::apply_selection(&rows, &mut selected, &mut anchor, 1, false, false);
    assert!(Arc::ptr_eq(&rows, &rows_snapshot));
    assert_eq!(anchor, Some(1));
    assert_eq!(selected.to_vec(), vec![rows[1].address.clone()]);

    let single_snapshot = selected.clone();
    View::apply_selection(&rows, &mut selected, &mut anchor, 1, false, false);
    assert!(selected.ptr_eq(&single_snapshot));

    // Ctrl/Cmd toggle adds and removes items while preserving insertion order.
    View::apply_selection(&rows, &mut selected, &mut anchor, 3, false, true);
    assert_eq!(anchor, Some(3));
    assert_eq!(
        selected.to_vec(),
        vec![rows[1].address.clone(), rows[3].address.clone()]
    );
    View::apply_selection(&rows, &mut selected, &mut anchor, 1, false, true);
    assert_eq!(selected.to_vec(), vec![rows[3].address.clone()]);

    // Shift range selection selects the inclusive span from the current anchor.
    anchor = Some(0);
    View::apply_selection(&rows, &mut selected, &mut anchor, 2, true, false);
    assert!(Arc::ptr_eq(&rows, &rows_snapshot));
    assert_eq!(
        selected.to_vec(),
        vec![
            rows[0].address.clone(),
            rows[1].address.clone(),
            rows[2].address.clone(),
        ]
    );

    // Retaining when all selected entries remain visible preserves the Selection Arc.
    let before_noop_prune = selected.clone();
    View::retain_visible_selection(&rows, &mut selected);
    assert!(selected.ptr_eq(&before_noop_prune));

    // Hiding dotfiles and filtering by query prunes hidden and unmatched selections.
    let visible_without_hidden = View::filter_entries(&entries, "txt", false);
    View::retain_visible_selection(&visible_without_hidden, &mut selected);
    assert_eq!(
        selected.to_vec(),
        vec![rows[1].address.clone(), rows[2].address.clone()]
    );
    assert!(!selected.contains(&rows[0].address));
}

#[cfg(feature = "gui-tests")]
#[gpui_kit::test]
async fn table_sync_shares_rows_across_selection_modes_and_prunes_filtered_items(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gds::app::AppExt as _;
    use gpui_kit::{AppContext, WeakEntity, px, size};

    cx.update(|cx| {
        gpui_kit::init(cx);
        let catalog = crate::shared::i18n::prepare(gds::i18n::Language::English)
            .expect("prepare English translations");
        cx.gds_mut().i18n().install_catalog(catalog);
        cx.gds_mut()
            .appearance()
            .theme()
            .apply(&Default::default())
            .expect("apply default theme");
    });

    let entries = vec![
        sample_entry("sftp://example.com/.hidden", ".hidden"),
        sample_entry("sftp://example.com/a.txt", "a.txt"),
        sample_entry("sftp://example.com/b.txt", "b.txt"),
        sample_entry("sftp://example.com/c.md", "c.md"),
    ];
    let rows = View::filter_entries(&entries, "", true);
    let mut table_entity = None;
    let window = cx.open_window(size(px(800.), px(500.)), |window, cx| {
        let state = Files::state(WeakEntity::new_invalid(), window, cx);
        table_entity = Some(state.clone());
        gpui_kit::component::Root::new(state, window, cx)
    });
    let table = table_entity.expect("table state created");

    cx.update_window(window.into(), |_, _, cx| {
        let mut selected = Selection::new();
        let mut anchor = None;

        table.update(cx, |state, cx| {
            View::sync_table_state(state, Arc::clone(&rows), selected.clone(), cx);
            assert!(Arc::ptr_eq(&state.delegate().rows, &rows));
            assert!(state.delegate().selected.is_empty());
        });

        // Single, Ctrl, and Shift selections share the exact `Arc<[Entry]>` in the table delegate.
        View::apply_selection(&rows, &mut selected, &mut anchor, 1, false, false);
        table.update(cx, |state, cx| {
            View::sync_table_state(state, Arc::clone(&rows), selected.clone(), cx);
            assert!(Arc::ptr_eq(&state.delegate().rows, &rows));
            assert!(state.delegate().selected.ptr_eq(&selected));
        });

        View::apply_selection(&rows, &mut selected, &mut anchor, 3, false, true);
        table.update(cx, |state, cx| {
            View::sync_table_state(state, Arc::clone(&rows), selected.clone(), cx);
            assert!(Arc::ptr_eq(&state.delegate().rows, &rows));
            assert_eq!(
                state.delegate().selected.to_vec(),
                vec![rows[1].address.clone(), rows[3].address.clone()]
            );
        });

        anchor = Some(0);
        View::apply_selection(&rows, &mut selected, &mut anchor, 2, true, false);
        table.update(cx, |state, cx| {
            View::sync_table_state(state, Arc::clone(&rows), selected.clone(), cx);
            assert!(Arc::ptr_eq(&state.delegate().rows, &rows));
            assert_eq!(state.delegate().selected.len(), 3);
        });

        // Filtering replaces the row slice and prunes hidden selections.
        let filtered_rows = View::filter_entries(&entries, "txt", false);
        View::retain_visible_selection(&filtered_rows, &mut selected);
        table.update(cx, |state, cx| {
            View::sync_table_state(state, Arc::clone(&filtered_rows), selected.clone(), cx);
            assert!(Arc::ptr_eq(&state.delegate().rows, &filtered_rows));
            assert_eq!(
                state.delegate().selected.to_vec(),
                vec![rows[1].address.clone(), rows[2].address.clone()]
            );
        });
    })
    .expect("verify table state sync");

    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
}
