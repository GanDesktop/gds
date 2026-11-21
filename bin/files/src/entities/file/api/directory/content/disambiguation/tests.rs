use std::{ffi::OsStr, fs};

use super::*;
use crate::entities::file::api::directory::content::mime_db::MimeDatabase;

fn conflict_fixture_db() -> (tempfile::TempDir, MimeDatabase) {
    let root = tempfile::tempdir().expect("create disambiguation fixture");
    let mime = root.path().join("mime");
    fs::create_dir(&mime).expect("create mime subdir");
    fs::write(
        mime.join("globs2"),
        "50:text/vnd.trolltech.linguist:*.ts\n\
         50:video/mp2t:*.ts\n\
         50:application/x-tiled-tsx:*.tsx\n\
         50:video/mp2t:*.mts\n\
         50:application/x-genesis-32x-rom:*.mdx\n\
         50:audio/x-mod:*.mod\n\
         50:application/smil+xml:*.smi\n\
         50:application/x-sami:*.smi\n\
         50:application/smil+xml:*.sml\n\
         50:application/xhtml+xml:*.xhtml\n\
         50:application/xslt+xml:*.xslt\n\
         50:application/json:*.json\n\
         50:application/schema+json:*.json\n\
         50:image/png:*.png\n",
    )
    .expect("write globs2");
    fs::write(
        mime.join("subclasses"),
        "text/vnd.trolltech.linguist application/xml\n\
         application/x-tiled-tsx application/xml\n\
         application/smil+xml application/xml\n\
         application/xhtml+xml application/xml\n\
         application/xslt+xml application/xml\n\
         application/xml text/plain\n\
         application/x-sami text/plain\n\
         application/schema+json application/json\n\
         application/json text/plain\n",
    )
    .expect("write subclasses");
    fs::write(
        mime.join("magic"),
        b"MIME-Magic\0\n\
          [55:application/smil+xml]\n>0=\0\x05<smil\n\
          [50:application/x-sami]\n>0=\0\x06<SAMI>\n\
          [50:text/vnd.trolltech.linguist]\n>0=\0\x04<TS>\n\
          [50:application/x-genesis-32x-rom]\n>0=\0\x08SEGA 32X\n",
    )
    .expect("write magic");
    let db = MimeDatabase::from_directory(root.path());
    (root, db)
}

#[test]
fn detects_multi_mime_and_linguist_shadow_without_touching_normal_fast_paths() {
    let (_dir, db) = conflict_fixture_db();

    // Pattern 1: multi-MIME conflicts.
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("index.ts"),
            &db.filename_candidates("index.ts")
        ),
        ConflictKind::MultiMime
    );
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("sub.smi"),
            &db.filename_candidates("sub.smi")
        ),
        ConflictKind::MultiMime
    );

    // Pattern 2: legacy binary/XML single matches shadowed by Linguist text/code formats.
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("App.tsx"),
            &db.filename_candidates("App.tsx")
        ),
        ConflictKind::LinguistShadow
    );
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("mod.mts"),
            &db.filename_candidates("mod.mts")
        ),
        ConflictKind::LinguistShadow
    );
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("post.mdx"),
            &db.filename_candidates("post.mdx")
        ),
        ConflictKind::LinguistShadow
    );
    assert_eq!(
        detect_conflict(&db, OsStr::new("go.mod"), &db.filename_candidates("go.mod")),
        ConflictKind::LinguistShadow
    );
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("tree.sml"),
            &db.filename_candidates("tree.sml")
        ),
        ConflictKind::LinguistShadow
    );

    // Normal formats, XML code formats, and parent+subclass globs stay on the zero-I/O fast path.
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("photo.png"),
            &db.filename_candidates("photo.png")
        ),
        ConflictKind::None
    );
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("config.json"),
            &db.filename_candidates("config.json")
        ),
        ConflictKind::None
    );
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("page.xhtml"),
            &db.filename_candidates("page.xhtml")
        ),
        ConflictKind::None
    );
    assert_eq!(
        detect_conflict(
            &db,
            OsStr::new("style.xslt"),
            &db.filename_candidates("style.xslt")
        ),
        ConflictKind::None
    );
}

#[test]
fn disambiguates_ts_tsx_mts_mdx_go_mod_and_smi_structurally() {
    let (_dir, db) = conflict_fixture_db();

    // `.ts`: TypeScript text vs Qt Linguist XML vs MPEG-2 binary.
    let ts_cands = db.filename_candidates("index.ts");
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("index.ts"),
            &ts_cands,
            Some(b"export const answer: number = 42;\n"),
        ),
        Outcome::Linguist(LinguistFallback::Code("TypeScript"))
    );
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("zh_CN.ts"),
            &ts_cands,
            Some(b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE TS>\n<TS version=\"2.1\"></TS>\n"),
        ),
        Outcome::Mime {
            mime: "text/vnd.trolltech.linguist".to_owned(),
            source: DetectionSource::Filename,
        }
    );
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("clip.ts"),
            &ts_cands,
            Some(&[0x47, 0x40, 0x00, 0x10, 0x00, 0xff]),
        ),
        Outcome::Mime {
            mime: "video/mp2t".to_owned(),
            source: DetectionSource::Filename,
        }
    );

    // `.tsx`: React TSX vs Tiled XML.
    let tsx_cands = db.filename_candidates("App.tsx");
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("App.tsx"),
            &tsx_cands,
            Some(b"import React from 'react';\nexport const App = () => <div />;\n"),
        ),
        Outcome::Linguist(LinguistFallback::Code("TSX"))
    );
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("tiles.tsx"),
            &tsx_cands,
            Some(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tileset version=\"1.10\" name=\"grass\"/>\n"),
        ),
        Outcome::Mime {
            mime: "application/x-tiled-tsx".to_owned(),
            source: DetectionSource::Filename,
        }
    );

    // `.mts`: TypeScript ESM vs binary MPEG-2.
    let mts_cands = db.filename_candidates("mod.mts");
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("mod.mts"),
            &mts_cands,
            Some(b"export function greet(name: string): string { return name; }\n"),
        ),
        Outcome::Linguist(LinguistFallback::Code("TypeScript"))
    );
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("camera.mts"),
            &mts_cands,
            Some(&[0x00, 0x00, 0x00, 0x01, 0x47, 0xff]),
        ),
        Outcome::Mime {
            mime: "video/mp2t".to_owned(),
            source: DetectionSource::Filename,
        }
    );

    // `.mdx`: MDX text vs binary Sega 32X ROM.
    let mdx_cands = db.filename_candidates("post.mdx");
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("post.mdx"),
            &mdx_cands,
            Some(b"# Hello MDX\n\n<Callout>Content</Callout>\n"),
        ),
        Outcome::Linguist(LinguistFallback::Code("MDX"))
    );
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("game.mdx"),
            &mdx_cands,
            Some(b"SEGA 32X\x00\x00\xff"),
        ),
        Outcome::Mime {
            mime: "application/x-genesis-32x-rom".to_owned(),
            source: DetectionSource::Magic,
        }
    );

    // `go.mod`: Go Module text vs binary Amiga audio.
    let mod_cands = db.filename_candidates("go.mod");
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("go.mod"),
            &mod_cands,
            Some(b"module github.com/example/app\n\ngo 1.24\n"),
        ),
        Outcome::Linguist(LinguistFallback::Data("Go Module"))
    );
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("go.mod"),
            &mod_cands,
            Some(&[0x00, 0x4d, 0x2e, 0x4b, 0x2e, 0x00]),
        ),
        Outcome::Mime {
            mime: "audio/x-mod".to_owned(),
            source: DetectionSource::Filename,
        }
    );

    // `.smi`: non-XML SAMI subtitle vs SMIL XML.
    let smi_cands = db.filename_candidates("sub.smi");
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("sub.smi"),
            &smi_cands,
            Some(b"<SAMI><BODY>hello</BODY></SAMI>"),
        ),
        Outcome::Mime {
            mime: "application/x-sami".to_owned(),
            source: DetectionSource::Magic,
        }
    );
    assert_eq!(
        resolve_with_header(
            &db,
            OsStr::new("pres.smi"),
            &smi_cands,
            Some(b"<smil><body/></smil>"),
        ),
        Outcome::Mime {
            mime: "application/smil+xml".to_owned(),
            source: DetectionSource::Magic,
        }
    );
}
