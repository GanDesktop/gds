use super::*;

#[test]
fn protocol_fields_preserve_paths_and_account_identity() {
    let sftp = Details {
        protocol: Protocol::Sftp,
        host: "delta".into(),
        port: "2222".into(),
        user: "atlas".into(),
        path: "/home/atlas/中文 100%\n".into(),
    }
    .address()
    .expect("SFTP fields");
    assert_eq!(sftp.protocol(), Protocol::Sftp);
    assert_eq!(sftp.host(), "delta");
    assert_eq!(sftp.port(), 2222);
    assert_eq!(sftp.path(), "/home/atlas/中文 100%\n");
    let smb = Details {
        protocol: Protocol::Smb,
        host: "nas".into(),
        port: "445".into(),
        user: "DOMAIN\\atlas".into(),
        path: "团队共享/资料".into(),
    }
    .address()
    .expect("SMB fields");
    assert_eq!(smb.protocol(), Protocol::Smb);
    assert_eq!(smb.username(), "DOMAIN\\atlas");
    assert_eq!(smb.path(), "/团队共享/资料");
}

#[test]
fn invalid_connection_fields_are_rejected_before_network_access() {
    let mut details = Details {
        protocol: Protocol::Smb,
        host: "nas".into(),
        port: "445".into(),
        user: "atlas".into(),
        path: "".into(),
    };
    assert!(matches!(details.address(), Err(Error::Share)));
    details.path = "share".into();
    details.port = "0".into();
    assert!(matches!(details.address(), Err(Error::Port)));
    details.port = "445".into();
    details.host = "sftp://delta".into();
    assert!(matches!(details.address(), Err(Error::Host)));
    details.host = "nas/other".into();
    assert!(matches!(details.address(), Err(Error::Host)));
    details.host = "delta".into();
    details.protocol = Protocol::Sftp;
    details.path = "relative".into();
    assert!(matches!(details.address(), Err(Error::Path)));
}
