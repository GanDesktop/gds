use super::*;

#[test]
fn user_theme_catalog_and_palette_validation() {
    let root = tempfile::tempdir().expect("theme directory");
    std::fs::create_dir(root.path().join("personal")).expect("subdirectory");
    let text = include_str!("../builtin/assets/bamboo.toml");
    let path = root.path().join("personal/bamboo.toml");
    std::fs::write(&path, text).expect("theme");
    assert_eq!(
        catalog_at(root.path()).expect("catalog"),
        vec![Entry {
            path: PathBuf::from("personal/bamboo.toml"),
            metadata: definition(text, &path)
                .expect("definition")
                .metadata
                .expect("metadata"),
        }]
    );
    let catalog = catalog_at(root.path()).expect("catalog");
    let metadata = &catalog[0].metadata;
    assert_eq!(metadata.display_name("zh-CN"), "竹林");
    assert_eq!(metadata.display_name("fr-FR"), "Bamboo");
    assert!(
        parse(
            &text.replace("name = \"Bamboo\"", "name = \"\""),
            path.clone(),
            Mode::Dark
        )
        .is_err()
    );
    assert!(
        parse(
            &text.replace("schema_version = 1", "schema_version = 999"),
            path.clone(),
            Mode::Dark
        )
        .is_err()
    );
    assert!(
        parse(
            &text.replace("name = \"Bamboo\"", ""),
            path.clone(),
            Mode::Dark
        )
        .is_err()
    );
    assert!(
        parse(
            &text.replace("schema_version = 1", ""),
            path.clone(),
            Mode::Dark
        )
        .is_ok()
    );
    let light = parse(text, path.clone(), Mode::Light).expect("light variant");
    let dark = parse(text, path.clone(), Mode::Dark).expect("dark variant");
    assert_ne!(light.palette, dark.palette);
    assert_eq!(dark.source_path.as_ref(), Some(&path));
    assert!(
        parse(
            &text.replace("#f9fbf7", "invalid"),
            path.clone(),
            Mode::Light
        )
        .is_err()
    );
    assert!(parse("[dark]", path, Mode::Dark).is_err());
    assert!(resolve(Path::new("../app/files/settings.toml")).is_err());
}
