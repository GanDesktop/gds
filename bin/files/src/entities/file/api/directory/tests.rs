use super::*;

#[test]
fn disappeared_entry_is_retained_with_a_warning_but_inspection_fails() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("file");
    fs::write(&path, "content").expect("write file");
    let entry = fs::read_dir(root.path())
        .expect("read directory")
        .next()
        .expect("single entry")
        .expect("directory entry");
    fs::remove_file(&path).expect("remove file before inspection");

    let mut warnings = Vec::new();
    let entry = Entry::from_dir_entry(entry, &mut IdentityCache::new(), &mut warnings);
    assert_eq!(entry.path, path);
    assert_eq!(entry.name, "file");
    assert_eq!(entry.size, None);
    assert_eq!(entry.modified, None);
    assert_eq!(entry.details, EntryDetails::default());
    assert!(warnings.iter().any(|warning| {
        warning.path == path
            && warning.operation == WarningOperation::Metadata
            && warning.source.kind() == io::ErrorKind::NotFound
    }));
    assert_eq!(
        Entry::inspect(&path)
            .expect_err("missing entry must fail inspection")
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn symlink_targets_preserve_file_directory_broken_and_unreachable_states() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create fixture directory");
    fs::write(root.path().join("source.rs"), "fn main() {}\n").expect("write source file");
    fs::create_dir(root.path().join("folder")).expect("create target directory");
    symlink("source.rs", root.path().join("file-link.png")).expect("create file symlink");
    symlink("folder", root.path().join("folder-link")).expect("create directory symlink");
    symlink("missing", root.path().join("broken-link")).expect("create broken symlink");
    symlink("loop-link", root.path().join("loop-link")).expect("create loop symlink");

    let snapshot =
        read_directory(root.path(), ReadOptions::default()).expect("read directory snapshot");
    assert!(snapshot.warnings.is_empty());

    let file_link = assert_symlink_entry(&snapshot, root.path(), "file-link.png");
    assert_eq!(
        file_link.details.target,
        Some(LinkTarget::Resolved {
            kind: EntryKind::File,
            content: Some(ContentType::new(
                ContentCategory::Code,
                Some("Rust"),
                Some("text/rust"),
            )),
        })
    );
    assert_eq!(
        file_link.details.display_content(),
        Some(&ContentType::new(
            ContentCategory::Code,
            Some("Rust"),
            Some("text/rust"),
        ))
    );

    let folder_link = assert_symlink_entry(&snapshot, root.path(), "folder-link");
    assert_eq!(
        folder_link.details.target,
        Some(LinkTarget::Resolved {
            kind: EntryKind::Directory,
            content: None,
        })
    );

    let broken_link = assert_symlink_entry(&snapshot, root.path(), "broken-link");
    assert_eq!(broken_link.details.target, Some(LinkTarget::Broken));

    let loop_link = assert_symlink_entry(&snapshot, root.path(), "loop-link");
    assert_eq!(loop_link.details.target, Some(LinkTarget::Unreachable));
}

#[cfg(unix)]
fn assert_symlink_entry<'a>(snapshot: &'a DirectorySnapshot, root: &Path, name: &str) -> &'a Entry {
    let listed = snapshot
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .expect("listed symlink entry");
    let inspected = Entry::inspect(&root.join(name)).expect("inspect symlink entry");
    assert_eq!(listed.kind, EntryKind::SymbolicLink);
    assert_eq!(inspected.kind, EntryKind::SymbolicLink);
    assert_eq!(listed.details.target, inspected.details.target);
    listed
}

#[test]
fn inspection_preserves_root_names_and_listing_filters_hidden_entries() {
    let root = tempfile::tempdir().expect("create fixture directory");
    fs::write(root.path().join(".hidden"), "content").expect("write hidden file");
    fs::create_dir(root.path().join("folder")).expect("create folder");
    let snapshot =
        read_directory(root.path(), ReadOptions::default()).expect("read directory snapshot");
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].kind, EntryKind::Directory);
    assert_eq!(
        snapshot.entries[0].details.children,
        Some(ChildCount {
            count: 0,
            truncated: false
        })
    );
    let hidden = Entry::inspect(&root.path().join(".hidden")).expect("inspect hidden file");
    assert!(hidden.is_hidden);
    assert_eq!(hidden.size, Some(7));

    let filesystem_root = root
        .path()
        .ancestors()
        .last()
        .expect("filesystem root ancestor");
    let entry = Entry::inspect(filesystem_root).expect("inspect filesystem root");
    assert_eq!(entry.name, filesystem_root.as_os_str());
    assert_eq!(entry.kind, EntryKind::Directory);
}

#[test]
fn filter_matching_handles_empty_ascii_and_unicode_case_folding() {
    let ascii = Entry::new(
        PathBuf::from("/tmp/ReadMe.MD"),
        OsString::from("ReadMe.MD"),
        EntryKind::File,
        Some(12),
        None,
        EntryDetails::default(),
    );
    assert!(ascii.matches_filter(""));
    assert!(ascii.matches_filter("readme"));
    assert!(ascii.matches_filter("me.md"));
    assert!(!ascii.matches_filter("cargo"));

    let unicode = Entry::new(
        PathBuf::from("/tmp/Übersicht_Файл.txt"),
        OsString::from("Übersicht_Файл.txt"),
        EntryKind::File,
        Some(24),
        None,
        EntryDetails::default(),
    );
    assert!(unicode.matches_filter(""));
    assert!(unicode.matches_filter("übersicht"));
    assert!(unicode.matches_filter("файл"));
    assert!(!unicode.matches_filter("missing"));
}

#[test]
fn entry_for_path_finds_directories_and_files_in_canonical_order() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let dir_a = root.path().join("alpha-dir");
    let dir_b = root.path().join("zeta-dir");
    let file_a = root.path().join("alpha.txt");
    let file_b = root.path().join("zeta.txt");
    fs::create_dir(&dir_a).expect("create alpha-dir");
    fs::create_dir(&dir_b).expect("create zeta-dir");
    fs::write(&file_a, "a").expect("write alpha.txt");
    fs::write(&file_b, "b").expect("write zeta.txt");

    let snapshot =
        read_directory(root.path(), ReadOptions::default()).expect("read directory snapshot");
    assert_eq!(
        snapshot.entry_for_path(&dir_a).map(|entry| entry.kind),
        Some(EntryKind::Directory)
    );
    assert_eq!(
        snapshot.entry_for_path(&dir_b).map(|entry| entry.kind),
        Some(EntryKind::Directory)
    );
    assert_eq!(
        snapshot.entry_for_path(&file_a).map(|entry| entry.kind),
        Some(EntryKind::File)
    );
    assert_eq!(
        snapshot.entry_for_path(&file_b).map(|entry| entry.kind),
        Some(EntryKind::File)
    );
    assert!(
        snapshot
            .entry_for_path(&root.path().join("absent"))
            .is_none()
    );
}

#[test]
fn base_read_skips_secondary_probes_and_honors_cancellation() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let folder = root.path().join("folder");
    fs::create_dir(&folder).expect("create folder");
    fs::write(folder.join("child.txt"), "child").expect("write child");

    let known_file = root.path().join("lib.rs");
    fs::write(&known_file, "pub fn main() {}\n").expect("write known file");

    let unknown_file = root.path().join(" mystery-blob");
    fs::write(&unknown_file, b"\x89PNG\r\n\x1a\nheader").expect("write unknown-name file");

    let cancel = CancellationToken::new();
    let snapshot = read_directory_base(root.path(), ReadOptions::default(), &cancel)
        .expect("read base directory snapshot");

    let folder_entry = snapshot.entry_for_path(&folder).expect("folder entry");
    assert_eq!(folder_entry.details.children, None);
    assert!(folder_entry.needs_enrichment(EnrichmentNeed {
        children: true,
        ..EnrichmentNeed::default()
    }));

    let known_entry = snapshot
        .entry_for_path(&known_file)
        .expect("known file entry");
    assert_eq!(
        known_entry
            .details
            .content
            .as_ref()
            .and_then(|content| content.mime.as_deref()),
        Some("text/rust")
    );
    assert!(!known_entry.needs_enrichment(EnrichmentNeed {
        content: true,
        ..EnrichmentNeed::default()
    }));

    let unknown_entry = snapshot
        .entry_for_path(&unknown_file)
        .expect("unknown file entry");
    assert_eq!(unknown_entry.details.content, None);
    assert!(unknown_entry.needs_enrichment(EnrichmentNeed {
        content: true,
        ..EnrichmentNeed::default()
    }));

    cancel.cancel();
    let error = read_directory_base(root.path(), ReadOptions::default(), &cancel)
        .expect_err("cancelled base read must fail");
    assert!(error.is_cancelled());
}

#[test]
fn enrichment_prioritizes_viewport_bounds_budget_and_marks_unreadable_entries_probed() {
    let root = tempfile::tempdir().expect("create fixture directory");
    (0..DEFAULT_PROBE_BUDGET + 10).for_each(|index| {
        let dir = root.path().join(format!("dir-{index:04}"));
        fs::create_dir(&dir).expect("create numbered directory");
    });
    let priority_dir = root.path().join("z-priority-dir");
    fs::create_dir(&priority_dir).expect("create priority directory");
    fs::write(priority_dir.join("inside.txt"), "1").expect("write priority child");

    let cancel = CancellationToken::new();
    let base = Arc::new(
        read_directory_base(root.path(), ReadOptions::default(), &cancel)
            .expect("read base snapshot"),
    );

    let bounded_need = EnrichmentNeed {
        children: true,
        content: false,
        symlinks: false,
        budget: ProbeBudget::default(),
    };
    let planned = plan_enrichment(&base, std::slice::from_ref(&priority_dir), bounded_need);
    assert_eq!(planned.len(), 1 + DEFAULT_PROBE_BUDGET);
    assert_eq!(planned[0].path, priority_dir);

    let custom_need = EnrichmentNeed {
        budget: ProbeBudget::Bounded(0),
        ..bounded_need
    };
    let viewport_only = plan_enrichment(&base, std::slice::from_ref(&priority_dir), custom_need);
    assert_eq!(viewport_only.len(), 1);
    assert_eq!(viewport_only[0].path, priority_dir);
    assert!(custom_need.is_satisfied_by(bounded_need));
    assert!(!bounded_need.is_satisfied_by(custom_need));

    let full_need = EnrichmentNeed {
        budget: ProbeBudget::Full,
        ..bounded_need
    };
    assert!(bounded_need.is_satisfied_by(full_need));
    assert!(!full_need.is_satisfied_by(bounded_need));
    assert_eq!(custom_need.union(bounded_need), bounded_need);
    assert_eq!(bounded_need.union(full_need), full_need);
    let all_planned = plan_enrichment(&base, std::slice::from_ref(&priority_dir), full_need);
    assert_eq!(all_planned.len(), DEFAULT_PROBE_BUDGET + 11);

    let probed =
        probe_batch(&planned[..1], bounded_need, &cancel).expect("probe priority directory");
    let (merged, changed_visible) =
        merge_enriched(&base, &probed).expect("merge priority directory update");
    assert!(changed_visible);
    assert_eq!(
        merged
            .entry_for_path(&priority_dir)
            .and_then(|entry| entry.details.children),
        Some(ChildCount {
            count: 1,
            truncated: false,
        })
    );

    // A directory whose child count cannot be read stays `None` in the UI,
    // records `children_probed = true`, and is not re-planned on subsequent passes.
    let vanished_dir = root.path().join("dir-0000");
    fs::remove_dir(&vanished_dir).expect("remove directory before child probe");
    let vanished_entry = merged
        .entry_for_path(&vanished_dir)
        .expect("vanished directory still in snapshot")
        .clone();
    let vanished_probed = probe_batch(std::slice::from_ref(&vanished_entry), bounded_need, &cancel)
        .expect("probe vanished directory");
    let (merged_after_vanished, vanished_visible_change) =
        merge_enriched(&merged, &vanished_probed).expect("merge vanished directory probe state");
    assert!(!vanished_visible_change);
    let updated_vanished = merged_after_vanished
        .entry_for_path(&vanished_dir)
        .expect("vanished entry after merge");
    assert_eq!(updated_vanished.details.children, None);
    assert!(!updated_vanished.needs_enrichment(bounded_need));
    assert!(
        plan_enrichment(
            &merged_after_vanished,
            std::slice::from_ref(&vanished_dir),
            EnrichmentNeed {
                children: true,
                ..EnrichmentNeed::default()
            },
        )
        .iter()
        .all(|entry| entry.path != vanished_dir)
    );
}

#[test]
fn merge_enriched_in_place_reuses_unique_snapshot_and_falls_back_to_cow_when_shared() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let dir_a = root.path().join("dir-a");
    let dir_b = root.path().join("dir-b");
    fs::create_dir(&dir_a).expect("create dir-a");
    fs::create_dir(&dir_b).expect("create dir-b");
    fs::write(dir_a.join("inside.txt"), "1").expect("write child");

    let cancel = CancellationToken::new();
    let mut unique = Arc::new(
        read_directory_base(root.path(), ReadOptions::default(), &cancel)
            .expect("read base snapshot"),
    );
    let need = EnrichmentNeed {
        children: true,
        ..EnrichmentNeed::default()
    };
    let updates = probe_batch(&unique.entries[..1], need, &cancel).expect("probe first entry");

    let snapshot_ptr = Arc::as_ptr(&unique);
    let entries_ptr = unique.entries.as_ptr();
    assert_eq!(merge_enriched_in_place(&mut unique, &updates), Some(true));
    assert_eq!(Arc::as_ptr(&unique), snapshot_ptr);
    assert_eq!(unique.entries.as_ptr(), entries_ptr);

    let shared = Arc::clone(&unique);
    let updates_b = probe_batch(&unique.entries[1..2], need, &cancel).expect("probe second entry");
    assert_eq!(merge_enriched_in_place(&mut unique, &updates_b), Some(true));
    assert_ne!(Arc::as_ptr(&unique), Arc::as_ptr(&shared));
    assert_eq!(shared.entries[1].details.children, None);
    assert_eq!(
        unique.entries[1].details.children,
        Some(ChildCount {
            count: 0,
            truncated: false
        })
    );
}
