use super::{View, table::Files};
use gds::service::remote::{Address, Entry, EntryKind};

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
    ];

    let visible = View::filter_entries(&entries, "", false);
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].name, "Readme.md");
    assert_eq!(visible[1].name, "notes.txt");

    let with_hidden = View::filter_entries(&entries, "", true);
    assert_eq!(with_hidden.len(), 3);

    let filtered = View::filter_entries(&entries, "READ", false);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "Readme.md");
}
