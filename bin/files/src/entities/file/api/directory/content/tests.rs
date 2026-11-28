use super::*;
use std::fs;

fn text_fixture_database() -> (tempfile::TempDir, MimeDatabase) {
    let root = tempfile::tempdir().expect("create MIME test directory");
    let mime = root.path().join("mime");
    fs::create_dir(&mime).expect("create mime subdirectory");
    fs::write(
        mime.join("globs2"),
        "50:text/rust:*.rs\n\
         50:image/png:*.png\n\
         50:image/jpeg:*.jpg\n\
         50:application/vnd.oasis.opendocument.spreadsheet-template:*.ots\n\
         50:application/x-compressed-tar:*.tar.gz\n\
         50:application/gzip:*.gz\n\
         50:image/x-test-png:*.alias\n\
         50:text/x-makefile:Makefile\n\
         50:application/json:*.json\n\
         50:application/schema+json:*.json\n\
         50:audio/ogg:*.ogg\n\
         50:video/ogg:*.ogg\n\
         50:audio/x-vorbis+ogg:*.ogg\n\
         50:video/x-theora+ogg:*.ogg\n\
         50:application/msword-template:*.dot\n\
         50:text/vnd.graphviz:*.dot\n",
    )
    .expect("write globs2 fixture");
    fs::write(mime.join("aliases"), "image/x-test-png image/png\n").expect("write aliases fixture");
    fs::write(
        mime.join("subclasses"),
        "application/schema+json application/json\n\
         application/json text/plain\n\
         audio/x-vorbis+ogg audio/ogg\n\
         video/x-theora+ogg video/ogg\n\
         audio/ogg application/ogg\n\
         video/ogg application/ogg\n\
         text/vnd.graphviz text/plain\n",
    )
    .expect("write subclasses fixture");
    fs::write(
        mime.join("generic-icons"),
        "application/vnd.oasis.opendocument.spreadsheet-template:x-office-spreadsheet\n\
         application/x-compressed-tar:package-x-generic\n\
         application/json:text-x-script\n\
         application/ogg:video-x-generic\n",
    )
    .expect("write generic-icons fixture");
    // Minimal shared-mime-info magic file: priority, MIME, offset, length, bytes.
    fs::write(
        mime.join("magic"),
        b"MIME-Magic\0\n\
          [80:image/png]\n>0=\0\x08\x89PNG\r\n\x1a\n\n\
          [60:audio/x-vorbis+ogg]\n>0=\0\x04OggS\n1>28=\0\x07\x01vorbis\n\
          [60:video/x-theora+ogg]\n>0=\0\x04OggS\n1>28=\0\x07\x80theora\n\
          [50:application/zip]\n>0=\0\x04PK\x03\x04\n",
    )
    .expect("write magic fixture");
    let db = MimeDatabase::from_directory(root.path());
    (root, db)
}

#[test]
fn unique_compound_and_alias_matches_resolve_without_opening_files() {
    let (_db_dir, db) = text_fixture_database();
    let root = tempfile::tempdir().expect("create fixture directory");
    let missing = root.path().join("missing-unopened");

    let rs = detect_with_database(&db, OsStr::new("main.rs"), &missing)
        .expect("rust should detect from filename");
    assert_eq!(rs.category, ContentCategory::Code);
    assert_eq!(rs.mime.as_deref(), Some("text/rust"));
    assert!(rs.text_previewable);

    let makefile = detect_with_database(&db, OsStr::new("Makefile"), &missing)
        .expect("Makefile should detect from literal name");
    assert_eq!(makefile.category, ContentCategory::Code);
    assert_eq!(makefile.mime.as_deref(), Some("text/x-makefile"));

    let alias = detect_with_database(&db, OsStr::new("picture.alias"), &missing)
        .expect("alias should normalize to image/png");
    assert_eq!(alias.category, ContentCategory::Image);
    assert_eq!(alias.mime.as_deref(), Some("image/png"));

    let ots = detect_with_database(&db, OsStr::new("template.ots"), &missing)
        .expect("ots should detect via generic-icons");
    assert_eq!(ots.category, ContentCategory::Document);
    assert_eq!(
        ots.mime.as_deref(),
        Some("application/vnd.oasis.opendocument.spreadsheet-template")
    );
    assert!(!ots.text_previewable);

    let tar_gz = detect_with_database(&db, OsStr::new("bundle.tar.gz"), &missing)
        .expect("compound extension should win over .gz");
    assert_eq!(tar_gz.category, ContentCategory::Archive);
    assert_eq!(tar_gz.mime.as_deref(), Some("application/x-compressed-tar"));
}

#[test]
fn parent_subclass_globs_and_linguist_extensions_stay_on_zero_io_fast_path() {
    let (_db_dir, db) = text_fixture_database();

    // `.json` has `application/json` + subclass `application/schema+json`: collapses without enrich.
    let FilenameClassification::Resolved(json) =
        classify_filename_with_database(&db, OsStr::new("config.json"))
    else {
        panic!(".json must resolve on zero-I/O fast path");
    };
    assert_eq!(json.category, ContentCategory::Text);
    assert_eq!(json.mime.as_deref(), Some("application/json"));
    assert!(json.text_previewable);

    // `.zig` and `.nix` are absent from the MIME fixture; Linguist resolves them with zero I/O.
    let FilenameClassification::Resolved(zig) =
        classify_filename_with_database(&db, OsStr::new("main.zig"))
    else {
        panic!(".zig must resolve via Linguist without enrich");
    };
    assert_eq!(zig.category, ContentCategory::Code);
    assert_eq!(zig.code_language.as_deref(), Some("Zig"));
    assert_eq!(zig.mime, None);
    assert_eq!(zig.source, DetectionSource::Linguist);

    let FilenameClassification::Resolved(nix) =
        classify_filename_with_database(&db, OsStr::new("flake.nix"))
    else {
        panic!(".nix must resolve via Linguist without enrich");
    };
    assert_eq!(nix.category, ContentCategory::Code);
    assert_eq!(nix.code_language.as_deref(), Some("Nix"));
}

#[test]
fn ambiguous_ogg_and_dot_fixtures_disambiguate_when_probed() {
    let (_db_dir, db) = text_fixture_database();
    let root = tempfile::tempdir().expect("create fixture directory");

    // Filename-only marks `.ogg` as Ambiguous with deterministic shared-parent convergence.
    let FilenameClassification::Ambiguous(fallback) =
        classify_filename_with_database(&db, OsStr::new("clip.ogg"))
    else {
        panic!(".ogg must be marked Ambiguous before probing");
    };
    assert_eq!(fallback.mime.as_deref(), Some("application/ogg"));

    // Vorbis audio header resolves to Audio.
    let mut vorbis = vec![0u8; 40];
    vorbis[..4].copy_from_slice(b"OggS");
    vorbis[28..35].copy_from_slice(b"\x01vorbis");
    let audio_path = root.path().join("track.ogg");
    fs::write(&audio_path, &vorbis).expect("write vorbis fixture");
    let audio = probe_content_with_database(&db, OsStr::new("track.ogg"), &audio_path)
        .expect("probe vorbis ogg");
    assert_eq!(audio.category, ContentCategory::Audio);
    assert_eq!(audio.mime.as_deref(), Some("audio/x-vorbis+ogg"));

    // Theora video header resolves to Video.
    let mut theora = vec![0u8; 40];
    theora[..4].copy_from_slice(b"OggS");
    theora[28..35].copy_from_slice(b"\x80theora");
    let video_path = root.path().join("movie.ogg");
    fs::write(&video_path, &theora).expect("write theora fixture");
    let video = probe_content_with_database(&db, OsStr::new("movie.ogg"), &video_path)
        .expect("probe theora ogg");
    assert_eq!(video.category, ContentCategory::Video);
    assert_eq!(video.mime.as_deref(), Some("video/x-theora+ogg"));

    // `.dot` with plain-text Graphviz content resolves via text/binary heuristic disambiguation.
    let dot_path = root.path().join("graph.dot");
    fs::write(&dot_path, b"digraph G { a -> b; }\n").expect("write graphviz fixture");
    let dot = probe_content_with_database(&db, OsStr::new("graph.dot"), &dot_path)
        .expect("probe dot file");
    assert_eq!(dot.mime.as_deref(), Some("text/vnd.graphviz"));
}

#[test]
fn recognized_names_take_precedence_over_file_headers() {
    let (_db_dir, db) = text_fixture_database();
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("sample");
    fs::write(&path, b"\x89PNG\r\n\x1a\nrest").expect("write sample header");

    let jpg = detect_with_database(&db, OsStr::new("picture.jpg"), &path).expect("detect jpg");
    assert_eq!(jpg.category, ContentCategory::Image);
    assert_eq!(jpg.mime.as_deref(), Some("image/jpeg"));
}

#[test]
fn unknown_names_fall_back_to_magic_and_heuristics() {
    let (_db_dir, db) = text_fixture_database();
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("sample");

    // Missing file returns None.
    assert!(detect_with_database(&db, OsStr::new("unknown"), &path).is_none());

    // Magic header match.
    fs::write(&path, b"\x89PNG\r\n\x1a\nrest").expect("write PNG header");
    let png = detect_with_database(&db, OsStr::new("no-extension"), &path).expect("sniff png");
    assert_eq!(png.category, ContentCategory::Image);
    assert_eq!(png.mime.as_deref(), Some("image/png"));
    assert_eq!(png.source, DetectionSource::Magic);

    // Empty unmatched file -> application/x-zerosize heuristic.
    fs::write(&path, b"").expect("write empty file");
    let empty = detect_with_database(&db, OsStr::new("unknown"), &path).expect("detect empty");
    assert_eq!(empty.category, ContentCategory::Text);
    assert_eq!(empty.mime.as_deref(), Some("application/x-zerosize"));
    assert_eq!(empty.source, DetectionSource::Heuristic);
    assert!(empty.text_previewable);

    // Empty file with recognized extension still uses its filename MIME.
    let empty_rs =
        detect_with_database(&db, OsStr::new("empty.rs"), &path).expect("detect empty rust");
    assert_eq!(empty_rs.category, ContentCategory::Code);
    assert_eq!(empty_rs.mime.as_deref(), Some("text/rust"));
    assert_eq!(empty_rs.source, DetectionSource::Filename);

    // Plain text -> text/plain heuristic.
    fs::write(&path, b"plain text").expect("write text file");
    let text = detect_with_database(&db, OsStr::new("notes.foo"), &path).expect("detect text");
    assert_eq!(text.category, ContentCategory::Text);
    assert_eq!(text.mime.as_deref(), Some("text/plain"));
    assert_eq!(text.source, DetectionSource::Heuristic);

    // Binary bytes -> application/octet-stream heuristic.
    fs::write(&path, b"\0\xff\0\xff").expect("write binary file");
    let bin = detect_with_database(&db, OsStr::new("data.bar"), &path).expect("detect binary");
    assert_eq!(bin.category, ContentCategory::Binary);
    assert_eq!(bin.mime.as_deref(), Some("application/octet-stream"));
    assert_eq!(bin.source, DetectionSource::Heuristic);
}

#[test]
fn content_reads_remain_bounded() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("large-file");
    fs::write(&path, vec![b'a'; HEADER_LEN as usize + 1024]).expect("write large file");
    assert_eq!(
        read_header(&path).expect("read bounded header").len(),
        HEADER_LEN as usize
    );
}

#[test]
fn system_shared_mime_info_acceptance_checks_when_installed() {
    if MIME_DB.mime_dirs().is_empty() {
        return;
    }
    // Verify `.ots`, `.tar.gz`, `.blend`, `.dmg`, `.rs`, `.py`, `.json` against the installed DB.
    let FilenameClassification::Resolved(ots) = classify_filename(OsStr::new("template.ots"))
    else {
        panic!(".ots should resolve cleanly on installed shared-mime-info");
    };
    assert_eq!(ots.category, ContentCategory::Document);
    assert!(!ots.text_previewable);

    let FilenameClassification::Resolved(tar_gz) = classify_filename(OsStr::new("archive.tar.gz"))
    else {
        panic!(".tar.gz should resolve cleanly");
    };
    assert_eq!(tar_gz.category, ContentCategory::Archive);
    assert_eq!(tar_gz.mime.as_deref(), Some("application/x-compressed-tar"));

    let FilenameClassification::Resolved(json) = classify_filename(OsStr::new("config.json"))
    else {
        panic!(".json should collapse to base MIME without enrich");
    };
    assert_eq!(json.category, ContentCategory::Text);
    assert_eq!(json.mime.as_deref(), Some("application/json"));

    let FilenameClassification::Resolved(rs) = classify_filename(OsStr::new("lib.rs")) else {
        panic!(".rs should resolve cleanly");
    };
    assert_eq!(rs.category, ContentCategory::Code);

    let FilenameClassification::Resolved(py) = classify_filename(OsStr::new("app.py")) else {
        panic!(".py should resolve cleanly");
    };
    assert_eq!(py.category, ContentCategory::Code);

    // If mime.cache is installed on the host, confirm the binary cache backend was engaged.
    if MIME_DB
        .mime_dirs()
        .iter()
        .any(|dir| dir.join("mime.cache").is_file())
    {
        assert!(MIME_DB.uses_binary_cache());
    }

    // Verify real installed per-type XML comments in Simplified Chinese and English
    // regardless of the process environment locale.
    let zh = crate::shared::i18n::prepare(gds::i18n::Language::Chinese).expect("zh catalog");
    let en = crate::shared::i18n::prepare(gds::i18n::Language::English).expect("en catalog");
    let root = tempfile::tempdir().expect("temp fixture dir");

    for (filename, expected_zh, expected_mime) in [
        ("scene.blend", "Blender 场景", "application/x-blender"),
        (
            "installer.dmg",
            "Apple 磁盘映像",
            "application/x-apple-diskimage",
        ),
        (
            "template.ots",
            "ODS 模板",
            "application/vnd.oasis.opendocument.spreadsheet-template",
        ),
        ("book.epub", "EPUB 电子书", "application/epub+zip"),
    ] {
        let path = root.path().join(filename);
        fs::write(&path, b"").expect("write fixture");
        let entry = crate::entities::file::directory::Entry::inspect(&path).expect("inspect entry");
        assert_eq!(
            crate::entities::file::ui::EntryPresentation::mime_type(&entry),
            Some(expected_mime)
        );
        let label_zh = crate::entities::file::ui::EntryPresentation::detailed_type_with_catalog(
            &entry,
            &zh,
            descriptions::cached_comments,
        );
        let label_en = crate::entities::file::ui::EntryPresentation::detailed_type_with_catalog(
            &entry,
            &en,
            descriptions::cached_comments,
        );
        assert_eq!(label_zh, expected_zh, "zh label mismatch for {filename}");
        assert!(
            !label_en.is_empty(),
            "en label must be non-empty for {filename}"
        );
    }

    // `.ots` uses Document category but must NOT trigger plain-text preview.
    let ots_path = root.path().join("template.ots");
    let ots_entry =
        crate::entities::file::directory::Entry::inspect(&ots_path).expect("inspect ots");
    assert_eq!(
        ots_entry.inspection_source().kind,
        gds::service::file_inspection::Kind::Other
    );

    // `.ogg` fixtures in a directory snapshot are marked for enrichment while `.json`/`.zig` are not.
    let json_path = root.path().join("config.json");
    let zig_path = root.path().join("main.zig");
    let ogg_audio_path = root.path().join("voice.ogg");
    let ogg_video_path = root.path().join("clip.ogg");
    fs::write(&json_path, b"{}").expect("write json");
    fs::write(&zig_path, b"pub fn main() void {}\n").expect("write zig");
    let mut vorbis = vec![0u8; 40];
    vorbis[..4].copy_from_slice(b"OggS");
    vorbis[28..35].copy_from_slice(b"\x01vorbis");
    fs::write(&ogg_audio_path, &vorbis).expect("write vorbis ogg");
    let mut theora = vec![0u8; 40];
    theora[..4].copy_from_slice(b"OggS");
    theora[28..35].copy_from_slice(b"\x80theora");
    fs::write(&ogg_video_path, &theora).expect("write theora ogg");

    let cancel = tokio_util::sync::CancellationToken::new();
    let snap = crate::entities::file::directory::read_directory_base(
        root.path(),
        crate::entities::file::directory::ReadOptions::default(),
        &cancel,
    )
    .expect("read base snapshot");
    let content_need = crate::entities::file::directory::EnrichmentNeed {
        content: true,
        ..Default::default()
    };
    assert!(
        !snap
            .entry_for_path(&json_path)
            .expect("json")
            .needs_enrichment(content_need)
    );
    assert!(
        !snap
            .entry_for_path(&zig_path)
            .expect("zig")
            .needs_enrichment(content_need)
    );
    let ogg_audio_base = snap.entry_for_path(&ogg_audio_path).expect("ogg audio");
    let ogg_video_base = snap.entry_for_path(&ogg_video_path).expect("ogg video");
    assert!(ogg_audio_base.needs_enrichment(content_need));
    assert!(ogg_video_base.needs_enrichment(content_need));

    let probed = crate::entities::file::directory::probe_batch(
        &[ogg_audio_base.clone(), ogg_video_base.clone()],
        content_need,
        &cancel,
    )
    .expect("probe ogg batch");
    assert_eq!(
        probed[0].details.content.as_ref().map(|c| c.category),
        Some(ContentCategory::Audio)
    );
    assert_eq!(
        probed[1].details.content.as_ref().map(|c| c.category),
        Some(ContentCategory::Video)
    );

    // Structural disambiguation on real installed `shared-mime-info` 2.4:
    // `.ts` (TypeScript vs Qt Linguist XML), `.tsx` (React TSX vs Tiled XML), `.mdx`, and `go.mod`.
    let ts_code = root.path().join("index.ts");
    let ts_qt = root.path().join("zh_CN.ts");
    let tsx_code = root.path().join("App.tsx");
    let tsx_tiled = root.path().join("tiles.tsx");
    let mdx_code = root.path().join("guide.mdx");
    let go_mod = root.path().join("go.mod");
    fs::write(&ts_code, "export const x: number = 1;\n").expect("write ts");
    fs::write(
        &ts_qt,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE TS>\n<TS version=\"2.1\"></TS>\n",
    )
    .expect("write qt ts");
    fs::write(&tsx_code, "export const App = () => <main />;\n").expect("write tsx");
    fs::write(
        &tsx_tiled,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tileset version=\"1.10\" name=\"map\"/>\n",
    )
    .expect("write tiled tsx");
    fs::write(&mdx_code, "# Heading\n\n<Badge />\n").expect("write mdx");
    fs::write(&go_mod, "module example.com/demo\n\ngo 1.24\n").expect("write go.mod");

    let ts_entry = crate::entities::file::directory::Entry::inspect(&ts_code).expect("inspect ts");
    assert_eq!(
        crate::entities::file::ui::EntryPresentation::detailed_type_with_catalog(
            &ts_entry,
            &zh,
            descriptions::cached_comments,
        ),
        "TypeScript 源代码"
    );
    let qt_entry = crate::entities::file::directory::Entry::inspect(&ts_qt).expect("inspect qt ts");
    assert_eq!(
        crate::entities::file::ui::EntryPresentation::mime_type(&qt_entry),
        Some("text/vnd.trolltech.linguist")
    );
    let tsx_entry =
        crate::entities::file::directory::Entry::inspect(&tsx_code).expect("inspect tsx");
    assert_eq!(
        crate::entities::file::ui::EntryPresentation::detailed_type_with_catalog(
            &tsx_entry,
            &zh,
            descriptions::cached_comments,
        ),
        "TSX 源代码"
    );
    let tiled_entry =
        crate::entities::file::directory::Entry::inspect(&tsx_tiled).expect("inspect tiled tsx");
    assert_eq!(
        crate::entities::file::ui::EntryPresentation::mime_type(&tiled_entry),
        Some("application/x-tiled-tsx")
    );
    let mdx_entry =
        crate::entities::file::directory::Entry::inspect(&mdx_code).expect("inspect mdx");
    assert_eq!(
        crate::entities::file::ui::EntryPresentation::detailed_type_with_catalog(
            &mdx_entry,
            &zh,
            descriptions::cached_comments,
        ),
        "MDX 源代码"
    );
    let gomod_entry =
        crate::entities::file::directory::Entry::inspect(&go_mod).expect("inspect go.mod");
    assert_eq!(
        gomod_entry.details.content.as_ref().map(|c| c.category),
        Some(ContentCategory::Text)
    );
    assert_eq!(
        crate::entities::file::ui::EntryPresentation::detailed_type_with_catalog(
            &gomod_entry,
            &zh,
            descriptions::cached_comments,
        ),
        "Go Module"
    );
}

#[test]
fn subclass_excludes_non_streamable_inodes_and_picker_shares_case_sensitive_rules() {
    let base = tempfile::tempdir().expect("create temp XDG dir");
    let mime_dir = base.path().join("mime");
    fs::create_dir_all(&mime_dir).expect("create mime dir");
    fs::write(mime_dir.join("globs2"), "60:text/x-kind:ExactName:cs\n").expect("write globs2");
    fs::write(
        mime_dir.join("subclasses"),
        "application/x-special-text text/plain\n",
    )
    .expect("write subclasses");

    let db = MimeDatabase::from_directory(base.path());

    // Per freedesktop spec, non-streamable `inode/*` types are not subclasses of `application/octet-stream`.
    assert!(!db.is_subclass("inode/directory", "application/octet-stream"));
    assert!(!db.is_subclass("inode/blockdevice", "application/octet-stream"));
    assert!(db.is_subclass("inode/directory", "inode/directory"));
    assert!(db.is_subclass("inode/directory", "inode/*"));
    assert!(db.is_subclass("inode/directory", "*/*"));
    assert!(db.is_subclass("text/plain", "application/octet-stream"));
    assert!(db.is_subclass("application/x-special-text", "text/*"));

    // Both directory filename classification and picker MIME filtering enforce case-sensitive literal rules.
    assert!(matches!(
        classify_filename_with_database(&db, OsStr::new("ExactName")),
        FilenameClassification::Resolved(content) if content.mime.as_deref() == Some("text/x-kind")
    ));
    assert!(matches!(
        classify_filename_with_database(&db, OsStr::new("exactname")),
        FilenameClassification::Unmatched
    ));
    assert!(matches_mime_filter_with_database(
        &db,
        OsStr::new("ExactName"),
        None,
        ["text/x-kind"],
    ));
    assert!(matches_mime_filter_with_database(
        &db,
        OsStr::new("ExactName"),
        None,
        ["text/plain"],
    ));
    assert!(!matches_mime_filter_with_database(
        &db,
        OsStr::new("exactname"),
        None,
        ["text/x-kind"],
    ));
    // Unknown basename falls back to `application/octet-stream`, matching wildcard/octet-stream filters.
    assert!(matches_mime_filter_with_database(
        &db,
        OsStr::new("exactname"),
        None,
        ["application/octet-stream"],
    ));
    assert!(matches_mime_filter_with_database(
        &db,
        OsStr::new("exactname"),
        None,
        ["*/*"],
    ));
    // Explicit detected MIME takes precedence over filename fallback and honours subclass inheritance.
    assert!(matches_mime_filter_with_database(
        &db,
        OsStr::new("exactname"),
        Some("application/x-special-text"),
        ["text/plain"],
    ));
    // Fallback directory without `magic` returns `None` from `sniff_data` without re-opening text indexes.
    assert_eq!(db.sniff_data(b"hello world"), None);
}

#[derive(Default)]
struct MimeCacheFixture<'a> {
    aliases: &'a [(&'a str, &'a str)],
    literals: &'a [(&'a str, &'a str, u32)],
    namespaces: &'a [(&'a str, &'a str, &'a str)],
    icons: &'a [(&'a str, &'a str)],
    generic_icons: &'a [(&'a str, &'a str)],
}

/// Build a valid big-endian FreeDesktop `mime.cache` v1.2 binary fixture without
/// requiring `update-mime-database` on the test host.
fn build_mime_cache_v1_2(fixture: &MimeCacheFixture<'_>) -> Vec<u8> {
    use std::collections::HashMap;

    fn pad4(buf: &mut Vec<u8>) {
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
    }

    fn write_u32(buf: &mut [u8], offset: usize, value: u32) {
        buf[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn intern(buf: &mut Vec<u8>, pool: &mut HashMap<String, u32>, text: &str) -> u32 {
        if let Some(&off) = pool.get(text) {
            return off;
        }
        let off = u32::try_from(buf.len()).expect("fixture buffer fits in u32");
        buf.extend_from_slice(text.as_bytes());
        buf.push(0);
        pad4(buf);
        pool.insert(text.to_owned(), off);
        off
    }

    // Reserve 40-byte v1.2 header.
    let mut buf = vec![0u8; 40];
    buf[0..2].copy_from_slice(&1u16.to_be_bytes());
    buf[2..4].copy_from_slice(&2u16.to_be_bytes());

    let mut pool = HashMap::new();

    let mut aliases: Vec<(u32, u32, &str)> = fixture
        .aliases
        .iter()
        .map(|&(alias, canonical)| {
            (
                intern(&mut buf, &mut pool, alias),
                intern(&mut buf, &mut pool, canonical),
                alias,
            )
        })
        .collect();
    aliases.sort_by_key(|&(_, _, key)| key);

    let mut literals: Vec<(u32, u32, u32, &str)> = fixture
        .literals
        .iter()
        .map(|&(lit, mime, wf)| {
            (
                intern(&mut buf, &mut pool, lit),
                intern(&mut buf, &mut pool, mime),
                wf,
                lit,
            )
        })
        .collect();
    literals.sort_by_key(|&(_, _, _, lit)| lit);

    let mut namespaces: Vec<(u32, u32, u32, &str, &str)> = fixture
        .namespaces
        .iter()
        .map(|&(ns, local, mime)| {
            (
                intern(&mut buf, &mut pool, ns),
                intern(&mut buf, &mut pool, local),
                intern(&mut buf, &mut pool, mime),
                ns,
                local,
            )
        })
        .collect();
    namespaces.sort_by_key(|&(_, _, _, ns, local)| (ns, local));

    let mut icons: Vec<(u32, u32, &str)> = fixture
        .icons
        .iter()
        .map(|&(mime, icon)| {
            (
                intern(&mut buf, &mut pool, mime),
                intern(&mut buf, &mut pool, icon),
                mime,
            )
        })
        .collect();
    icons.sort_by_key(|&(_, _, mime)| mime);

    let mut generic_icons: Vec<(u32, u32, &str)> = fixture
        .generic_icons
        .iter()
        .map(|&(mime, icon)| {
            (
                intern(&mut buf, &mut pool, mime),
                intern(&mut buf, &mut pool, icon),
                mime,
            )
        })
        .collect();
    generic_icons.sort_by_key(|&(_, _, mime)| mime);

    // AliasList (header offset 4)
    let alias_offset = u32::try_from(buf.len()).expect("alias offset");
    buf.extend_from_slice(
        &u32::try_from(aliases.len())
            .expect("alias count")
            .to_be_bytes(),
    );
    for (alias_off, canonical_off, _) in aliases {
        buf.extend_from_slice(&alias_off.to_be_bytes());
        buf.extend_from_slice(&canonical_off.to_be_bytes());
    }

    // ParentList (header offset 8) — empty
    let parent_offset = u32::try_from(buf.len()).expect("parent offset");
    buf.extend_from_slice(&0u32.to_be_bytes());

    // LiteralList (header offset 12)
    let literal_offset = u32::try_from(buf.len()).expect("literal offset");
    buf.extend_from_slice(
        &u32::try_from(literals.len())
            .expect("literal count")
            .to_be_bytes(),
    );
    for (lit_off, mime_off, wf, _) in literals {
        buf.extend_from_slice(&lit_off.to_be_bytes());
        buf.extend_from_slice(&mime_off.to_be_bytes());
        buf.extend_from_slice(&wf.to_be_bytes());
    }

    // ReverseSuffixTree (header offset 16) — empty
    let suffix_offset = u32::try_from(buf.len()).expect("suffix offset");
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&(suffix_offset + 8).to_be_bytes());

    // GlobList (header offset 20) — empty
    let glob_offset = u32::try_from(buf.len()).expect("glob offset");
    buf.extend_from_slice(&0u32.to_be_bytes());

    // MagicList (header offset 24) — empty
    let magic_offset = u32::try_from(buf.len()).expect("magic offset");
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&(magic_offset + 12).to_be_bytes());

    // NamespaceList (header offset 28)
    let namespace_offset = u32::try_from(buf.len()).expect("namespace offset");
    buf.extend_from_slice(
        &u32::try_from(namespaces.len())
            .expect("namespace count")
            .to_be_bytes(),
    );
    for (ns_off, local_off, mime_off, _, _) in namespaces {
        buf.extend_from_slice(&ns_off.to_be_bytes());
        buf.extend_from_slice(&local_off.to_be_bytes());
        buf.extend_from_slice(&mime_off.to_be_bytes());
    }

    // IconsList (header offset 32)
    let icons_offset = u32::try_from(buf.len()).expect("icons offset");
    buf.extend_from_slice(
        &u32::try_from(icons.len())
            .expect("icons count")
            .to_be_bytes(),
    );
    for (mime_off, icon_off, _) in icons {
        buf.extend_from_slice(&mime_off.to_be_bytes());
        buf.extend_from_slice(&icon_off.to_be_bytes());
    }

    // GenericIconsList (header offset 36)
    let generic_icons_offset = u32::try_from(buf.len()).expect("generic icons offset");
    buf.extend_from_slice(
        &u32::try_from(generic_icons.len())
            .expect("generic icons count")
            .to_be_bytes(),
    );
    for (mime_off, icon_off, _) in generic_icons {
        buf.extend_from_slice(&mime_off.to_be_bytes());
        buf.extend_from_slice(&icon_off.to_be_bytes());
    }

    write_u32(&mut buf, 4, alias_offset);
    write_u32(&mut buf, 8, parent_offset);
    write_u32(&mut buf, 12, literal_offset);
    write_u32(&mut buf, 16, suffix_offset);
    write_u32(&mut buf, 20, glob_offset);
    write_u32(&mut buf, 24, magic_offset);
    write_u32(&mut buf, 28, namespace_offset);
    write_u32(&mut buf, 32, icons_offset);
    write_u32(&mut buf, 36, generic_icons_offset);
    buf
}

#[test]
fn xdg_base_dir_resolution_ignores_relative_paths_and_applies_spec_fallbacks() {
    use std::path::PathBuf;

    let default_home = Some(PathBuf::from("/home/alice/.local/share"));

    // Empty or relative XDG_DATA_HOME falls back to BaseDirs default; relative-only XDG_DATA_DIRS
    // falls back to `/usr/local/share:/usr/share`.
    assert_eq!(
        mime_db::resolve_xdg_base_dirs(
            Some(OsStr::new("relative/data")),
            Some(OsStr::new("rel/one:./rel/two:")),
            default_home.clone(),
        ),
        vec![
            PathBuf::from("/home/alice/.local/share"),
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]
    );
    assert_eq!(
        mime_db::resolve_xdg_base_dirs(
            Some(OsStr::new("")),
            Some(OsStr::new("")),
            default_home.clone(),
        ),
        vec![
            PathBuf::from("/home/alice/.local/share"),
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]
    );

    // Mixed absolute + relative XDG_DATA_DIRS keeps only absolute paths in order.
    assert_eq!(
        mime_db::resolve_xdg_base_dirs(
            Some(OsStr::new("/custom/data")),
            Some(OsStr::new("relative:/opt/share:./ignored:/srv/share")),
            default_home,
        ),
        vec![
            PathBuf::from("/custom/data"),
            PathBuf::from("/opt/share"),
            PathBuf::from("/srv/share"),
        ]
    );

    // Missing home default still produces system XDG_DATA_DIRS fallback.
    assert_eq!(
        mime_db::resolve_xdg_base_dirs(Some(OsStr::new("relative")), None, None),
        vec![
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]
    );
}

#[test]
fn xml_namespace_refinement_and_icons_work_for_text_and_binary_mime_cache() {
    let text_root = tempfile::tempdir().expect("create text MIME dir");
    let text_mime = text_root.path().join("mime");
    fs::create_dir(&text_mime).expect("create text mime dir");
    fs::write(
        text_mime.join("XMLnamespaces"),
        "http://www.w3.org/2000/svg svg image/svg+xml\n\
         \x20xbel application/x-xbel\n\
         http://www.w3.org/1999/02/22-rdf-syntax-ns# RDF application/rdf+xml\n\
         http://example.com/ns  application/x-any-local\n",
    )
    .expect("write XMLnamespaces");
    fs::write(
        text_mime.join("icons"),
        "application/x-xbel:custom-xbel-icon\n\
         application/x-custom-doc:x-office-document\n",
    )
    .expect("write icons");
    fs::write(
        text_mime.join("aliases"),
        "application/x-xbel-alias application/x-xbel\n",
    )
    .expect("write aliases");
    fs::write(
        text_mime.join("magic"),
        b"MIME-Magic\0\n[40:application/xml]\n>0=\0\x05<?xml\n",
    )
    .expect("write xml magic");

    let text_db = MimeDatabase::from_directory(text_root.path());
    assert!(!text_db.uses_binary_cache());
    assert_eq!(
        text_db.explicit_icon("application/x-xbel").as_deref(),
        Some("custom-xbel-icon")
    );
    assert_eq!(
        text_db.explicit_icon("application/x-xbel-alias").as_deref(),
        Some("custom-xbel-icon")
    );
    assert_eq!(
        text_db
            .match_xml_namespace("http://www.w3.org/2000/svg", "svg")
            .as_deref(),
        Some("image/svg+xml")
    );
    assert_eq!(
        text_db.match_xml_namespace("", "xbel").as_deref(),
        Some("application/x-xbel")
    );
    assert_eq!(
        text_db
            .match_xml_namespace("http://example.com/ns", "anything")
            .as_deref(),
        Some("application/x-any-local")
    );

    // Explicit freedesktop category icon in `icons` contributes to category derivation.
    let custom_doc = build_from_mime(
        &text_db,
        "application/x-custom-doc",
        None,
        DetectionSource::Magic,
    );
    assert_eq!(custom_doc.category, ContentCategory::Document);

    let samples = tempfile::tempdir().expect("create sample files dir");

    // Extensionless SVG sniffed as `application/xml` refines to `image/svg+xml`.
    let svg_path = samples.path().join("logo");
    fs::write(
        &svg_path,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!-- leading comment -->\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><circle r=\"8\"/></svg>\n",
    )
    .expect("write svg sample");
    let svg = detect_with_database(&text_db, OsStr::new("logo"), &svg_path).expect("detect svg");
    assert_eq!(svg.mime.as_deref(), Some("image/svg+xml"));
    assert_eq!(svg.category, ContentCategory::Image);
    assert_eq!(svg.source, DetectionSource::Magic);

    // Extensionless XBEL with DOCTYPE internal subset and no xmlns refines to `application/x-xbel`.
    let xbel_path = samples.path().join("bookmarks");
    fs::write(
        &xbel_path,
        "<?xml version=\"1.0\"?>\n\
         <!DOCTYPE xbel [ <!ENTITY arrow \"a > b\"> <!-- > --> ]>\n\
         <xbel version=\"1.0\"><bookmark href=\"https://example.org\"/></xbel>\n",
    )
    .expect("write xbel sample");
    let xbel =
        detect_with_database(&text_db, OsStr::new("bookmarks"), &xbel_path).expect("detect xbel");
    assert_eq!(xbel.mime.as_deref(), Some("application/x-xbel"));
    assert_eq!(xbel.source, DetectionSource::Magic);

    // Prefixed XML root tag without XML declaration still refines via namespace binding.
    let rdf_path = samples.path().join("graph");
    fs::write(
        &rdf_path,
        "<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description/></rdf:RDF>",
    )
    .expect("write rdf sample");
    let rdf = detect_with_database(&text_db, OsStr::new("graph"), &rdf_path).expect("detect rdf");
    assert_eq!(rdf.mime.as_deref(), Some("application/rdf+xml"));

    // Truncated > 8 KiB SVG file (unclosed root element within the 8 KiB read window) still refines.
    let big_svg_path = samples.path().join("huge-vector");
    let mut big_svg = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"",
    );
    big_svg.push_str(&"M0 0 ".repeat(2048));
    big_svg.push_str("\"/></svg>");
    fs::write(&big_svg_path, &big_svg).expect("write truncated svg sample");
    let huge = detect_with_database(&text_db, OsStr::new("huge-vector"), &big_svg_path)
        .expect("detect huge svg");
    assert_eq!(huge.mime.as_deref(), Some("image/svg+xml"));
    assert_eq!(huge.category, ContentCategory::Image);

    // Binary `mime.cache` v1.2 fixture with `NamespaceList` (offset 28) and `IconsList` (offset 32).
    let cache_root = tempfile::tempdir().expect("create binary cache dir");
    let cache_mime = cache_root.path().join("mime");
    fs::create_dir(&cache_mime).expect("create binary cache mime dir");
    let cache_bytes = build_mime_cache_v1_2(&MimeCacheFixture {
        aliases: &[("application/x-xbel-alias", "application/x-xbel")],
        literals: &[],
        namespaces: &[
            ("", "xbel", "application/x-xbel"),
            ("http://example.com/ns", "", "application/x-any-local"),
            ("http://www.w3.org/2000/svg", "svg", "image/svg+xml"),
        ],
        icons: &[("application/x-xbel", "binary-xbel-icon")],
        generic_icons: &[],
    });
    fs::write(cache_mime.join("mime.cache"), cache_bytes).expect("write binary mime.cache");

    let cache_db = MimeDatabase::from_directory(cache_root.path());
    assert!(cache_db.uses_binary_cache());
    assert_eq!(
        cache_db.explicit_icon("application/x-xbel").as_deref(),
        Some("binary-xbel-icon")
    );
    assert_eq!(
        cache_db
            .explicit_icon("application/x-xbel-alias")
            .as_deref(),
        Some("binary-xbel-icon")
    );
    assert_eq!(
        cache_db
            .match_xml_namespace("http://www.w3.org/2000/svg", "svg")
            .as_deref(),
        Some("image/svg+xml")
    );
    assert_eq!(
        cache_db.match_xml_namespace("", "xbel").as_deref(),
        Some("application/x-xbel")
    );
    assert_eq!(
        cache_db
            .match_xml_namespace("http://example.com/ns", "other")
            .as_deref(),
        Some("application/x-any-local")
    );

    let cache_svg = detect_with_database(&cache_db, OsStr::new("huge-vector"), &big_svg_path)
        .expect("detect cache svg");
    assert_eq!(cache_svg.mime.as_deref(), Some("image/svg+xml"));
    let cache_xbel = detect_with_database(&cache_db, OsStr::new("bookmarks"), &xbel_path)
        .expect("detect cache xbel");
    assert_eq!(cache_xbel.mime.as_deref(), Some("application/x-xbel"));
}

#[test]
fn noglobs_suppresses_lower_priority_globs_without_matching_literal_sentinel() {
    let lower = tempfile::tempdir().expect("create lower XDG dir");
    let lower_mime = lower.path().join("mime");
    fs::create_dir(&lower_mime).expect("create lower mime dir");
    fs::write(
        lower_mime.join("globs2"),
        "50:text/plain:*.txt\n\
         50:text/x-plain-alias:*.alias\n\
         50:text/markdown:*.md\n",
    )
    .expect("write lower globs2");
    fs::write(
        lower_mime.join("aliases"),
        "text/x-plain-alias text/plain\n",
    )
    .expect("write lower aliases");

    // 1. Plain-text higher-priority layer with `0:text/plain:__NOGLOBS__` + replacement glob.
    let upper_text = tempfile::tempdir().expect("create upper text XDG dir");
    let upper_text_mime = upper_text.path().join("mime");
    fs::create_dir(&upper_text_mime).expect("create upper text mime dir");
    fs::write(
        upper_text_mime.join("globs2"),
        "0:text/plain:__NOGLOBS__\n\
         50:text/plain:*.custom\n",
    )
    .expect("write upper globs2");

    let text_layered = MimeDatabase::from_base_directories(&[
        upper_text.path().to_path_buf(),
        lower.path().to_path_buf(),
    ]);
    assert!(text_layered.filename_candidates("notes.txt").is_empty());
    assert!(text_layered.filename_candidates("notes.alias").is_empty());
    assert!(text_layered.filename_candidates("__NOGLOBS__").is_empty());
    assert_eq!(
        text_layered.filename_candidates("notes.custom"),
        vec!["text/plain".to_owned()]
    );
    assert_eq!(
        text_layered.filename_candidates("readme.md"),
        vec!["text/markdown".to_owned()]
    );

    // 2. Binary `mime.cache` v1.2 higher-priority layer with `__NOGLOBS__` in `LiteralList`.
    let upper_cache = tempfile::tempdir().expect("create upper cache XDG dir");
    let upper_cache_mime = upper_cache.path().join("mime");
    fs::create_dir(&upper_cache_mime).expect("create upper cache mime dir");
    let cache_bytes = build_mime_cache_v1_2(&MimeCacheFixture {
        aliases: &[],
        literals: &[
            ("CustomPlain", "text/plain", 50),
            ("__NOGLOBS__", "text/plain", 0),
        ],
        namespaces: &[],
        icons: &[],
        generic_icons: &[],
    });
    fs::write(upper_cache_mime.join("mime.cache"), cache_bytes).expect("write upper mime.cache");

    let cache_layered = MimeDatabase::from_base_directories(&[
        upper_cache.path().to_path_buf(),
        lower.path().to_path_buf(),
    ]);
    assert!(cache_layered.uses_binary_cache());
    assert!(cache_layered.filename_candidates("notes.txt").is_empty());
    assert!(cache_layered.filename_candidates("notes.alias").is_empty());
    assert!(cache_layered.filename_candidates("__NOGLOBS__").is_empty());
    assert_eq!(
        cache_layered.filename_candidates("CustomPlain"),
        vec!["text/plain".to_owned()]
    );
    assert_eq!(
        cache_layered.filename_candidates("readme.md"),
        vec!["text/markdown".to_owned()]
    );
}
