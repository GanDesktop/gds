use super::*;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use zbus::zvariant::Value;

struct Portal {
    reduced_motion: Arc<AtomicU32>,
    color_scheme: Arc<AtomicU32>,
    accent_color: Arc<Mutex<(f64, f64, f64)>>,
    contrast: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Portal {
    fn read_all(&self, namespaces: Vec<String>) -> Settings {
        assert_eq!(namespaces, [NAMESPACE]);
        let accent = *self.accent_color.lock().expect("accent color test state");
        let accent_value = Value::from(accent)
            .try_to_owned()
            .expect("valid DBus struct accent color");
        HashMap::from([(
            NAMESPACE.into(),
            HashMap::from([
                (
                    REDUCED_MOTION_KEY.into(),
                    self.reduced_motion.load(Ordering::SeqCst).into(),
                ),
                (
                    COLOR_SCHEME_KEY.into(),
                    self.color_scheme.load(Ordering::SeqCst).into(),
                ),
                (ACCENT_COLOR_KEY.into(), accent_value),
                (
                    CONTRAST_KEY.into(),
                    self.contrast.load(Ordering::SeqCst).into(),
                ),
            ]),
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
    let reduced = Arc::new(AtomicU32::new(1));
    let color_scheme = Arc::new(AtomicU32::new(1));
    let accent_color = Arc::new(Mutex::new((0.2, 0.5, 0.9)));
    let contrast = Arc::new(AtomicU32::new(0));
    let server = zbus::connection::Builder::unix_stream(server_socket)
        .server(zbus::Guid::generate())
        .expect("server guid")
        .p2p()
        .serve_at(
            "/org/freedesktop/portal/desktop",
            Portal {
                reduced_motion: reduced.clone(),
                color_scheme: color_scheme.clone(),
                accent_color: accent_color.clone(),
                contrast: contrast.clone(),
            },
        )
        .expect("portal interface")
        .build();
    let client = zbus::connection::Builder::unix_stream(client_socket)
        .p2p()
        .build();
    let (service, client) = tokio::try_join!(server, client).expect("private portal peers");
    let (sender, mut receiver) = watch::channel(None);
    let task = tokio::spawn(listen_on(client, sender));

    tokio::time::timeout(Duration::from_secs(5), receiver.changed())
        .await
        .expect("initial portal response deadline")
        .expect("reader alive");
    assert_eq!(
        receiver.borrow_and_update().map(|u| u.reduced_motion),
        Some(true)
    );
    let initial = snapshot();
    assert!(initial.primed);
    assert_eq!(initial.color_scheme, Some(Mode::Dark));
    assert_eq!(initial.accent_color, Some(Color::srgb(0.2, 0.5, 0.9, 1.0)));
    assert_eq!(initial.contrast, Contrast::Normal);

    let interface = service
        .object_server()
        .interface::<_, Portal>("/org/freedesktop/portal/desktop")
        .await
        .expect("private portal fixture");

    for (
        signal_key,
        next_reduced,
        next_scheme,
        next_accent,
        next_contrast,
        expected_reduced,
        expected_scheme,
        expected_accent,
        expected_contrast,
        expected_contrast_changed,
    ) in [
        (
            COLOR_SCHEME_KEY,
            0,
            2,
            (-1.0, -1.0, -1.0),
            1,
            false,
            Some(Mode::Light),
            None,
            Contrast::More,
            true,
        ),
        (
            CONTRAST_KEY,
            1,
            0,
            (0.8, 0.3, 0.1),
            0,
            true,
            None,
            Some(Color::srgb(0.8, 0.3, 0.1, 1.0)),
            Contrast::Normal,
            true,
        ),
    ] {
        reduced.store(next_reduced, Ordering::SeqCst);
        color_scheme.store(next_scheme, Ordering::SeqCst);
        *accent_color.lock().expect("accent lock") = next_accent;
        contrast.store(next_contrast, Ordering::SeqCst);
        let signal_value: OwnedValue = if signal_key == CONTRAST_KEY {
            next_contrast.into()
        } else {
            next_scheme.into()
        };
        Portal::setting_changed(
            interface.signal_emitter(),
            NAMESPACE,
            signal_key,
            signal_value,
        )
        .await
        .expect("private portal signal");

        tokio::time::timeout(Duration::from_secs(5), receiver.changed())
            .await
            .expect("portal update deadline")
            .expect("reader alive");
        let update = receiver.borrow_and_update().expect("portal update");
        assert_eq!(update.reduced_motion, expected_reduced);
        assert!(update.appearance_changed);
        assert_eq!(update.contrast_changed, expected_contrast_changed);
        let current = snapshot();
        assert_eq!(current.color_scheme, expected_scheme);
        assert_eq!(current.accent_color, expected_accent);
        assert_eq!(current.contrast, expected_contrast);
    }

    task.abort();
    assert!(task.await.expect_err("listener cancelled").is_cancelled());
}
