use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

struct Portal(Arc<AtomicU32>);
#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Portal {
    fn read_all(&self, namespaces: Vec<String>) -> Settings {
        assert_eq!(namespaces, [NAMESPACE]);
        HashMap::from([(
            NAMESPACE.into(),
            HashMap::from([(KEY.into(), self.0.load(Ordering::SeqCst).into())]),
        )])
    }

    #[zbus(signal)]
    async fn setting_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        namespace: &str,
        key: &str,
        value: OwnedValue,
    ) -> zbus::Result<()>;
}

#[tokio::test]
async fn portal_snapshot_and_live_changes_share_one_subscription() {
    // In-process D-Bus peers exercise the real protocol without a daemon or
    // fork inheriting unrelated file locks from concurrently running tests.
    let (server_socket, client_socket) = tokio::net::UnixStream::pair().expect("portal sockets");
    let state = Arc::new(AtomicU32::new(1));
    let server = zbus::connection::Builder::unix_stream(server_socket)
        .server(zbus::Guid::generate())
        .expect("server guid")
        .p2p()
        .serve_at("/org/freedesktop/portal/desktop", Portal(state.clone()))
        .expect("portal interface")
        .build();
    let client = zbus::connection::Builder::unix_stream(client_socket)
        .p2p()
        .build();
    let (service, client) = tokio::try_join!(server, client).expect("private portal peers");
    let (sender, mut receiver) = watch::channel(None);
    let task = tokio::spawn(listen_on(client, sender));
    for expected in [true, false, true] {
        tokio::time::timeout(Duration::from_secs(5), receiver.changed())
            .await
            .expect("portal response deadline")
            .expect("reader alive");
        assert_eq!(*receiver.borrow_and_update(), Some(expected));
        if expected == false {
            state.store(1, Ordering::SeqCst);
        } else {
            state.store(0, Ordering::SeqCst);
        }
        let interface = service
            .object_server()
            .interface::<_, Portal>("/org/freedesktop/portal/desktop")
            .await
            .expect("private portal fixture");
        Portal::setting_changed(
            interface.signal_emitter(),
            NAMESPACE,
            KEY,
            state.load(Ordering::SeqCst).into(),
        )
        .await
        .expect("private portal fixture");
    }
    task.abort();
    assert!(task.await.expect_err("listener cancelled").is_cancelled());
}
