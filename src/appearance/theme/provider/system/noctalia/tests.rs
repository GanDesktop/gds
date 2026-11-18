use super::{self as noctalia, TEMPLATE};
use crate::appearance::theme::{Error, Mode, ModePreference};
use anti_webui::prelude::{Color, rgb};
use std::path::Path;

fn color(value: u32) -> Color {
    rgb(value)
}

fn rendered_template(active_mode: &str) -> String {
    let tokens = [
        ("{{ mode }}", active_mode),
        ("{{ colors.surface.dark.hex }}", "#101214"),
        ("{{ colors.on_surface.dark.hex }}", "#e2e2e6"),
        ("{{ colors.surface_container_low.dark.hex }}", "#181c20"),
        ("{{ colors.surface_container.dark.hex }}", "#1c2024"),
        ("{{ colors.surface_container_high.dark.hex }}", "#262a2f"),
        ("{{ colors.on_surface_variant.dark.hex }}", "#c2c7cf"),
        ("{{ colors.outline.dark.hex }}", "#8c9199"),
        ("{{ colors.outline_variant.dark.hex }}", "#42474e"),
        ("{{ colors.shadow.dark.hex }}", "#010203"),
        ("{{ colors.primary.dark.hex }}", "#9ecaff"),
        ("{{ colors.on_primary.dark.hex }}", "#003258"),
        ("{{ colors.primary_container.dark.hex }}", "#00497d"),
        ("{{ colors.on_primary_container.dark.hex }}", "#d1e4ff"),
        ("{{ colors.error.dark.hex }}", "#ffb4ab"),
        ("{{ colors.on_error.dark.hex }}", "#690005"),
        ("{{ colors.surface.light.hex }}", "#f8f9ff"),
        ("{{ colors.on_surface.light.hex }}", "#191c20"),
        ("{{ colors.surface_container_low.light.hex }}", "#f2f3fa"),
        ("{{ colors.surface_container.light.hex }}", "#eceef4"),
        ("{{ colors.surface_container_high.light.hex }}", "#e6e8ee"),
        ("{{ colors.on_surface_variant.light.hex }}", "#42474e"),
        ("{{ colors.outline.light.hex }}", "#72777f"),
        ("{{ colors.outline_variant.light.hex }}", "#c2c7cf"),
        ("{{ colors.shadow.light.hex }}", "#040506"),
        ("{{ colors.primary.light.hex }}", "#0061a4"),
        ("{{ colors.on_primary.light.hex }}", "#ffffff"),
        ("{{ colors.primary_container.light.hex }}", "#d1e4ff"),
        ("{{ colors.on_primary_container.light.hex }}", "#001d36"),
        ("{{ colors.error.light.hex }}", "#ba1a1a"),
        ("{{ colors.on_error.light.hex }}", "#ffffff"),
        ("{{ colors.terminal_foreground.dark.hex }}", "#d0d4dc"),
        ("{{ colors.terminal_background.dark.hex }}", "#0c0e11"),
        ("{{ colors.terminal_cursor.dark.hex }}", "#9ecaff"),
        ("{{ colors.terminal_cursor_text.dark.hex }}", "#003258"),
        ("{{ colors.terminal_selection_bg.dark.hex }}", "#243b55"),
        ("{{ colors.terminal_selection_fg.dark.hex }}", "#eef4ff"),
        ("{{ colors.terminal_normal_black.dark.hex }}", "#110000"),
        ("{{ colors.terminal_normal_red.dark.hex }}", "#110001"),
        ("{{ colors.terminal_normal_green.dark.hex }}", "#110002"),
        ("{{ colors.terminal_normal_yellow.dark.hex }}", "#110003"),
        ("{{ colors.terminal_normal_blue.dark.hex }}", "#110004"),
        ("{{ colors.terminal_normal_magenta.dark.hex }}", "#110005"),
        ("{{ colors.terminal_normal_cyan.dark.hex }}", "#110006"),
        ("{{ colors.terminal_normal_white.dark.hex }}", "#110007"),
        ("{{ colors.terminal_bright_black.dark.hex }}", "#220000"),
        ("{{ colors.terminal_bright_red.dark.hex }}", "#220001"),
        ("{{ colors.terminal_bright_green.dark.hex }}", "#220002"),
        ("{{ colors.terminal_bright_yellow.dark.hex }}", "#220003"),
        ("{{ colors.terminal_bright_blue.dark.hex }}", "#220004"),
        ("{{ colors.terminal_bright_magenta.dark.hex }}", "#220005"),
        ("{{ colors.terminal_bright_cyan.dark.hex }}", "#220006"),
        ("{{ colors.terminal_bright_white.dark.hex }}", "#220007"),
        ("{{ colors.terminal_foreground.light.hex }}", "#1e2228"),
        ("{{ colors.terminal_background.light.hex }}", "#f5f7fc"),
        ("{{ colors.terminal_cursor.light.hex }}", "#0061a4"),
        ("{{ colors.terminal_cursor_text.light.hex }}", "#ffffff"),
        ("{{ colors.terminal_selection_bg.light.hex }}", "#cfe4ff"),
        ("{{ colors.terminal_selection_fg.light.hex }}", "#082038"),
        ("{{ colors.terminal_normal_black.light.hex }}", "#aa0000"),
        ("{{ colors.terminal_normal_red.light.hex }}", "#aa0001"),
        ("{{ colors.terminal_normal_green.light.hex }}", "#aa0002"),
        ("{{ colors.terminal_normal_yellow.light.hex }}", "#aa0003"),
        ("{{ colors.terminal_normal_blue.light.hex }}", "#aa0004"),
        ("{{ colors.terminal_normal_magenta.light.hex }}", "#aa0005"),
        ("{{ colors.terminal_normal_cyan.light.hex }}", "#aa0006"),
        ("{{ colors.terminal_normal_white.light.hex }}", "#aa0007"),
        ("{{ colors.terminal_bright_black.light.hex }}", "#bb0000"),
        ("{{ colors.terminal_bright_red.light.hex }}", "#bb0001"),
        ("{{ colors.terminal_bright_green.light.hex }}", "#bb0002"),
        ("{{ colors.terminal_bright_yellow.light.hex }}", "#bb0003"),
        ("{{ colors.terminal_bright_blue.light.hex }}", "#bb0004"),
        ("{{ colors.terminal_bright_magenta.light.hex }}", "#bb0005"),
        ("{{ colors.terminal_bright_cyan.light.hex }}", "#bb0006"),
        ("{{ colors.terminal_bright_white.light.hex }}", "#bb0007"),
    ];
    tokens
        .into_iter()
        .fold(TEMPLATE.to_owned(), |rendered, (placeholder, value)| {
            rendered.replace(placeholder, value)
        })
}

#[test]
fn canonical_template_renders_valid_dark_and_light_palettes_including_terminal_roles() {
    let dark = noctalia::parse(
        &rendered_template("dark"),
        Path::new("noctalia.json"),
        ModePreference::System,
    )
    .expect("rendered dark Noctalia template");
    assert_eq!(dark.mode, Mode::Dark);
    assert_eq!(dark.palette.content, color(0x101214));
    assert_eq!(dark.palette.sidebar, color(0x181c20));
    assert_eq!(dark.palette.panel, color(0x262a2f));
    assert_eq!(dark.palette.selection, color(0x00497d));
    assert_eq!(dark.palette.selection_foreground, color(0xd1e4ff));
    assert_eq!(dark.palette.terminal.foreground, color(0xd0d4dc));
    assert_eq!(dark.palette.terminal.background, color(0x0c0e11));
    assert_eq!(dark.palette.terminal.cursor, color(0x9ecaff));
    assert_eq!(dark.palette.terminal.cursor_text, color(0x003258));
    assert_eq!(dark.palette.terminal.selection, color(0x243b55));
    assert_eq!(dark.palette.terminal.selection_text, color(0xeef4ff));
    assert_eq!(dark.palette.terminal.normal[0], color(0x110000));
    assert_eq!(dark.palette.terminal.normal[7], color(0x110007));
    assert_eq!(dark.palette.terminal.bright[0], color(0x220000));
    assert_eq!(dark.palette.terminal.bright[7], color(0x220007));

    let light_override = noctalia::parse(
        &rendered_template("dark"),
        Path::new("noctalia.json"),
        ModePreference::Light,
    )
    .expect("explicit light mode selects light UI and terminal schemes");
    assert_eq!(light_override.mode, Mode::Light);
    assert_eq!(light_override.palette.content, color(0xf8f9ff));
    assert_eq!(light_override.palette.terminal.background, color(0xf5f7fc));
    assert_eq!(light_override.palette.terminal.selection, color(0xcfe4ff));
    assert_eq!(
        light_override.palette.terminal.selection_text,
        color(0x082038)
    );
    assert_eq!(light_override.palette.terminal.normal[0], color(0xaa0000));
    assert_eq!(light_override.palette.terminal.bright[7], color(0xbb0007));
}

#[test]
fn terminal_section_is_optional_when_absent_and_atomic_when_present() {
    let minimal = r##"{
        "mode": "dark",
        "colors": {
            "dark": {
                "surface": "#101010",
                "on_surface": "#eeeeee",
                "primary": "#888888",
                "on_primary": "#111111"
            }
        }
    }"##;
    let fallback = noctalia::parse(minimal, Path::new("noctalia.json"), ModePreference::System)
        .expect("absent terminal section uses fallback");
    assert_eq!(
        fallback.palette.terminal.background,
        fallback.palette.content
    );
    assert_eq!(
        fallback.palette.terminal.foreground,
        fallback.palette.foreground
    );

    let mut null_terminal: serde_json::Value =
        serde_json::from_str(&rendered_template("dark")).expect("valid fixture");
    null_terminal["terminal"] = serde_json::Value::Null;
    assert!(
        noctalia::parse(
            &null_terminal.to_string(),
            Path::new("noctalia.json"),
            ModePreference::Dark
        )
        .is_err()
    );

    let mut short_ansi: serde_json::Value =
        serde_json::from_str(&rendered_template("dark")).expect("valid fixture");
    short_ansi["terminal"]["dark"]["normal"] = serde_json::json!(["#000000"]);
    assert!(
        noctalia::parse(
            &short_ansi.to_string(),
            Path::new("noctalia.json"),
            ModePreference::Dark
        )
        .is_err()
    );
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let mut future: serde_json::Value =
        serde_json::from_str(&rendered_template("dark")).expect("valid fixture");
    future["schema_version"] = serde_json::json!(2);
    assert!(matches!(
        noctalia::parse(
            &future.to_string(),
            Path::new("noctalia.json"),
            ModePreference::System
        ),
        Err(Error::Invalid { field, .. }) if field == "schema_version"
    ));
}

#[cfg(unix)]
fn accept_ipc_command(listener: &std::os::unix::net::UnixListener) -> String {
    use std::io::{Read, Write};
    let (mut stream, _) = listener.accept().expect("accept Noctalia IPC connection");
    let mut command = String::new();
    stream
        .read_to_string(&mut command)
        .expect("read Noctalia IPC command");
    stream
        .write_all(b"ok\n")
        .expect("acknowledge Noctalia IPC command");
    command
}

#[cfg(unix)]
#[test]
fn cold_start_provisions_template_and_renders_via_ipc_while_respecting_user_config() {
    let sandbox = tempfile::tempdir().expect("isolated Noctalia sandbox");
    let cache_dir = sandbox.path().join("cache");
    let config_dir = sandbox.path().join("config/noctalia");
    let socket = sandbox.path().join("noctalia-wayland-0.sock");
    let listener =
        std::os::unix::net::UnixListener::bind(&socket).expect("bind fake Noctalia socket");
    let output = noctalia::output_path(&cache_dir);
    let rendered = rendered_template("dark");
    let output_for_server = output.clone();

    let server = std::thread::spawn(move || {
        let reload = accept_ipc_command(&listener);
        let apply = accept_ipc_command(&listener);
        std::fs::create_dir_all(output_for_server.parent().expect("output parent"))
            .expect("create output parent");
        std::fs::write(&output_for_server, rendered).expect("write rendered Noctalia export");
        (reload, apply)
    });

    let env = noctalia::Environment {
        cache_dir: &cache_dir,
        config_dir: config_dir.clone(),
        socket: &socket,
    };
    let loaded = noctalia::load_with(&env, ModePreference::System)
        .expect("bootstrap Noctalia template and load rendered palette");
    let (reload, apply) = server.join().expect("IPC server thread");

    assert_eq!(reload, "config-reload");
    assert_eq!(apply, "templates-apply");
    assert_eq!(loaded.mode, Mode::Dark);
    assert_eq!(loaded.source_path.as_deref(), Some(output.as_path()));
    assert_eq!(
        std::fs::read_to_string(cache_dir.join("gds/theme/noctalia-template.json"))
            .expect("staged template"),
        TEMPLATE
    );

    let custom_config = "[theme.templates.user.gds]\ninput_path = \"/custom/gds.json\"\n";
    let config_path = config_dir.join("gds.toml");
    std::fs::write(&config_path, custom_config).expect("write custom user gds.toml");
    let steady = noctalia::load_with(&env, ModePreference::Light)
        .expect("steady-state load reuses existing export without IPC");
    assert_eq!(steady.mode, Mode::Light);
    assert_eq!(
        std::fs::read_to_string(&config_path).expect("read preserved user gds.toml"),
        custom_config
    );
}
