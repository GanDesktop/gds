use super::*;
use crate::shared::i18n::Language;
use gds::{
    appearance::theme::{ModePreference, Source},
    ui::time::Format,
};

fn fixture() -> tempfile::TempDir {
    tempfile::tempdir().expect("create preferences fixture")
}

fn application_path(root: &tempfile::TempDir) -> PathBuf {
    root.path().join("gds/files/settings.toml")
}

#[test]
fn missing_configurations_use_defaults_without_creating_any_file() {
    let root = fixture();
    let application = application_path(&root);
    let loaded = load_from(&application).expect("load missing application preferences");
    assert_eq!(loaded, Preferences::default());
    assert!(!application.exists());
}

#[test]
fn saves_and_reloads_the_complete_preferences_including_cleared_language() {
    let root = fixture();
    let path = application_path(&root);
    let mut preferences = Preferences::default();
    preferences.appearance.source = Source::Qt6ct;
    preferences.appearance.builtin = gds::appearance::theme::BuiltinTheme::Neon;
    preferences.appearance.mode = ModePreference::Light;
    preferences.appearance.content_opacity = 0.75;
    preferences.appearance.sidebar_opacity = 0.85;
    preferences.language = Some(Language::Chinese);
    preferences.time_format = Format::Absolute;
    preferences.animations_enabled = Some(false);
    preferences.font.family = Some("Noto Sans".to_owned());
    preferences.font.size = 18.0;
    preferences.files.show_hidden = true;
    preferences.terminal_enabled = Some(false);
    preferences.terminal.shell = Some("/usr/bin/fish".into());
    preferences.terminal.shell_args =
        vec!["--private".into(), "literal argument with spaces".into()];
    preferences.terminal.login_shell = true;
    preferences.terminal.font_family = "Terminal Mono".into();
    preferences.terminal.font_size = 16.;
    preferences.terminal.scrollback_lines = 25_000;
    preferences.terminal.cursor_blink = false;
    preferences.terminal.alt_sends_escape = false;
    preferences.files.terminal_content_height = Some(420.);
    preferences.files.sidebar.tabs_height = Some(284.);
    preferences.files.sidebar.volumes_collapsed = true;
    preferences.files.sidebar.locations_collapsed = true;
    preferences.files.sidebar.show_volumes = true;
    preferences
        .files
        .volume_mount_points
        .insert("filesystem-identity".into(), "/nix/store".into());

    save_to(&path, &preferences).expect("save preferences");
    let reloaded = load_from(&path).expect("reload preferences");
    assert_eq!(reloaded, preferences);
    assert_eq!(reloaded.time_format, Format::Absolute);
    assert_eq!(reloaded.font.family.as_deref(), Some("Noto Sans"));
    assert_eq!(reloaded.font.size, 18.0);

    preferences.language = None;
    preferences.files.show_hidden = false;
    save_to(&path, &preferences).expect("replace preferences");
    let reloaded = load_from(&path).expect("reload replacement");
    assert_eq!(reloaded, preferences);
    assert_eq!(reloaded.time_format, Format::Absolute);
}

#[test]
fn omitted_fields_receive_defaults_but_malformed_configuration_is_an_error() {
    let root = fixture();
    let path = root.path().join("settings.toml");
    fs::write(&path, "[files]\nshow_hidden = true\n").expect("write partial preferences");
    let preferences = load_from(&path).expect("read supported partial preferences");
    assert!(preferences.files.show_hidden);
    assert!(preferences.terminal_enabled());
    assert_eq!(preferences.appearance, Preferences::default().appearance);
    assert_eq!(preferences.language, None);
    assert_eq!(preferences.time_format, Format::Relative);
    assert!(preferences.animations_enabled.unwrap_or(true));
    assert_eq!(preferences.font, Preferences::default().font);
    assert_eq!(
        preferences.terminal,
        gds::ui::terminal::Preferences::default()
    );
    assert_eq!(preferences.files.terminal_content_height, None);

    let invalid = "[files\nshow_hidden = ?";
    fs::write(&path, invalid).expect("write malformed preferences");
    assert!(matches!(load_from(&path), Err(Error::Parse { .. })));
    assert_eq!(
        fs::read_to_string(&path).expect("read unchanged file"),
        invalid
    );
}

#[test]
fn partial_terminal_preferences_keep_shared_defaults_and_literal_arguments() {
    let root = fixture();
    let path = application_path(&root);
    fs::create_dir_all(path.parent().expect("preferences parent")).expect("create preferences dir");
    fs::write(
        &path,
        "[terminal]\nlogin_shell = true\nshell_args = ['--flag', 'two words', '$(not executed)']\n",
    )
    .expect("write partial terminal preferences");
    let preferences = load_from(&path).expect("load partial terminal preferences");
    assert!(preferences.terminal.login_shell);
    assert_eq!(preferences.terminal.shell, None);
    assert_eq!(
        preferences.terminal.font_size,
        gds::ui::terminal::Preferences::default().font_size
    );
    assert_eq!(
        preferences.terminal.shell_args,
        ["--flag", "two words", "$(not executed)"]
    );
    save_to(&path, &preferences).expect("save terminal configuration");
    assert_eq!(
        load_from(&path).expect("reload terminal configuration"),
        preferences
    );
}

#[test]
fn replacement_leaves_only_the_complete_configuration_behind() {
    let root = fixture();
    let path = application_path(&root);
    save_to(&path, &Preferences::default()).expect("save preferences");
    let mut preferences = Preferences::default();
    preferences.files.show_hidden = true;
    save_to(&path, &preferences).expect("replace preferences");

    let directory = path.parent().expect("application directory");
    let entries: Vec<_> = fs::read_dir(directory)
        .expect("read application directory")
        .map(|entry| {
            entry
                .expect("read directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(entries.contains(&"settings.toml".to_owned()));
    assert!(entries.contains(&"settings.toml.lock".to_owned()));
    assert_eq!(load_from(&path).expect("reload preferences"), preferences);
}

#[cfg(unix)]
#[test]
fn readonly_configuration_is_not_replaced_even_when_the_directory_is_writable() {
    use std::os::unix::fs::PermissionsExt;

    let root = fixture();
    let path = root.path().join("settings.toml");
    let contents = "[files]\nshow_hidden = true\n";
    fs::write(&path, contents).expect("write managed preferences");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444))
        .expect("make preferences readonly");

    assert!(matches!(
        save_to(&path, &Preferences::default()),
        Err(Error::Persistence(gds::persistence::Error::ReadOnly(_)))
    ));
    assert_eq!(
        fs::read_to_string(&path).expect("read unchanged file"),
        contents
    );
    assert!(
        fs::metadata(&path)
            .expect("read permissions")
            .permissions()
            .readonly()
    );
}

#[cfg(unix)]
#[test]
fn declarative_symlinks_can_be_read_but_cannot_be_replaced() {
    use std::os::unix::fs::symlink;

    let root = fixture();
    let target = root.path().join("managed.toml");
    let path = root.path().join("settings.toml");
    let contents = "[files]\nshow_hidden = true\n";
    fs::write(&target, contents).expect("write managed target");
    symlink(&target, &path).expect("link declarative preferences");

    assert!(
        load_from(&path)
            .expect("read declarative preferences")
            .files
            .show_hidden
    );
    assert!(matches!(
        save_to(&path, &Preferences::default()),
        Err(Error::Persistence(gds::persistence::Error::Symlink(_)))
    ));
    assert_eq!(fs::read_link(&path).expect("retain symlink"), target);
    assert_eq!(
        fs::read_to_string(&target).expect("read unchanged target"),
        contents
    );
}

#[cfg(unix)]
#[test]
fn a_dangling_configuration_link_is_reported_instead_of_reset_to_defaults() {
    use std::os::unix::fs::symlink;

    let root = fixture();
    let target = root.path().join("missing-managed.toml");
    let path = root.path().join("settings.toml");
    symlink(&target, &path).expect("create dangling configuration link");

    assert!(matches!(load_from(&path), Err(Error::Read { .. })));
    assert!(matches!(
        save_to(&path, &Preferences::default()),
        Err(Error::Persistence(gds::persistence::Error::Symlink(_)))
    ));
    assert_eq!(fs::read_link(&path).expect("retain dangling link"), target);
    assert!(!target.exists());
}
