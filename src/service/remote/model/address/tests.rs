use super::*;

#[test]
fn remote_identity_preserves_user_port_and_encoded_names() {
    let root = Address::parse("sftp://alice@example.test:2222/team").expect("address");
    let child = root.child("文件 100%\n.txt").expect("child");
    assert!(child.uri().contains("%25"));
    assert!(!child.uri().contains('\n'));
    assert_eq!(child.name(), "文件 100%\n.txt");
    assert_eq!(child.parent(), Some(root.clone()));
    assert_eq!(child.connection_key(), root.connection_key());
    assert_ne!(
        root.connection_key(),
        root.with_username("bob")
            .expect("username")
            .connection_key()
    );
    let restored: Address =
        serde_json::from_str(&serde_json::to_string(&child).expect("encode")).expect("decode");
    assert_eq!(restored, child);
}

#[test]
fn untrusted_addresses_cannot_smuggle_secrets_or_invalid_names() {
    for input in [
        "file:///tmp/test",
        "https://host/path",
        "sftp://alice:secret@host/path",
        "smb://host/share?password=secret",
        "sftp://host/%FF",
        "sftp://host/%00",
        "sftp://host/%xx",
        "smb://host/share%5Coutside",
    ] {
        assert!(Address::parse(input).is_err(), "{input}");
    }
    let root = Address::parse("smb://host/")
        .expect("SMB root")
        .with_username("DOMAIN\\alice")
        .expect("domain username");
    assert_eq!(root.username(), "DOMAIN\\alice");
    assert_eq!(Address::parse(root.uri()).expect("encoded username"), root);
    assert!(root.parent().is_none());
    for name in ["", ".", "..", "../other", "dir/file", "dir\\file"] {
        assert!(root.child(name).is_err(), "{name}");
    }
}
