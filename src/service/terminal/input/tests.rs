use super::*;

fn key(name: &str, modifiers: Modifiers) -> Keystroke<'_> {
    Keystroke {
        key: name,
        key_char: Some(name),
        modifiers,
    }
}

#[test]
fn printable_text_is_left_to_ime_and_shell_controls_are_not_app_actions() {
    assert_eq!(
        key_bytes(&key("a", Modifiers::default()), TermMode::NONE, true),
        None
    );
    assert_eq!(
        key_bytes(&key("中", Modifiers::default()), TermMode::NONE, true),
        None
    );
    for (name, byte) in [("c", 3), ("w", 23), ("n", 14), ("b", 2), ("l", 12)] {
        assert_eq!(
            key_bytes(
                &key(
                    name,
                    Modifiers {
                        control: true,
                        ..Default::default()
                    }
                ),
                TermMode::NONE,
                true
            ),
            Some(vec![byte])
        );
    }
    for name in [",", "`"] {
        assert_eq!(
            key_bytes(
                &key(
                    name,
                    Modifiers {
                        control: true,
                        ..Default::default()
                    }
                ),
                TermMode::NONE,
                true
            ),
            None
        );
    }
    assert_eq!(
        key_bytes(
            &key(
                "€",
                Modifiers {
                    control: true,
                    alt: true,
                    ..Default::default()
                }
            ),
            TermMode::NONE,
            true
        ),
        None
    );
}

#[test]
fn cursor_and_newline_modes_change_wire_sequences() {
    // F6 belongs to the PTY protocol unless its host explicitly binds navigation.
    assert_eq!(
        key_bytes(&key("f6", Modifiers::default()), TermMode::NONE, true),
        Some(b"\x1b[17~".to_vec())
    );
    let up = key("up", Modifiers::default());
    assert_eq!(
        key_bytes(&up, TermMode::NONE, true),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
        key_bytes(&up, TermMode::APP_CURSOR, true),
        Some(b"\x1bOA".to_vec())
    );
    let modified = key(
        "left",
        Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(
        key_bytes(&modified, TermMode::APP_CURSOR, true),
        Some(b"\x1b[1;6D".to_vec())
    );
    assert_eq!(
        key_bytes(
            &key("enter", Modifiers::default()),
            TermMode::LINE_FEED_NEW_LINE,
            true
        ),
        Some(b"\r\n".to_vec())
    );
}

#[test]
fn bracketed_paste_cannot_be_terminated_by_pasted_escape() {
    let text = "echo 中文\r\n\x1b[201~danger\rnext";
    assert_eq!(
        paste_bytes(text, TermMode::BRACKETED_PASTE),
        "\x1b[200~echo 中文\n[201~danger\nnext\x1b[201~".as_bytes()
    );
    assert_eq!(paste_bytes("a\r\nb\nc", TermMode::NONE), b"a\rb\rc");
}

#[test]
fn mouse_sgr_release_and_legacy_bounds_are_explicit() {
    assert_eq!(
        mouse_bytes(0, 4, 7, true, Modifiers::default(), TermMode::SGR_MOUSE),
        Some(b"\x1b[<0;5;8m".to_vec())
    );
    assert!(mouse_bytes(0, 223, 0, false, Modifiers::default(), TermMode::NONE).is_none());
}

#[cfg(unix)]
#[test]
fn dropped_paths_round_trip_without_shell_expansion_or_submission() {
    let paths = [
        "/tmp/a b",
        "/tmp/a'b",
        "/tmp/a\\b",
        "/tmp/中文 $(echo injected); $HOME",
    ]
    .map(std::path::PathBuf::from);
    let text = super::path_text(&paths).expect("quoted paths");
    assert!(!text.contains(['\n', '\r']));
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("set -- {text}; printf '%s\\0' \"$@\""))
        .output()
        .expect("POSIX shell");
    assert!(output.status.success());
    let expected = paths
        .iter()
        .flat_map(|p| p.to_str().unwrap().bytes().chain([0]))
        .collect::<Vec<_>>();
    assert_eq!(output.stdout, expected);
    assert_eq!(
        super::paste_bytes(&text, TermMode::empty()),
        text.as_bytes()
    );
    assert_eq!(
        super::paste_bytes(&text, TermMode::BRACKETED_PASTE),
        format!("\x1b[200~{text}\x1b[201~").as_bytes()
    );
}
