//! File presentation shared by the window's directory tabs.

use super::*;
use gpui_kit::component::table::TableState;

pub(in crate::pages::workspace) struct State {
    pub table: Entity<TableState<table::FileTable>>,
    pub columns: Entity<columns::Editor>,
    pub marquee: marquee::State,
    pub preview: Entity<preview::View>,
    pub inspections: Entity<ui::file_inspection::Cache>,
}

impl State {
    pub(in crate::pages::workspace) fn new(window: &mut Window, cx: &mut Context<Browser>) -> Self {
        let inspections = cx.new(|_| ui::file_inspection::Cache::default());
        cx.observe(&inspections, |_, _, cx| cx.notify()).detach();
        Self {
            table: table::FileTable::make_state(cx.entity().downgrade(), window, cx),
            columns: cx.new(|_| columns::Editor::new()),
            marquee: marquee::State::default(),
            preview: cx.new(|cx| preview::View::new(inspections.clone(), cx)),
            inspections,
        }
    }
}
