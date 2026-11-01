use super::*;

#[test]
fn older_configuration_retains_terminal_defaults_and_round_trips() {
    let defaults: Preferences = serde_json::from_str("{}").expect("legacy empty preferences");
    assert_eq!(defaults, Preferences::default());
    let prefs: Preferences =
        serde_json::from_str(r#"{"font_size":18,"login_shell":true,"shell_args":["--norc"]}"#)
            .expect("partial preferences");
    assert_eq!(prefs.shell, None);
    assert_eq!(prefs.scrollback_lines, 10_000);
    assert_eq!(prefs.shell_args, ["--norc"]);
    let restored: Preferences =
        serde_json::from_str(&serde_json::to_string(&prefs).expect("serialize preferences"))
            .expect("restore preferences");
    assert_eq!(restored, prefs);
}

#[test]
fn malformed_sizes_and_relative_shell_are_rejected() {
    let mut prefs = Preferences::default();
    prefs.font_size = f32::NAN;
    assert!(prefs.validate().is_err());
    assert!(prefs.clone().normalized().font_size.is_finite());
    prefs.font_size = 14.;
    prefs.shell = Some(PathBuf::from("sh"));
    assert!(prefs.validate().is_err());
}

#[test]
fn terminal_catalog_coexists_with_application_messages() {
    use crate::i18n::{Catalog, Language};
    for (language, agents, files) in [
        (
            Language::English,
            include_str!("../../../../bin/agents/src/shared/i18n/assets/en-US/app.ftl"),
            include_str!("../../../../bin/files/src/shared/i18n/assets/en-US/app.ftl"),
        ),
        (
            Language::Chinese,
            include_str!("../../../../bin/agents/src/shared/i18n/assets/zh-CN/app.ftl"),
            include_str!("../../../../bin/files/src/shared/i18n/assets/zh-CN/app.ftl"),
        ),
    ] {
        for application in [agents, files] {
            let catalog = Catalog::new(language, &[application])
                .expect("shared terminal messages must not shadow application messages");
            assert!(catalog.text("gds-terminal-starting").is_some());
        }
    }
}
