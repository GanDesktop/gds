use super::*;
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn saved_names_match_all_credentials_and_prefer_the_selected_profile(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("settings directory");
    cx.update(|cx| {
        let profiles = cx.new(|_| Profiles::new(Store::at(directory.path().join("settings.json"))));
        profiles.update(cx, |profiles, cx| {
            let mut first = ConnectionProfile {
                id: "first".into(),
                name: "Production".into(),
                note: None,
                target: "mtls://hostname".parse().expect("target"),
                tls: Some(bas::service::tls::ClientTls {
                    ca: "/ca.pem".into(),
                    certificate: "/client.pem".into(),
                    private_key: "/client.key".into(),
                    server_name: None,
                }),
            };
            profiles.put(first.clone(), cx).expect("first");
            let options = first.options();
            first.id = "second".into();
            first.name = "Alternate name".into();
            profiles.put(first.clone(), cx).expect("second");
            assert_eq!(profiles.matching(&options).unwrap().name, "Production");
            profiles.select("second", cx).expect("select");
            assert_eq!(profiles.matching(&options).unwrap().name, "Alternate name");
            for field in ["ca", "certificate", "private_key", "server_name"] {
                let mut alternate = options.clone();
                let tls = alternate.tls.as_mut().expect("TLS");
                match field {
                    "ca" => tls.ca = "/other-ca.pem".into(),
                    "certificate" => tls.certificate = "/other-client.pem".into(),
                    "private_key" => tls.private_key = "/other-client.key".into(),
                    _ => tls.server_name = Some("other.example".into()),
                }
                assert!(profiles.matching(&alternate).is_none(), "{field}");
            }
        });
    });
}

#[test]
fn invalid_saved_tls_configuration_never_falls_back_to_local() {
    let directory = tempfile::tempdir().expect("settings directory");
    let store = Store::at(directory.path().join("settings.json"));
    std::fs::write(
        store.path(),
        r#"{
        "frontend": {
            "selected": "secure",
            "connections": [{"id":"secure","name":"Secure","target":"mtls://server:7338"}]
        }
    }"#,
    )
    .expect("incomplete TLS configuration");
    let profiles = Profiles::new(store);
    assert!(profiles.startup_target(None).is_err());
    assert!(
        profiles
            .startup_target(Some("mtls://server:7338".parse().expect("target")))
            .is_err()
    );
    assert_eq!(
        profiles
            .startup_target(Some(Target::default()))
            .expect("deliberate local override"),
        Target::default(),
    );
}

#[gpui_kit::test]
fn deleting_profile_preserves_a_valid_default_and_never_deletes_the_last_one(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().join("settings.json"));
    cx.update(|cx| {
        let profiles = cx.new(|_| Profiles::new(store.clone()));
        profiles.update(cx, |profiles, cx| {
            assert!(profiles.remove("local", cx).is_err());
            profiles
                .put(
                    ConnectionProfile {
                        id: "other".into(),
                        name: "Other".into(),
                        note: None,
                        target: bas::service::Target::Pipe("bas-other".into()),
                        tls: None,
                    },
                    cx,
                )
                .unwrap();
            assert_eq!(profiles.remove("other", cx).unwrap(), Target::default());
            assert_eq!(profiles.selected().unwrap().id, "local");
            assert_eq!(store.load().unwrap().settings.frontend.connections.len(), 1);
        });
    });
}
