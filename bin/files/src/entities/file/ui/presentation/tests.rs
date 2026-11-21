use std::{ffi::OsString, path::PathBuf, sync::Arc};

use gds::i18n::Language;

use super::*;
use crate::{
    entities::file::directory::{
        ContentCategory, ContentType, DetectionSource, Entry, EntryDetails, EntryKind,
    },
    shared::i18n,
};

fn file_entry(name: &str, content: Option<ContentType>) -> Entry {
    Entry::new(
        PathBuf::from(format!("/tmp/{name}")),
        OsString::from(name),
        EntryKind::File,
        Some(128),
        None,
        EntryDetails {
            content,
            ..EntryDetails::default()
        },
    )
}

fn mime_content(
    category: ContentCategory,
    mime: &str,
    source: DetectionSource,
    text_previewable: bool,
) -> ContentType {
    ContentType {
        category,
        code_language: None,
        mime: Some(Arc::from(mime)),
        source,
        text_previewable,
    }
}

#[test]
fn detailed_type_follows_localized_comment_fluent_english_and_category_fallbacks() {
    let zh = i18n::prepare(Language::Chinese).expect("Chinese catalog");
    let en = i18n::prepare(Language::English).expect("English catalog");

    let lookup = |mime: &str| match mime {
        "application/x-blender" => Some(mime_descriptions::parse_xml_comments(
            r#"<mime-type type="application/x-blender">
  <comment>Blender scene</comment>
  <comment xml:lang="zh-Hans-CN">Blender 场景</comment>
</mime-type>"#,
        )),
        "application/x-apple-diskimage" => Some(mime_descriptions::parse_xml_comments(
            r#"<mime-type type="application/x-apple-diskimage">
  <comment>Apple disk image</comment>
  <comment xml:lang="zh-Hans-CN">Apple 磁盘映像</comment>
</mime-type>"#,
        )),
        "application/vnd.oasis.opendocument.spreadsheet-template" => {
            Some(mime_descriptions::parse_xml_comments(
                r#"<mime-type type="application/vnd.oasis.opendocument.spreadsheet-template">
  <comment>ODS template</comment>
  <comment xml:lang="zh-Hans-CN">ODS 模板</comment>
</mime-type>"#,
            ))
        }
        // Sampled 2.4 package: EPUB has English + Traditional Chinese (different script), no Simplified Chinese.
        "application/epub+zip" => Some(mime_descriptions::parse_xml_comments(
            r#"<mime-type type="application/epub+zip">
  <comment>Electronic book document</comment>
  <comment xml:lang="zh-Hant-TW">電子書文件</comment>
</mime-type>"#,
        )),
        // Rare MIME absent from Fluent fallback: falls back to default English comment in non-English UI.
        "application/font-tdpfr" => Some(mime_descriptions::parse_xml_comments(
            r#"<mime-type type="application/font-tdpfr">
  <comment>TDPFR font</comment>
</mime-type>"#,
        )),
        _ => None,
    };

    // 1. Primary truth: XML localized comments in GDS interface language (independent of system locale).
    let blend = file_entry(
        "scene.blend",
        Some(mime_content(
            ContentCategory::Image,
            "application/x-blender",
            DetectionSource::Filename,
            false,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&blend, &zh, lookup),
        "Blender 场景"
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&blend, &en, lookup),
        "Blender scene"
    );
    assert_eq!(
        EntryPresentation::mime_type(&blend),
        Some("application/x-blender")
    );

    let dmg = file_entry(
        "installer.dmg",
        Some(mime_content(
            ContentCategory::Archive,
            "application/x-apple-diskimage",
            DetectionSource::Filename,
            false,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&dmg, &zh, lookup),
        "Apple 磁盘映像"
    );

    let ots = file_entry(
        "template.ots",
        Some(mime_content(
            ContentCategory::Document,
            "application/vnd.oasis.opendocument.spreadsheet-template",
            DetectionSource::Filename,
            false,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&ots, &zh, lookup),
        "ODS 模板"
    );

    // 2. Missing Simplified Chinese comment -> Fluent fallback for common types (`epub`),
    //    and default English comment for rare types (`font-tdpfr`).
    let epub = file_entry(
        "book.epub",
        Some(mime_content(
            ContentCategory::Document,
            "application/epub+zip",
            DetectionSource::Filename,
            false,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&epub, &zh, lookup),
        "EPUB 电子书"
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&epub, &en, lookup),
        "Electronic book document"
    );

    let rare_font = file_entry(
        "custom.pfr",
        Some(mime_content(
            ContentCategory::Font,
            "application/font-tdpfr",
            DetectionSource::Filename,
            false,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&rare_font, &zh, lookup),
        "TDPFR font"
    );

    // 3. Missing database description -> localized category (+ extension).
    let unlisted = file_entry(
        "report.qxz",
        Some(mime_content(
            ContentCategory::Document,
            "application/x-custom-qxz",
            DetectionSource::Filename,
            false,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&unlisted, &zh, lookup),
        "QXZ 文档"
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&unlisted, &en, lookup),
        "QXZ Document"
    );

    // 4. Heuristic generic fallback on unknown extension retains uppercase extension.
    let heur_text = file_entry(
        "notes.foo",
        Some(mime_content(
            ContentCategory::Text,
            "text/plain",
            DetectionSource::Heuristic,
            true,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&heur_text, &zh, lookup),
        "FOO 纯文本"
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&heur_text, &en, lookup),
        "FOO Plain text"
    );

    let heur_bin = file_entry(
        "data.bar",
        Some(mime_content(
            ContentCategory::Binary,
            "application/octet-stream",
            DetectionSource::Heuristic,
            false,
        )),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&heur_bin, &zh, lookup),
        "BAR 二进制"
    );

    // 5. Unprobed unknown extension (e.g. content probing disabled) -> localized extension template.
    let unprobed_ext = file_entry("archive.xyz", None);
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&unprobed_ext, &zh, lookup),
        "XYZ 文件"
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&unprobed_ext, &en, lookup),
        "XYZ file"
    );

    // 6. Linguist zero-I/O code fallback.
    let zig = file_entry(
        "main.zig",
        Some(ContentType {
            category: ContentCategory::Code,
            code_language: Some(Arc::from("Zig")),
            mime: None,
            source: DetectionSource::Linguist,
            text_previewable: true,
        }),
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&zig, &zh, lookup),
        "Zig 源代码"
    );
    assert_eq!(
        EntryPresentation::detailed_type_with_catalog(&zig, &en, lookup),
        "Zig source code"
    );
}
