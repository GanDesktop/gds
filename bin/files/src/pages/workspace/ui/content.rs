//! File presentation shared by the window's directory tabs.

use super::*;
use gpui_kit::component::table::TableState;

pub(super) struct State {
    pub table: Entity<TableState<table::FileTable>>,
    pub columns: Entity<columns::Editor>,
    pub marquee: marquee::State,
    pub preview: Entity<preview::View>,
    pub thumbnails: Entity<ui::thumbnail::Cache>,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Browser>) -> Self {
        let thumbnails = cx.new(|_| ui::thumbnail::Cache::default());
        cx.observe(&thumbnails, |_, _, cx| cx.notify()).detach();
        Self {
            table: table::FileTable::make_state(cx.entity().downgrade(), window, cx),
            columns: cx.new(|_| columns::Editor::new()),
            marquee: marquee::State::default(),
            preview: cx.new(|cx| preview::View::new(thumbnails.clone(), cx)),
            thumbnails,
        }
    }
}
