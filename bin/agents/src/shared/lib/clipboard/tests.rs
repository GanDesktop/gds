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
