use super::*;

#[test]
fn frontend_updates_preserve_other_sections_and_reject_stale_edits() {
    let directory = tempfile::tempdir().expect("settings directory");
    let store = Store::at(directory.path().join("settings.json"));
    std::fs::write(
        store.path(),
        r#"{"daemon":{"unknown":"server-owned"},"future":true}"#,
    )
    .expect("legacy document");
    let mut appearance = store.load().expect("frontend ignores daemon schema");
    let mut connections = store.load().expect("second editor");
    let candidate = Appearance {
        ui_font_size: Some(19.),
        ..Default::default()
    };
    store
        .save_appearance(&mut appearance, candidate.clone())
        .expect("appearance save");
    let mut frontend = connections.settings.frontend.clone();
    frontend.connections[0].name = "My local service".into();
    store
        .save_frontend(&mut connections, frontend)
        .expect("independent section merges");
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.path()).expect("settings")).expect("JSON");
    assert_eq!(raw["daemon"]["unknown"], "server-owned");
    assert_eq!(raw["future"], true);
    assert_eq!(
        store.load().expect("merged document").settings.appearance,
        candidate
    );
    assert!(matches!(
        store.save_appearance(&mut appearance, Appearance::default()),
        Ok(())
    ));
    assert!(matches!(
        store.save_appearance(&mut connections, candidate),
        Err(Error::Conflict { .. })
    ));
}

#[test]
fn terminal_settings_merge_legacy_sections_and_reject_stale_writers() {
    let directory = tempfile::tempdir().expect("settings directory");
    let store = Store::at(directory.path().join("settings.json"));
    std::fs::write(store.path(), r#"{"daemon":{"untouched":true},"future":42}"#)
        .expect("legacy settings");
    let mut first = store.load().expect("defaults for absent terminal");
    let mut stale = store.load().expect("second editor");
    assert_eq!(
        first.settings.terminal,
        gds::ui::terminal::Preferences::default()
    );
    let candidate = gds::ui::terminal::Preferences {
        font_size: 18.,
        login_shell: true,
        ..Default::default()
    };
    store
        .save_terminal(&mut first, candidate.clone())
        .expect("terminal save");
    let mut appearance = stale.settings.appearance.clone();
    appearance.ui_font_size = Some(20.);
    store
        .save_appearance(&mut stale, appearance)
        .expect("independent section merge");
    assert_eq!(store.load().expect("reload").settings.terminal, candidate);
    let mut conflict = first.clone();
    store
        .save_terminal(&mut first, gds::ui::terminal::Preferences::default())
        .expect("new revision");
    assert!(matches!(
        store.save_terminal(&mut conflict, candidate),
        Err(Error::Conflict {
            section: "terminal",
            ..
        })
    ));
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.path()).expect("settings bytes"))
            .expect("JSON");
    assert_eq!(raw["daemon"]["untouched"], true);
    assert_eq!(raw["future"], 42);
}

#[cfg(unix)]
#[test]
fn managed_settings_can_be_read_but_never_replaced() {
    let directory = tempfile::tempdir().expect("settings directory");
    let source = directory.path().join("managed.json");
    let path = directory.path().join("settings.json");
    std::fs::write(&source, "{}").expect("managed source");
    std::os::unix::fs::symlink(&source, &path).expect("managed link");
    let store = Store::at(path.clone());
    let mut document = store.load().expect("read managed settings");
    assert!(
        store
            .save_appearance(&mut document, Appearance::default())
            .is_err()
    );
    assert!(path.is_symlink());
    assert_eq!(std::fs::read_to_string(source).expect("source"), "{}");
}
