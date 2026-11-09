use super::*;
#[test]
fn stat_preserves_spaces_in_local_paths() {
    assert_eq!(
        field("LOCAL_PATH            /tmp/a b \n", "LOCAL_PATH"),
        Some("/tmp/a b ")
    );
    assert_eq!(
        field("DISPLAY_TYPE          Tag Fragment\n", "DISPLAY_TYPE"),
        Some("Tag Fragment")
    );
    assert_eq!(field("LOCAL_PATH_OTHER      bad\n", "LOCAL_PATH"), None);
}
#[test]
fn explicit_missing_command_does_not_fall_back() {
    assert!(resolve(Some(Path::new("/missing/gds-test-baloo")), &["sh"]).is_err());
}

#[cfg(unix)]
mod command_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(root: &Path, name: &str, body: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn commands(root: &Path, kio_body: &str) -> Commands {
        Commands {
            control: Some(script(
                root,
                "baloo control",
                "case \"$1\" in --version) echo Baloo;; status) echo 'Total files indexed: 1';; *) exit 1;; esac",
            )),
            kio_client: Some(script(root, "kio client", kio_body)),
            ..Default::default()
        }
    }

    #[test]
    fn discovers_available_version_from_path_and_respects_custom_override() {
        let root = tempfile::tempdir().unwrap();
        let executable = script(root.path(), "balooctl", "exit 0");
        assert_eq!(
            resolve_in_path(None, &["balooctl6", "balooctl"], root.path().as_os_str()).unwrap(),
            executable
        );
        assert!(
            resolve_in_path(
                Some(Path::new("missing")),
                &["balooctl"],
                root.path().as_os_str()
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn resolves_virtual_folders_and_real_files_without_shell_interpolation() {
        let root = tempfile::tempdir().unwrap();
        let commands = commands(
            root.path(),
            r#"
case "$1:$2" in
    --version:) echo KIO;;
    ls:tags:/) printf '.\n工作\n';;
    stat:tags:/%E5%B7%A5%E4%BD%9C) printf 'DISPLAY_TYPE          Tag\n';;
    ls:tags:/%E5%B7%A5%E4%BD%9C) printf '.\ndesign\na b;$(false).txt\n';;
    stat:tags:/%E5%B7%A5%E4%BD%9C/design) printf 'DISPLAY_TYPE          Tag Fragment\n';;
    stat:tags:/%E5%B7%A5%E4%BD%9C/a%20b%3B%24%28false%29%2Etxt) printf 'LOCAL_PATH            /tmp/a b;$(false).txt\n';;
    ls:tags:/empty) :;;
    *) echo "unexpected arguments: $*" >&2; exit 9;;
esac
"#,
        );
        let backend = Baloo::connect(&commands).await.unwrap();
        assert_eq!(backend.browse("").await.unwrap().tags, ["工作"]);
        let listing = backend.browse("工作").await.unwrap();
        assert_eq!(listing.tags, ["工作/design"]);
        assert_eq!(listing.files, [PathBuf::from("/tmp/a b;$(false).txt")]);
        assert!(backend.browse("empty").await.unwrap().files.is_empty());
        assert!(
            backend
                .browse("failed")
                .await
                .unwrap_err()
                .contains("unexpected arguments")
        );
    }

    #[tokio::test]
    async fn complete_backend_requires_working_control_client_and_tags_worker() {
        let root = tempfile::tempdir().unwrap();
        let mut commands = commands(
            root.path(),
            "if [ \"$1\" = --version ]; then echo KIO; else echo 'Unknown protocol tags' >&2; exit 1; fi",
        );
        assert!(
            Baloo::connect(&commands)
                .await
                .unwrap_err()
                .contains("Unknown protocol tags")
        );
        commands.control = Some(script(
            root.path(),
            "disabled",
            "if [ \"$1\" = --version ]; then echo Baloo; else echo 'Baloo is disabled'; fi",
        ));
        assert!(
            Baloo::connect(&commands)
                .await
                .unwrap_err()
                .contains("disabled")
        );
        let not_executable = root.path().join("plain file");
        std::fs::write(&not_executable, "not executable").unwrap();
        commands.control = Some(not_executable);
        assert!(
            Baloo::connect(&commands)
                .await
                .unwrap_err()
                .contains("Executable not found")
        );
    }

    /// Supply isolated Baloo data and the installed tags worker in the process environment.
    #[tokio::test]
    #[ignore = "requires GDS_TEST_BALOOCTL, GDS_TEST_KIOCLIENT, GDS_TEST_TAGGED_FILE and isolated BALOO_DB_PATH"]
    async fn real_baloo_catalogue_and_local_targets() {
        let commands = Commands {
            control: Some(
                std::env::var_os("GDS_TEST_BALOOCTL")
                    .expect("Baloo executable")
                    .into(),
            ),
            kio_client: Some(
                std::env::var_os("GDS_TEST_KIOCLIENT")
                    .expect("KIO executable")
                    .into(),
            ),
            ..Default::default()
        };
        let target = PathBuf::from(std::env::var_os("GDS_TEST_TAGGED_FILE").expect("tagged file"));
        let backend = Baloo::connect(&commands).await.unwrap();
        assert!(
            backend
                .browse("")
                .await
                .unwrap()
                .tags
                .contains(&"工作".into())
        );
        assert!(
            backend
                .browse("工作")
                .await
                .unwrap()
                .files
                .contains(&target)
        );
        assert!(
            backend
                .browse("Project")
                .await
                .unwrap()
                .tags
                .contains(&"Project/design".into())
        );
        assert!(
            backend
                .browse("Project/design")
                .await
                .unwrap()
                .files
                .contains(&target)
        );
        assert!(
            backend
                .browse("space tag")
                .await
                .unwrap()
                .files
                .contains(&target)
        );
    }
}
