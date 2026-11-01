use super::*;

#[gpui_kit::test]
fn icon_toggle_preserves_the_acp_value_and_blocks_a_second_request_until_acknowledged(
    cx: &mut TestAppContext,
) {
    let mut fixture = fixture(cx);
    let mut meta = session();
    meta.acp = Some(
        bas::acp::NewSessionResponse::new(meta.id.to_string()).config_options(vec![
            bas::acp::SessionConfigOption::boolean("fast_mode", "Fast mode", false),
        ]),
    );
    attach(&fixture, meta.clone(), cx);
    while fixture.commands.try_recv().is_ok() {}
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("config-fast_mode", cx);
        window.render_frame(cx);
        assert!(fixture.view.read(cx).sessions.pending.configuring.contains(&meta.id));
        assert_eq!(window.find("config-fast_mode").disabled(), Some(true));
        let request = fixture.commands.try_recv().expect("configuration request");
        assert!(matches!(request, Command::SetConfig(request)
            if request == bas::acp::SetSessionConfigOptionRequest::new(meta.id.to_string(), "fast_mode", true)));
        assert!(fixture.commands.try_recv().is_err());
        fixture.view.update(cx, |view, cx| {
            view.update(Update::Event {
                epoch: view.selected.epoch,
                event: Event::new(meta.id, Cursor::from_raw(1), EventKind::Acp(
                    bas::acp::SessionUpdate::ConfigOptionUpdate(bas::acp::ConfigOptionUpdate::new(vec![
                        bas::acp::SessionConfigOption::boolean("fast_mode", "Fast mode", true),
                    ])),
                )).into(),
            }, window, cx);
            view.configured(meta.id, Ok(bas::acp::SetSessionConfigOptionResponse::new(vec![
                bas::acp::SessionConfigOption::boolean("fast_mode", "Fast mode", true),
            ])));
            cx.notify();
        });
        window.render_frame(cx);
        window.click("config-fast_mode", cx);
        let request = fixture.commands.try_recv().expect("disable configuration request");
        assert!(matches!(request, Command::SetConfig(request)
            if request == bas::acp::SetSessionConfigOptionRequest::new(meta.id.to_string(), "fast_mode", false)));
    }).expect("ACP toggle workflow");
}
