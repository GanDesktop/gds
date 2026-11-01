//! Content type detection for the type column.
//!
//! The header bytes decide whenever they carry a recognisable signature; the file
//! extension only names formats that a header cannot express (source languages,
//! office containers) and provides the fallback when the content is unreadable or
//! unrecognised. Detection never fails a directory listing: failures fall back to
//! the extension, then to an unknown content type.

use std::{ffi::OsStr, fs::File, io::Read, path::Path};

use super::details::{ContentCategory, ContentType};

/// Header bytes read for detection. Bounded so browsing never reads whole files.
const HEADER_LEN: u64 = 8 * 1024;

/// Detect the content type of a regular file.
///
/// `name` is the name whose extension is consulted; it is the target name for a
/// symlink whose target is inspected.
pub(super) fn detect(name: &OsStr, path: &Path) -> Option<ContentType> {
    let extension = extension_content(name);
    let Some(header) = read_header(path) else {
        return extension;
    };
    match sniff(&header) {
        Some(sniffed) => Some(reconcile(extension, sniffed)),
        None => extension.or_else(|| Some(text_or_binary(&header))),
    }
}

fn read_header(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut header = Vec::new();
    let mut reader = file.take(HEADER_LEN);
    reader.read_to_end(&mut header).ok()?;
    Some(header)
}

/// Identify content from its magic bytes.
fn sniff(header: &[u8]) -> Option<ContentType> {
    if header.is_empty() {
        return None;
    }
    if let Some(kind) = infer::get(header) {
        let mime = kind.mime_type();
        let (category, format) = content_for_mime(mime)
            .unwrap_or_else(|| (category_for_matcher(kind.matcher_type()), None));
        return Some(ContentType::new(category, format, Some(mime)));
    }
    // `image` recognises a few formats that the signature table does not.
    if let Ok(format) = image::guess_format(header) {
        let mime = format.to_mime_type();
        let (category, name) = content_for_mime(mime).unwrap_or((ContentCategory::Image, None));
        return Some(ContentType::new(category, name, Some(mime)));
    }
    None
}

/// Document containers that several formats share, so the header cannot tell
/// Word from Excel, PowerPoint or an e-book; the extension names the exact one.
const AMBIGUOUS_DOCUMENT_MIMES: [&str; 5] = [
    "application/zip",
    "application/msword",
    "application/vnd.ms-excel",
    "application/vnd.ms-powerpoint",
    "application/x-ole-storage",
];

/// Decide between the extension-derived and the content-derived type.
fn reconcile(extension: Option<ContentType>, sniffed: ContentType) -> ContentType {
    let Some(extension) = extension else {
        return sniffed;
    };
    if sniffed.category == ContentCategory::Text {
        // A header can prove that bytes are text, but not which language or
        // markup dialect they are written in; that name comes from the extension.
        let extension_names_text = matches!(
            extension.category,
            ContentCategory::Text | ContentCategory::Code | ContentCategory::Document
        ) || extension.format.as_deref() == Some("SVG");
        if extension_names_text {
            return extension;
        }
    }
    if extension.category == ContentCategory::Document
        && sniffed
            .mime
            .as_deref()
            .is_some_and(|mime| AMBIGUOUS_DOCUMENT_MIMES.contains(&mime))
    {
        return extension;
    }
    sniffed
}

/// Content type named by the extension alone, without reading the file.
fn extension_content(name: &OsStr) -> Option<ContentType> {
    let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    if let Some(content) = named_extension(&extension) {
        return Some(content);
    }
    let mime = mime_guess::from_ext(&extension)
        .first()?
        .essence_str()
        .to_owned();
    Some(match content_for_mime(&mime) {
        Some((category, format)) => ContentType::new(category, format, Some(mime.as_str())),
        None => {
            let format = extension.to_ascii_uppercase();
            ContentType::new(
                category_for_mime(&mime),
                Some(format.as_str()),
                Some(mime.as_str()),
            )
        }
    })
}

/// Extensions whose format the shared MIME table cannot name correctly.
fn named_extension(extension: &str) -> Option<ContentType> {
    use ContentCategory::{Archive, Code, Document};
    let (category, format, mime) = match extension {
        // Source languages that the shared MIME table reports as plain text.
        "rs" => (Code, "Rust", "text/x-rust"),
        "py" | "pyw" => (Code, "Python", "text/x-python"),
        "go" => (Code, "Go", "text/x-go"),
        "ts" | "tsx" => (Code, "TypeScript", "application/typescript"),
        "js" | "jsx" | "mjs" | "cjs" => (Code, "JavaScript", "text/javascript"),
        "kt" | "kts" => (Code, "Kotlin", "text/x-kotlin"),
        "zig" => (Code, "Zig", "text/x-zig"),
        "swift" => (Code, "Swift", "text/x-swift"),
        "nix" => (Code, "Nix", "text/x-nix"),
        // Containers whose names the shared MIME table lacks.
        "docx" => (
            Document,
            "Word",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        "xlsx" => (
            Document,
            "Excel",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        "pptx" => (
            Document,
            "PowerPoint",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ),
        "zst" => (Archive, "Zstandard", "application/zstd"),
        _ => return None,
    };
    Some(ContentType::new(category, Some(format), Some(mime)))
}

/// Broad category and friendly format name for a MIME type.
fn content_for_mime(mime: &str) -> Option<(ContentCategory, Option<&'static str>)> {
    use ContentCategory::{Archive, Audio, Code, Document, Executable, Font, Image, Text, Video};
    let content = match mime {
        "image/png" => (Image, Some("PNG")),
        "image/jpeg" => (Image, Some("JPEG")),
        "image/gif" => (Image, Some("GIF")),
        "image/webp" => (Image, Some("WebP")),
        "image/svg+xml" => (Image, Some("SVG")),
        "image/bmp" | "image/x-bmp" => (Image, Some("BMP")),
        "image/tiff" => (Image, Some("TIFF")),
        "image/x-icon" | "image/vnd.microsoft.icon" => (Image, Some("ICO")),
        "image/avif" => (Image, Some("AVIF")),
        "image/heic" => (Image, Some("HEIC")),
        "image/heif" => (Image, Some("HEIF")),
        "image/jxl" => (Image, Some("JPEG XL")),
        "image/x-tga" | "image/x-targa" => (Image, Some("TGA")),
        "image/vnd-ms.dds" | "image/x-dds" => (Image, Some("DDS")),
        "image/x-exr" | "image/openexr" => (Image, Some("OpenEXR")),
        "image/vnd.radiance" | "image/x-hdr" => (Image, Some("Radiance HDR")),
        "image/qoi" => (Image, Some("QOI")),
        "image/x-portable-bitmap"
        | "image/x-portable-graymap"
        | "image/x-portable-pixmap"
        | "image/x-portable-anymap" => (Image, Some("PNM")),
        "image/x-xcf" => (Image, Some("XCF")),
        "image/vnd.adobe.photoshop" | "image/x-psd" => (Image, Some("Photoshop")),

        "video/mp4" | "video/x-m4v" => (Video, Some("MPEG-4")),
        "video/webm" => (Video, Some("WebM")),
        "video/x-matroska" => (Video, Some("Matroska")),
        "video/quicktime" => (Video, Some("QuickTime")),
        "video/x-msvideo" => (Video, Some("AVI")),
        "video/mpeg" => (Video, Some("MPEG")),
        "video/mp2t" => (Video, Some("MPEG-TS")),
        "video/ogg" => (Video, Some("Ogg Video")),
        "video/x-flv" => (Video, Some("Flash Video")),
        "video/x-ms-wmv" | "video/x-ms-asf" => (Video, Some("Windows Media")),
        "video/3gpp" => (Video, Some("3GPP")),

        "audio/mpeg" => (Audio, Some("MP3")),
        "audio/flac" | "audio/x-flac" => (Audio, Some("FLAC")),
        "audio/wav" | "audio/x-wav" | "audio/vnd.wave" => (Audio, Some("WAV")),
        "audio/ogg" => (Audio, Some("Ogg")),
        "audio/opus" => (Audio, Some("Opus")),
        "audio/aac" => (Audio, Some("AAC")),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => (Audio, Some("M4A")),
        "audio/x-aiff" => (Audio, Some("AIFF")),
        "audio/midi" | "audio/x-midi" => (Audio, Some("MIDI")),
        "audio/x-ape" => (Audio, Some("Monkey's Audio")),
        "audio/x-wavpack" => (Audio, Some("WavPack")),

        "application/zip" | "application/x-zip-compressed" => (Archive, Some("ZIP")),
        "application/gzip" | "application/x-gzip" => (Archive, Some("Gzip")),
        "application/x-bzip2" => (Archive, Some("Bzip2")),
        "application/x-xz" => (Archive, Some("XZ")),
        "application/zstd" | "application/x-zstd" => (Archive, Some("Zstandard")),
        "application/x-7z-compressed" => (Archive, Some("7-Zip")),
        "application/vnd.rar" | "application/x-rar" | "application/x-rar-compressed" => {
            (Archive, Some("RAR"))
        }
        "application/x-tar" => (Archive, Some("tar")),
        "application/x-lz4" => (Archive, Some("LZ4")),
        "application/x-lzma" => (Archive, Some("LZMA")),
        "application/x-iso9660-image" => (Archive, Some("ISO")),
        "application/java-archive" => (Archive, Some("JAR")),
        "application/vnd.debian.binary-package" => (Archive, Some("Debian package")),
        "application/x-rpm" => (Archive, Some("RPM")),

        "application/pdf" => (Document, Some("PDF")),
        "application/rtf" | "text/rtf" => (Document, Some("RTF")),
        "application/msword" => (Document, Some("Word")),
        "application/vnd.ms-excel" => (Document, Some("Excel")),
        "application/vnd.ms-powerpoint" => (Document, Some("PowerPoint")),
        "application/epub+zip" => (Document, Some("EPUB")),
        "application/x-mobipocket-ebook" => (Document, Some("Mobipocket")),
        "application/vnd.oasis.opendocument.text" => (Document, Some("ODT")),
        "application/vnd.oasis.opendocument.spreadsheet" => (Document, Some("ODS")),
        "application/vnd.oasis.opendocument.presentation" => (Document, Some("ODP")),
        "application/vnd.oasis.opendocument.graphics" => (Document, Some("ODG")),
        "application/postscript" => (Document, Some("PostScript")),

        "application/x-executable"
        | "application/x-pie-executable"
        | "application/x-sharedlib"
        | "application/x-elf" => (Executable, Some("ELF")),
        "application/vnd.microsoft.portable-executable"
        | "application/x-dosexec"
        | "application/x-msdownload" => (Executable, Some("PE")),
        "application/x-mach-binary" => (Executable, Some("Mach-O")),
        "application/wasm" => (Executable, Some("WebAssembly")),

        "font/ttf" | "application/x-font-ttf" | "application/font-sfnt" => (Font, Some("TrueType")),
        "font/otf" | "application/x-font-otf" => (Font, Some("OpenType")),
        "font/woff" | "application/font-woff" => (Font, Some("WOFF")),
        "font/woff2" => (Font, Some("WOFF2")),
        "font/collection" => (Font, Some("TrueType Collection")),

        "text/plain" => (Text, None),
        "text/markdown" | "text/x-markdown" => (Text, Some("Markdown")),
        "text/csv" => (Text, Some("CSV")),
        "text/xml" | "application/xml" => (Text, Some("XML")),
        "application/json" | "text/x-json" => (Text, Some("JSON")),
        "application/toml" | "text/x-toml" => (Text, Some("TOML")),
        "application/x-yaml" | "text/x-yaml" | "text/yaml" => (Text, Some("YAML")),
        "application/x-ini" | "text/x-ini" => (Text, Some("INI")),
        "text/html" => (Code, Some("HTML")),
        "text/css" => (Code, Some("CSS")),
        "text/x-rust" => (Code, Some("Rust")),
        "text/x-python" | "application/x-python-code" => (Code, Some("Python")),
        "text/javascript" | "application/javascript" | "application/ecmascript" => {
            (Code, Some("JavaScript"))
        }
        "application/typescript" | "text/x-typescript" => (Code, Some("TypeScript")),
        "text/x-c" | "text/x-chdr" => (Code, Some("C")),
        "text/x-c++src" | "text/x-c++hdr" => (Code, Some("C++")),
        "text/x-csharp" => (Code, Some("C#")),
        "text/x-java" | "text/x-java-source" => (Code, Some("Java")),
        "text/x-go" => (Code, Some("Go")),
        "text/x-shellscript" | "application/x-sh" => (Code, Some("Shell")),
        "text/x-ruby" | "application/x-ruby" => (Code, Some("Ruby")),
        "text/x-php" | "application/x-php" => (Code, Some("PHP")),
        "application/x-perl" | "text/x-perl" => (Code, Some("Perl")),
        "text/x-lua" => (Code, Some("Lua")),
        "text/x-swift" => (Code, Some("Swift")),
        "text/x-kotlin" => (Code, Some("Kotlin")),
        "text/x-zig" => (Code, Some("Zig")),
        "text/x-haskell" => (Code, Some("Haskell")),
        "application/sql" | "text/x-sql" => (Code, Some("SQL")),
        "text/x-nix" => (Code, Some("Nix")),
        "text/x-makefile" => (Code, Some("Makefile")),
        "text/x-cmake" => (Code, Some("CMake")),
        _ => return None,
    };
    Some(content)
}

fn category_for_mime(mime: &str) -> ContentCategory {
    match mime.split_once('/') {
        Some(("image", _)) => ContentCategory::Image,
        Some(("audio", _)) => ContentCategory::Audio,
        Some(("video", _)) => ContentCategory::Video,
        Some(("font", _)) => ContentCategory::Font,
        Some(("text", _)) => ContentCategory::Text,
        _ => ContentCategory::Binary,
    }
}

fn category_for_matcher(matcher: infer::MatcherType) -> ContentCategory {
    match matcher {
        infer::MatcherType::App => ContentCategory::Executable,
        infer::MatcherType::Archive => ContentCategory::Archive,
        infer::MatcherType::Audio => ContentCategory::Audio,
        infer::MatcherType::Book | infer::MatcherType::Doc => ContentCategory::Document,
        infer::MatcherType::Font => ContentCategory::Font,
        infer::MatcherType::Image => ContentCategory::Image,
        infer::MatcherType::Text => ContentCategory::Text,
        infer::MatcherType::Video => ContentCategory::Video,
        infer::MatcherType::Custom => ContentCategory::Binary,
    }
}

fn text_or_binary(header: &[u8]) -> ContentType {
    if header.is_empty() || looks_like_text(header) {
        ContentType::new(ContentCategory::Text, None, Some("text/plain"))
    } else {
        ContentType::new(
            ContentCategory::Binary,
            None,
            Some("application/octet-stream"),
        )
    }
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => {
            // A multi-byte sequence cut off by the read limit is still text.
            if error.valid_up_to() + 3 >= bytes.len() {
                return true;
            }
            let control = bytes
                .iter()
                .copied()
                .filter(|byte| {
                    byte.is_ascii_control()
                        && !matches!(byte, b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
                })
                .count();
            control * 100 <= bytes.len()
        }
    }
}
