use super::{Command, SETTINGS_CONTEXT, VOLUME_CONTEXT, WORKSPACE_CONTEXT, bindings};
use gpui_kit::{AsKeystroke, KeyContext, Keymap, Keystroke, TestAppContext};

fn context(source: &'static str) -> Vec<KeyContext> {
    vec![KeyContext::parse(source).expect("bundled contexts are valid")]
}

fn keystroke(source: &'static str) -> Keystroke {
    Keystroke::parse(source).expect("bundled keystrokes are valid")
}

/// The refresh keycap follows the primary stroke while the compatibility one
/// keeps working, and neither leaks into the settings workspace.
#[gpui_kit::test]
fn refresh_displays_ctrl_r_and_keeps_f5(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    let action = Command::Refresh.action();

    let displayed = keymap
        .bindings_for_action(action.as_ref())
        .last()
        .expect("refresh is bound");
    let displayed = displayed.keystrokes()[0].as_keystroke();
    assert!(displayed.modifiers.control);
    assert_eq!(displayed.key, "r");

    for source in [Command::Refresh.default_stroke(), "f5"] {
        let (resolved, pending) =
            keymap.bindings_for_input(&[keystroke(source)], &context(WORKSPACE_CONTEXT));
        assert!(!pending, "{source} is not a prefix of a longer binding");
        assert_eq!(resolved.len(), 1, "{source} resolves one binding");
        assert!(
            resolved[0].action().partial_eq(action.as_ref()),
            "{source} runs refresh"
        );

        let (resolved, _) =
            keymap.bindings_for_input(&[keystroke(source)], &context(SETTINGS_CONTEXT));
        assert!(resolved.is_empty(), "{source} stays out of settings");
    }
}

/// `Alt+M` is the displayed menu key, `F10` keeps working, and both run the one
/// menu action in the file workspace only.
#[gpui_kit::test]
fn menu_displays_alt_m_and_keeps_f10(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    let action = Command::Menu.action();

    let displayed = keymap
        .bindings_for_action(action.as_ref())
        .last()
        .expect("the menu is bound");
    let displayed = displayed.keystrokes()[0].as_keystroke();
    assert!(displayed.modifiers.alt);
    assert_eq!(displayed.key, "m");

    for source in [Command::Menu.default_stroke(), "f10"] {
        let (resolved, pending) =
            keymap.bindings_for_input(&[keystroke(source)], &context(WORKSPACE_CONTEXT));
        assert!(!pending, "{source} is not a prefix of a longer binding");
        assert_eq!(resolved.len(), 1, "{source} resolves one binding");
        assert!(
            resolved[0].action().partial_eq(action.as_ref()),
            "{source} opens the menu"
        );

        let (resolved, _) =
            keymap.bindings_for_input(&[keystroke(source)], &context(SETTINGS_CONTEXT));
        assert!(resolved.is_empty(), "{source} stays out of settings");
    }
}

/// Mounting keys belong to volume pages. In a picker, another location or
/// Settings the same stroke cannot act on a previously selected volume.
#[gpui_kit::test]
fn volume_operations_only_resolve_in_the_volume_context(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    for (command, stroke) in [
        (Command::MountVolume, "ctrl-m"),
        (Command::UnmountVolume, "ctrl-shift-m"),
    ] {
        let stroke = keystroke(stroke);
        let action = command.action();
        let (resolved, pending) =
            keymap.bindings_for_input(&[stroke.clone()], &context("Workspace Volume"));
        assert!(!pending);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].action().partial_eq(action.as_ref()));
        assert_eq!(command.context(), Some(VOLUME_CONTEXT));
        for source in [WORKSPACE_CONTEXT, SETTINGS_CONTEXT, "Workspace Picker"] {
            let (resolved, _) = keymap.bindings_for_input(&[stroke.clone()], &context(source));
            assert!(
                resolved.is_empty(),
                "{source} must not act on a hidden volume"
            );
        }
    }
    for old in ["alt-g", "alt-u"] {
        let (resolved, _) =
            keymap.bindings_for_input(&[keystroke(old)], &context("Workspace Volume"));
        assert!(resolved.is_empty(), "the old volume binding is removed");
    }
}

#[gpui_kit::test]
fn selection_commands_do_not_claim_settings_or_plain_f10(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    for (stroke, command) in [
        ("ctrl-shift-a", Command::DeselectAll),
        ("shift-f10", Command::ContextMenu),
        ("f10", Command::Menu),
    ] {
        let action = command.action();
        let (resolved, pending) =
            keymap.bindings_for_input(&[keystroke(stroke)], &context(WORKSPACE_CONTEXT));
        assert!(!pending);
        assert!(resolved[0].action().partial_eq(action.as_ref()));
        let (resolved, _) =
            keymap.bindings_for_input(&[keystroke(stroke)], &context(SETTINGS_CONTEXT));
        assert!(resolved.is_empty());
    }
}

#[gpui_kit::test]
fn popup_keys_take_precedence_over_workspace_commands(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    let cases: [(&str, Box<dyn gpui_kit::Action>); 2] = [
        ("escape", Box::new(gpui_kit::base::actions::Cancel)),
        (
            "enter",
            Box::new(gpui_kit::base::actions::Confirm { secondary: false }),
        ),
    ];
    for (stroke, expected) in cases {
        let (resolved, _) = keymap.bindings_for_input(
            &[keystroke(stroke)],
            &[
                KeyContext::parse("Workspace Volume").expect("valid workspace context"),
                KeyContext::parse("PopupMenu").expect("valid popup context"),
            ],
        );
        assert!(
            resolved
                .first()
                .expect("popup key is bound")
                .action()
                .partial_eq(expected.as_ref())
        );
    }
}

#[gpui_kit::test]
fn direct_tab_numbers_leave_location_shortcuts_unchanged(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    for (stroke, action) in [
        (
            "ctrl-1",
            Box::new(super::SelectTab1) as Box<dyn gpui_kit::Action>,
        ),
        ("ctrl-9", Box::new(super::SelectTab9)),
    ] {
        let (resolved, pending) =
            keymap.bindings_for_input(&[keystroke(stroke)], &context(WORKSPACE_CONTEXT));
        assert!(!pending);
        assert!(resolved[0].action().partial_eq(action.as_ref()));
    }
    let (resolved, _) =
        keymap.bindings_for_input(&[keystroke("ctrl-v")], &context(WORKSPACE_CONTEXT));
    assert!(
        resolved[0]
            .action()
            .partial_eq(Command::Paste.action().as_ref())
    );
}

#[gpui_kit::test]
fn region_access_keys_preserve_address_editing_and_column_configuration(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    for (stroke, command) in [
        ("alt-l", Command::FocusLocations),
        ("alt-v", Command::FocusVolumes),
        ("alt-t", Command::FocusTabs),
        ("alt-space", Command::FocusContent),
        ("ctrl-l", Command::EditLocation),
        ("alt-shift-v", Command::Columns),
        ("ctrl-g", Command::ViewMode),
        ("alt-shift-s", Command::Sort),
    ] {
        let (resolved, pending) =
            keymap.bindings_for_input(&[keystroke(stroke)], &context(WORKSPACE_CONTEXT));
        assert!(!pending);
        assert_eq!(resolved.len(), 1, "{stroke} has one target");
        assert!(resolved[0].action().partial_eq(command.action().as_ref()));
        let (resolved, _) =
            keymap.bindings_for_input(&[keystroke(stroke)], &context(SETTINGS_CONTEXT));
        assert!(
            resolved.is_empty(),
            "file region commands stay out of settings"
        );
    }
}

#[gpui_kit::test]
fn file_bindings_do_not_capture_terminal_input_or_terminal_tabs(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    for terminal_context in [
        "GdsTerminal",
        super::TERMINAL_TABS_CONTEXT,
        super::TERMINAL_CONTEXT,
    ] {
        // A file or Volume ancestor must not override the terminal.
        let contexts = vec![
            KeyContext::parse("Workspace Volume").expect("workspace context"),
            KeyContext::parse(terminal_context).expect("terminal context"),
        ];
        for stroke in [
            "f2",
            "ctrl-c",
            "ctrl-d",
            "ctrl-n",
            "ctrl-l",
            "ctrl-r",
            "ctrl-w",
            "ctrl-b",
            "ctrl-h",
            "ctrl-g",
            "alt-shift-s",
            "ctrl-shift-c",
            "ctrl-v",
            "ctrl-z",
            "ctrl-shift-z",
            "ctrl-tab",
            "escape",
            "delete",
            "shift-delete",
            "up",
            "down",
            "left",
            "right",
        ] {
            let (resolved, pending) = keymap.bindings_for_input(&[keystroke(stroke)], &contexts);
            for binding in &resolved {
                for command in Command::ALL.into_iter().chain([Command::Cancel]) {
                    assert!(
                        !binding.action().partial_eq(command.action().as_ref()),
                        "{terminal_context}: {stroke} must not invoke a file command"
                    );
                }
            }
            assert!(!pending, "terminal keys must resolve immediately");
        }
        for command in [
            Command::Settings,
            Command::Terminal,
            Command::FocusNotifications,
            Command::CloseNotification,
            Command::CloseAllNotifications,
        ] {
            let (resolved, pending) =
                keymap.bindings_for_input(&[keystroke(command.default_stroke())], &contexts);
            assert!(!pending);
            assert_eq!(resolved.len(), 1);
            assert!(resolved[0].action().partial_eq(command.action().as_ref()));
        }
    }
}

#[gpui_kit::test]
fn files_owns_terminal_capability_bindings_and_tab_navigation(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    let inputs = vec![
        KeyContext::parse("Workspace").unwrap(),
        KeyContext::parse(super::TERMINAL_CONTEXT).unwrap(),
        KeyContext::parse("GdsTerminal").unwrap(),
    ];
    let tabs = context("Workspace FilesTerminalTabs");
    let input_actions: [(&str, Box<dyn gpui_kit::Action>); 10] = [
        ("f2", Box::new(super::RenameTerminal)),
        ("ctrl-n", Box::new(super::NewTerminal)),
        ("ctrl-w", Box::new(super::CloseTerminal)),
        ("ctrl-shift-c", Box::new(gds::ui::terminal::Copy)),
        ("ctrl-shift-v", Box::new(gds::ui::terminal::Paste)),
        ("shift-pageup", Box::new(gds::ui::terminal::ScrollPageUp)),
        (
            "shift-pagedown",
            Box::new(gds::ui::terminal::ScrollPageDown),
        ),
        ("shift-home", Box::new(gds::ui::terminal::ScrollTop)),
        ("shift-end", Box::new(gds::ui::terminal::ScrollBottom)),
        ("f6", Box::new(super::ReturnFromTerminal)),
    ];
    for (stroke, expected) in input_actions {
        let (resolved, pending) = keymap.bindings_for_input(&[keystroke(stroke)], &inputs);
        assert!(!pending);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].action().partial_eq(expected.as_ref()));
    }
    let tab_actions: [(&str, Box<dyn gpui_kit::Action>); 7] = [
        ("f2", Box::new(super::RenameTerminal)),
        ("ctrl-n", Box::new(super::NewTerminal)),
        ("ctrl-w", Box::new(super::CloseTerminal)),
        ("up", Box::new(super::PreviousTerminal)),
        ("down", Box::new(super::NextTerminal)),
        ("enter", Box::new(super::FocusTerminal)),
        ("f6", Box::new(super::ReturnFromTerminal)),
    ];
    for (stroke, expected) in tab_actions {
        let (resolved, pending) = keymap.bindings_for_input(&[keystroke(stroke)], &tabs);
        assert!(!pending);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].action().partial_eq(expected.as_ref()));
        if !matches!(stroke, "f6" | "f2" | "ctrl-n" | "ctrl-w") {
            assert!(
                keymap
                    .bindings_for_input(&[keystroke(stroke)], &inputs)
                    .0
                    .is_empty(),
                "PTY input keeps {stroke}"
            );
        }
    }
}

#[gpui_kit::test]
fn notification_access_keys_work_from_both_workspaces(cx: &mut TestAppContext) {
    let keymap = Keymap::new(cx.update(|cx| bindings(cx)));
    for source in [
        WORKSPACE_CONTEXT,
        SETTINGS_CONTEXT,
        "Workspace Notifications",
    ] {
        for (stroke, command) in [
            ("alt-n", Command::FocusNotifications),
            ("alt-x", Command::CloseNotification),
            ("alt-shift-x", Command::CloseAllNotifications),
        ] {
            let (resolved, pending) =
                keymap.bindings_for_input(&[keystroke(stroke)], &context(source));
            assert!(!pending);
            assert_eq!(resolved.len(), 1);
            assert!(resolved[0].action().partial_eq(command.action().as_ref()));
        }
    }
}
