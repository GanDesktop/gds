use super::Panel;
use gpui_kit::{AppContext, TestAppContext, px, size};

#[gpui_kit::test]
async fn closing_a_tab_removes_it_before_its_terminal_finishes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).unwrap();
        gds::appearance::theme::apply(&Default::default(), cx).unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let mut panel = None;
    let window = cx.open_window(size(px(800.), px(500.)), |window, cx| {
        let view = cx.new(Panel::new);
        panel = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let panel = panel.unwrap();
    let shutdown = cx
        .update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(directory.path().to_owned(), window, cx);
                let entry = &panel.entries[0];
                let id = entry.id;
                let terminal = entry.terminal.clone();
                panel.request_close(id, window, cx);
                assert!(
                    panel.entries.is_empty(),
                    "remove the tab without polling cleanup"
                );
                assert!(panel.active.is_none());
                terminal.update(cx, |terminal, cx| terminal.shutdown(cx))
            })
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .unwrap();
    shutdown.await;
}
