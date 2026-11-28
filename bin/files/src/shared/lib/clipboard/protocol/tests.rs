use std::path::PathBuf;

use super::Formats;

#[test]
fn local_file_uri_accepts_local_hostname_and_rejects_foreign_or_parameterized_uris() {
    let host = Some("workstation");

    assert_eq!(
        Formats::local_file_uri_with_host("file:///tmp/a%20b.txt", host).expect("empty host"),
        PathBuf::from("/tmp/a b.txt")
    );
    assert_eq!(
        Formats::local_file_uri_with_host("file://localhost/tmp/a.txt", host).expect("localhost"),
        PathBuf::from("/tmp/a.txt")
    );
    assert_eq!(
        Formats::local_file_uri_with_host("file://LOCALHOST/tmp/a.txt", host)
            .expect("uppercase localhost"),
        PathBuf::from("/tmp/a.txt")
    );
    assert_eq!(
        Formats::local_file_uri_with_host("file://workstation/tmp/a.txt", host)
            .expect("matching local hostname"),
        PathBuf::from("/tmp/a.txt")
    );
    assert_eq!(
        Formats::local_file_uri_with_host("file://WORKSTATION/tmp/a.txt", host)
            .expect("case-insensitive local hostname"),
        PathBuf::from("/tmp/a.txt")
    );

    assert!(Formats::local_file_uri_with_host("file://other-host/tmp/a.txt", host).is_err());
    assert!(Formats::local_file_uri_with_host("file://workstation/tmp/a.txt?q=1", host).is_err());
    assert!(Formats::local_file_uri_with_host("file://workstation/tmp/a.txt#frag", host).is_err());
    assert!(Formats::local_file_uri_with_host("file://user@workstation/tmp/a.txt", host).is_err());
    assert!(Formats::local_file_uri_with_host("file://workstation:22/tmp/a.txt", host).is_err());
    assert!(Formats::local_file_uri_with_host("file://workstation/tmp/a%00b", host).is_err());
    assert!(Formats::local_file_uri_with_host("file:relative/a.txt", host).is_err());

    assert!(Formats::copyable_uris_with_host(
        &[
            "file://workstation/tmp/a.txt".to_owned(),
            "sftp://example.com/home/user/doc".to_owned(),
        ],
        host,
    ));
    assert!(!Formats::copyable_uris_with_host(
        &["file://other-host/tmp/a.txt".to_owned()],
        host,
    ));
}
