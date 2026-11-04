use super::Contents;
use std::path::PathBuf;

#[test]
fn uri_lists_accept_only_local_files_and_preserve_unix_names() {
    let paths = Contents::file_paths(
        "# clipboard files\r\nfile:///tmp/a%20b.txt\r\nfile://localhost/tmp/a%20b.txt\n\
         file:///tmp/raw-%FF\nfile://remote/tmp/reject.txt\nhttps://example.com/image.png\ninvalid\n",
    );
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    assert_eq!(
        paths,
        [
            PathBuf::from("/tmp/a b.txt"),
            PathBuf::from(OsString::from_vec(b"/tmp/raw-\xff".to_vec())),
        ]
    );
}

#[test]
fn native_file_representations_take_precedence_over_image_alternatives() {
    use gpui_kit::{ClipboardMime, ClipboardMimeOffer};
    let contents = Contents::from_mime(Some(ClipboardMimeOffer {
        id: 1,
        entries: vec![
            ClipboardMime {
                mime_type: "x-special/gnome-copied-files".into(),
                bytes: b"copy\nfile:///tmp/a%20b.png".to_vec(),
            },
            ClipboardMime {
                mime_type: "image/png".into(),
                bytes: vec![1, 2, 3],
            },
            ClipboardMime {
                mime_type: "text/plain".into(),
                bytes: b"a\r\nb".to_vec(),
            },
        ],
    }))
    .expect("desktop offer");
    assert_eq!(contents.paths, [PathBuf::from("/tmp/a b.png")]);
    assert!(contents.images.is_empty());
    assert_eq!(contents.text.as_deref(), Some("a\nb"));
}
