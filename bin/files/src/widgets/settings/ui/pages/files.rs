use gds::i18n::t;
use gds::ui::settings::SettingDescription;
use gds_uim::el;
use gpui_kit::*;

use crate::widgets::settings::ui::{View, controls::Field};

impl View {
    pub(super) fn render_files(&mut self, cx: &mut Context<Self>) -> Div {
        let row = self.toggle_row(Field::ShowHidden, false, cx);
        let volumes_row = self.toggle_row(Field::SidebarVolumes, false, cx);
        let search_row = self.toggle_row(Field::AutoFocusSearch, false, cx);
        let budget_label = format!("{}", self.preferences.files.probe_budget).into();
        let budget_control =
            self.number_control(Field::ProbeBudget)
                .render(Field::ProbeBudget, budget_label, cx);
        let budget_row = self.setting_row(Field::ProbeBudget, budget_control, cx);
        let budget_note = el! { .SettingDescription::default [$(t!(@cx, "probe-budget-help"))] };
        el! { .div [row, volumes_row, search_row, budget_row, budget_note] }
    }
}
