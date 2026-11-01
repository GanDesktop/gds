use super::{Group, Path, Target, local_pipe};

#[test]
fn tcp_including_loopback_never_infers_local_cwd() {
    for address in [
        "tcp://127.0.0.1:7338",
        "tcp://localhost:7338",
        "tcp://192.0.2.1:7338",
        "mtls://example.com:7338",
    ] {
        let target: Target = address.parse().expect("connection target");
        assert!(
            Group::new(&target, Some(Path::new("/tmp"))).is_none(),
            "{address}"
        );
    }
}

#[cfg(unix)]
#[test]
fn groups_require_explicit_absolute_cwd_and_isolate_backends() {
    let first: Target = bas::service::Target::Unix("/tmp/first.sock".into()).into();
    let second: Target = bas::service::Target::Unix("/tmp/second.sock".into()).into();
    assert!(Group::new(&first, None).is_none());
    assert!(Group::new(&first, Some(Path::new(""))).is_none());
    assert!(Group::new(&first, Some(Path::new("project"))).is_none());
    let cwd = Some(Path::new("/tmp/project"));
    assert_ne!(Group::new(&first, cwd), Group::new(&second, cwd));
    assert_ne!(
        Group::new(&first, cwd),
        Group::new(&first, Some(Path::new("/tmp/other")))
    );
    assert_eq!(
        Group::new(&first, cwd),
        Group::new(&first, Some(Path::new("/tmp/./project/")))
    );
}

#[test]
fn remote_named_pipes_are_not_local_ipc() {
    assert!(local_pipe("bas-service"));
    assert!(local_pipe(r"\\.\pipe\bas-service"));
    assert!(!local_pipe(r"\\remote\pipe\bas-service"));
    assert!(!local_pipe("//remote/pipe/bas-service"));
}
