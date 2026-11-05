use super::*;

#[tokio::test]
async fn reads_standalone_options_and_isolates_unavailable_values() {
    let directory = tempfile::tempdir().expect("temporary flake");
    std::fs::write(
        directory.path().join("flake.nix"),
        include_str!("assets/test-flake.nix"),
    )
    .expect("write fixture");
    let path = directory.path().to_str().expect("temporary path is UTF-8");
    let targets = discover(path)
        .await
        .expect("discover fixture with nix installed");
    assert_eq!(targets.len(), 1);
    for target in targets {
        let options = target.options(path).await.expect("read options");
        assert_eq!(options.len(), 4);
        assert!(options.iter().all(|option| {
            option
                .path
                .starts_with(&["programs".into(), "example".into()])
                && option.path.len() == 3
        }));
        let path = options
            .iter()
            .find(|option| option.path == ["programs", "example", "path"])
            .expect("path option");
        assert!(
            path.current
                .value
                .as_ref()
                .expect("store path")
                .data
                .as_str()
                .expect("string")
                .starts_with("/nix/store/")
        );
        let enabled = options
            .iter()
            .find(|option| option.path == ["programs", "example", "enable"])
            .expect("enable option");
        assert_eq!(
            enabled.current.value.as_ref().expect("boolean value").data,
            Value::Bool(true)
        );
        let missing = options
            .iter()
            .find(|option| option.path == ["programs", "example", "missing"])
            .expect("missing option");
        assert!(!missing.current.available);
        let structured = options
            .iter()
            .find(|option| option.path == ["programs", "example", "settings"])
            .expect("settings option");
        assert_eq!(
            structured
                .current
                .value
                .as_ref()
                .expect("attribute summary")
                .data,
            serde_json::json!(["editor"])
        );
    }
    assert!(
        !directory.path().join("flake.lock").exists(),
        "preview must not create a lockfile"
    );
}

#[tokio::test]
async fn reads_nixos_options_in_each_users_module_context() {
    let directory = tempfile::tempdir().expect("temporary flake");
    let fixture = include_str!("assets/test-nixos-flake.nix").replace(
        "@nixpkgs-lock@",
        &nix_string(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/flake.lock"
        ))),
    );
    std::fs::write(directory.path().join("flake.nix"), fixture).expect("write fixture");
    let path = directory.path().to_str().expect("UTF-8 path");
    let targets = discover(path).await.expect("discover real submodules");
    assert_eq!(targets.len(), 2);
    for target in targets {
        let options = target
            .options(path)
            .await
            .expect("evaluate configured user options");
        let version = if target.name == "alice" {
            "25.11"
        } else {
            "26.05"
        };
        let editor = options
            .iter()
            .find(|option| option.path == ["programs", "editor", "version"])
            .expect("config-dependent option");
        assert_eq!(
            editor.current.value.as_ref().expect("current version").data,
            version
        );
        assert_eq!(
            editor.default.value.as_ref().expect("default version").data,
            version
        );
        let username = options
            .iter()
            .find(|option| option.path == ["home", "username"])
            .expect("username");
        assert_eq!(
            username
                .current
                .value
                .as_ref()
                .expect("username value")
                .data,
            target.name
        );
        let custom = options
            .iter()
            .find(|option| option.path == ["customFeature", "customFlag"])
            .expect("user-defined option");
        assert_eq!(
            custom.current.value.as_ref().expect("custom value").data,
            true
        );
        let context = options
            .iter()
            .find(|option| option.path == ["programs", "editor", "hostContext"])
            .expect("special argument");
        assert_eq!(
            context.current.value.as_ref().expect("host context").data,
            "workstation"
        );
        assert!(
            options
                .iter()
                .all(|option| !option.path[0].starts_with('_'))
        );
    }
    assert!(!directory.path().join("flake.lock").exists());
}
