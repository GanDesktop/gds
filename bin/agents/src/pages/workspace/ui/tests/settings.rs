use super::*;

#[gpui_kit::test]
fn nested_management_pages_return_one_level_without_replacing_the_workspace(
    cx: &mut TestAppContext,
) {
    let mut fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.composer.input.update(cx, |input, cx| {
                input.set_value("keep this draft", window, cx)
            });
            view.open_settings(window, cx);
        });
    })
    .expect("open settings");
    cx.run_until_parked();
    let about =
        cx.update(|cx| crate::features::navigation::open_about(cx).expect("embedded about"));
    assert_eq!(about, fixture.window);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("about-version").visible());
        assert!(window.try_find("dialog-cancel").is_none());
        window.click("about-credits", cx);
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.find("about-version").visible());
        window.click("page-return", cx);
        window.render_frame(cx);
        assert!(window.find("settings").visible());
    })
    .expect("about returns to settings");
    let catalog = cx
        .update(|cx| crate::widgets::settings::window::open_catalog(cx).expect("embedded catalog"));
    assert_eq!(catalog, fixture.window);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("agent-installation-page").visible());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.find("settings").visible());
        window.click("settings-return", cx);
    })
    .expect("catalog returns through settings");
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(cx.windows().len(), 1);
        let view = fixture.view.read(cx);
        assert!(view.window.presented);
        assert_eq!(view.composer.input.read(cx).value(), "keep this draft");
    });
    assert!(fixture.commands.try_recv().is_err());
}

#[gpui_kit::test]
fn window_style_switch_is_top_right_and_management_returns_to_compact_mode(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        let control = window.find("toggle-mode").bounds().center();
        assert!(control.x > window.viewport_size().width / 2.);
        assert!(control.y < window.viewport_size().height / 2.);
        window.click("toggle-mode", cx);
        assert!(fixture.view.read(cx).window.compact);
        fixture.view.update(cx, |view, cx| {
            view.composer
                .input
                .update(cx, |input, cx| input.set_value("compact draft", window, cx));
            view.open_agent_catalog(window, cx);
        });
    })
    .expect("compact mode and catalog");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("agent-installation-page").visible());
        window.click("page-return", cx);
    })
    .expect("return to compact workspace");
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(cx.windows().len(), 1);
        let view = fixture.view.read(cx);
        assert!(view.window.presented && view.window.compact);
        assert_eq!(view.composer.input.read(cx).value(), "compact draft");
    });
}

#[gpui_kit::test]
fn closing_the_window_from_settings_closes_its_host(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let settings = fixture
        .window
        .update(cx, |root, window, cx| {
            let host = crate::shared::lib::diagnostics::content(root, cx)
                .downcast::<crate::pages::workspace::windows::WorkspaceHost>()
                .expect("workspace host");
            host.update(cx, |host, cx| host.show_settings(window, cx))
        })
        .expect("open settings");
    cx.run_until_parked();
    // TestPlatform uses server decorations, so it deliberately has no custom
    // close button. Exercise the event emitted by that button on client windows.
    cx.update(|cx| {
        settings.update(cx, |_, cx| {
            cx.emit(crate::widgets::settings::Event::CloseWindow)
        });
    });
    cx.run_until_parked();
    cx.update(|cx| assert!(cx.windows().is_empty()));
}

#[gpui_kit::test]
fn settings_return_to_the_same_workspace_and_keep_the_unsent_draft(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.view.update(cx, |view, cx| {
            view.composer
                .input
                .update(cx, |input, cx| input.set_value("Unsent draft", window, cx));
            view.open_settings(window, cx);
        });
    })
    .expect("open settings from the workspace");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(cx.windows().len(), 1));
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("settings").visible());
        assert!(!fixture.view.read(cx).window.presented);
        window.click("settings-return", cx);
    })
    .expect("return from settings");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = fixture.view.read(cx);
        assert!(view.window.presented);
        assert_eq!(
            view.composer.input.read(cx).value().as_ref(),
            "Unsent draft"
        );
        assert!(window.find("conversation-header-row").visible());
    })
    .expect("original workspace remains alive");
    assert!(fixture.commands.try_recv().is_err());
}

#[gpui_kit::test]
fn settings_cycle_pages_without_dispatching_workspace_shortcuts(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture
            .view
            .update(cx, |view, cx| view.open_settings(window, cx));
    })
    .expect("settings request");
    cx.run_until_parked();
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-tab", cx);
        window.render_frame(cx);
        assert!(window.find("appearance-composer-enter").visible());
        window.press("ctrl-n", cx);
        window.press("ctrl-shift-tab", cx);
        window.render_frame(cx);
        assert!(window.find("settings-reset").visible());
        assert!(window.try_find("appearance-composer-enter").is_none());
        window.click("settings-keybindings", cx);
        window.render_frame(cx);
        assert!(window.find("settings-keybindings-page").visible());
        window.click("settings-search", cx);
        window.input("archive", cx);
        window.render_frame(cx);
        assert!(window.find("settings-keybindings-page").visible());
        window.press("ctrl-tab", cx);
        window.render_frame(cx);
        assert!(window.find("local-terminal-settings").visible());
        window.press("ctrl-tab", cx);
        window.render_frame(cx);
        assert!(window.find("acp-policy-page").visible());
        window.press("escape", cx);
    })
    .expect("page navigation and return");
    cx.run_until_parked();
    assert!(fixture.commands.try_recv().is_err());
    cx.update(|cx| assert_eq!(cx.windows().len(), 1));
    cx.update_window(fixture.window.into(), |_, _, cx| {
        assert!(fixture.view.read(cx).window.presented)
    })
    .expect("workspace restored");
}
